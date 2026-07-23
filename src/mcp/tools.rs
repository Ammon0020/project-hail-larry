//! MCP tool enumeration for diagnostics and future Settings UI.
//!
//! # Caching strategy (locked)
//!
//! **Lazy on first access, cached until MCP config change.** There is no TTL —
//! only explicit invalidation after `mcp.json` writes (PUT/PATCH) clears the
//! cache. Recorded in `docs/plans/OpenItems.md`.
//!
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

use super::{expand_env, File, ServerConfig, Transport};

/// Default wall-clock budget for one server's initialize + `tools/list` round-trip.
const ENUMERATE_TIMEOUT: Duration = Duration::from_secs(8);

/// Deterministic map of MCP server name → tool names that server exposes.
pub type ServerTools = BTreeMap<String, Vec<String>>;

/// Cached MCP tool catalog for future REST/UI diagnostics.
///
/// Holds an optional snapshot so callers can read synchronously (empty when
/// never populated) and refresh asynchronously via [`Self::enumerate`].
#[derive(Debug, Default)]
pub struct ToolCatalog {
    /// `None` = invalidated / never enumerated; `Some` = last successful pass
    /// (possibly with empty per-server lists after isolated failures).
    cache: RwLock<Option<ServerTools>>,
    /// Counts live `tools/list` attempts (one increment per server per refresh).
    /// Used by unit tests to assert cache hits avoid re-listing.
    list_calls: AtomicUsize,
}

impl ToolCatalog {
    /// Creates an empty catalog (cache cold).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps the catalog in an [`Arc`] for shared AppState / ClientDeps wiring.
    #[must_use]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Drops the cached snapshot so the next [`Self::enumerate`] re-lists.
    ///
    /// Call after any successful MCP config write (servers added/removed/toggled).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.cache.write() {
            *guard = None;
            tracing::debug!("MCP tool catalog cache invalidated");
        } else {
            tracing::error!("MCP tool catalog cache lock poisoned on invalidate");
        }
    }

    /// Returns the cached map, or empty when the cache is cold/invalidated.
    ///
    /// Synchronous so REST handlers can serve a snapshot without awaiting.
    #[must_use]
    pub fn cached(&self) -> ServerTools {
        self.cache
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// True when a snapshot is present (does not mean tools are non-empty).
    #[must_use]
    pub fn is_cached(&self) -> bool {
        self.cache
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Number of per-server list attempts since construction (test/metrics aid).
    #[must_use]
    pub fn list_call_count(&self) -> usize {
        self.list_calls.load(Ordering::Relaxed)
    }

    /// Enumerates tools on every enabled server using the live transport lister.
    ///
    /// Returns the new snapshot and stores it in the cache. Isolated failures
    /// yield an empty tool list for that server (logged), not a hard error.
    pub async fn enumerate(&self, file: &File) -> ServerTools {
        self.enumerate_with(file, &LiveToolLister).await
    }

    /// Like [`Self::enumerate`] but with an injectable lister (unit tests).
    pub async fn enumerate_with<L: ToolLister + ?Sized>(
        &self,
        file: &File,
        lister: &L,
    ) -> ServerTools {
        if let Ok(guard) = self.cache.read() {
            if let Some(hit) = guard.as_ref() {
                tracing::debug!(
                    servers = hit.len(),
                    "MCP tool catalog cache hit; skipping tools/list"
                );
                return hit.clone();
            }
        }

        let mut result = ServerTools::new();
        for (name, config) in file.enabled() {
            self.list_calls.fetch_add(1, Ordering::Relaxed);
            match lister.list_tools(name, config).await {
                Ok(mut tools) => {
                    tools.sort();
                    tools.dedup();
                    tracing::debug!(
                        server = %name,
                        tool_count = tools.len(),
                        "enumerated MCP tools"
                    );
                    result.insert(name.to_owned(), tools);
                }
                Err(error) => {
                    // Failure isolation: one broken server must not block others.
                    tracing::warn!(
                        server = %name,
                        %error,
                        "MCP tools/list failed; treating server as exposing no tools"
                    );
                    result.insert(name.to_owned(), Vec::new());
                }
            }
        }

        if let Ok(mut guard) = self.cache.write() {
            *guard = Some(result.clone());
        } else {
            tracing::error!("MCP tool catalog cache lock poisoned on store");
        }
        result
    }
}

/// Abstracts `tools/list` so tests can mock without spawning real MCP servers.
#[async_trait]
pub trait ToolLister: Send + Sync {
    /// Lists tool names for one configured server.
    ///
    /// Errors are converted to an empty list by [`ToolCatalog::enumerate_with`].
    async fn list_tools(&self, name: &str, config: &ServerConfig) -> Result<Vec<String>, String>;
}

/// Production lister: stdio JSON-RPC (Content-Length) and HTTP JSON-RPC POST.
pub struct LiveToolLister;

#[async_trait]
impl ToolLister for LiveToolLister {
    async fn list_tools(&self, name: &str, config: &ServerConfig) -> Result<Vec<String>, String> {
        match config.effective_transport().map_err(|e| e.to_string())? {
            Transport::Stdio => list_tools_stdio(name, config).await,
            Transport::Http | Transport::Sse => list_tools_http(name, config).await,
        }
    }
}

/// Filters a capability-selected server list with the active profile whitelist.
async fn list_tools_stdio(name: &str, config: &ServerConfig) -> Result<Vec<String>, String> {
    let command = expand_env(&config.command);
    if command.is_empty() {
        return Err("no command configured".to_owned());
    }

    let mut cmd = Command::new(&command);
    cmd.args(config.args.iter().map(|arg| expand_env(arg)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (key, value) in &config.env {
        cmd.env(key, expand_env(value));
    }
    if !config.cwd.is_empty() {
        cmd.current_dir(expand_env(&config.cwd));
    }

    let mut child = cmd
        .spawn()
        .map_err(|error| format!("spawn MCP server `{name}`: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("MCP server `{name}` missing stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("MCP server `{name}` missing stdout"))?;
    let mut reader = BufReader::new(stdout);

    let work = async {
        write_mcp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "local-agent", "version": "1.0" }
                }
            }),
        )
        .await?;
        let _init = read_mcp_message(&mut reader).await?;

        write_mcp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }),
        )
        .await?;

        write_mcp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": {}
            }),
        )
        .await?;
        let list_msg = read_mcp_message(&mut reader).await?;
        parse_tools_list_result(&list_msg)
    };

    let result = timeout(ENUMERATE_TIMEOUT, work)
        .await
        .map_err(|_| format!("MCP tools/list timed out for `{name}`"))?;

    // Best-effort teardown; kill_on_drop covers the drop path.
    let _ = child.kill().await;
    result
}

async fn list_tools_http(name: &str, config: &ServerConfig) -> Result<Vec<String>, String> {
    let url = expand_env(&config.url);
    if url.is_empty() {
        return Err("no URL configured".to_owned());
    }

    let client = reqwest::Client::builder()
        .timeout(ENUMERATE_TIMEOUT)
        .build()
        .map_err(|error| format!("HTTP client: {error}"))?;

    let mut request = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        );
    for (header_name, value) in &config.headers {
        request = request.header(header_name.as_str(), expand_env(value));
    }

    // Streamable HTTP / simple JSON-RPC POST: initialize then tools/list.
    // Session headers (mcp-session-id) are forwarded when the server returns them.
    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "local-agent", "version": "1.0" }
        }
    });
    let init_resp = request
        .try_clone()
        .ok_or_else(|| "HTTP request not cloneable".to_owned())?
        .json(&init_body)
        .send()
        .await
        .map_err(|error| format!("initialize `{name}`: {error}"))?;
    let session_id = init_resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let _ = init_resp
        .bytes()
        .await
        .map_err(|error| format!("initialize body `{name}`: {error}"))?;

    let mut list_req = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        );
    for (header_name, value) in &config.headers {
        list_req = list_req.header(header_name.as_str(), expand_env(value));
    }
    if let Some(session_id) = session_id {
        list_req = list_req.header("mcp-session-id", session_id);
    }

    let list_body = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let list_resp = list_req
        .json(&list_body)
        .send()
        .await
        .map_err(|error| format!("tools/list `{name}`: {error}"))?;
    let text = list_resp
        .text()
        .await
        .map_err(|error| format!("tools/list body `{name}`: {error}"))?;
    // Some transports wrap JSON in SSE `data:` lines — strip that if present.
    let json_text = extract_json_payload(&text);
    let value: Value = serde_json::from_str(json_text)
        .map_err(|error| format!("tools/list JSON `{name}`: {error}"))?;
    parse_tools_list_result(&value)
}

fn extract_json_payload(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        return trimmed;
    }
    // Last `data: {...}` line wins for simple SSE frames.
    trimmed
        .lines()
        .rev()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("data:")
                .map(str::trim)
                .filter(|payload| payload.starts_with('{'))
        })
        .unwrap_or(trimmed)
}

fn parse_tools_list_result(message: &Value) -> Result<Vec<String>, String> {
    if let Some(error) = message.get("error") {
        return Err(format!("tools/list error: {error}"));
    }
    let tools = message
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .ok_or_else(|| "tools/list response missing result.tools".to_owned())?;
    let mut names = Vec::with_capacity(tools.len());
    for tool in tools {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

async fn write_mcp_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<(), String> {
    let body =
        serde_json::to_vec(value).map_err(|error| format!("serialize MCP message: {error}"))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|error| format!("write MCP header: {error}"))?;
    writer
        .write_all(&body)
        .await
        .map_err(|error| format!("write MCP body: {error}"))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("flush MCP stdin: {error}"))?;
    Ok(())
}

async fn read_mcp_message<R: AsyncReadExt + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Value, String> {
    // Skip non-header noise; accept Content-Length framing (MCP stdio / LSP style).
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|error| format!("read MCP header line: {error}"))?;
        if n == 0 {
            return Err("MCP server closed stdout before responding".to_owned());
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed
            .split_once(':')
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .map(|(_, v)| v.trim())
        {
            content_length = Some(
                value
                    .parse()
                    .map_err(|_| format!("invalid Content-Length: {value}"))?,
            );
        }
    }
    let len = content_length.ok_or_else(|| "MCP response missing Content-Length".to_owned())?;
    if len > 8 * 1024 * 1024 {
        return Err(format!("MCP response too large: {len} bytes"));
    }
    let mut buf = vec![0_u8; len];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|error| format!("read MCP body: {error}"))?;
    serde_json::from_slice(&buf).map_err(|error| format!("parse MCP JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockLister {
        /// server → Ok(tools) or Err
        responses: BTreeMap<String, Result<Vec<String>, String>>,
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ToolLister for MockLister {
        async fn list_tools(
            &self,
            name: &str,
            _config: &ServerConfig,
        ) -> Result<Vec<String>, String> {
            self.calls.lock().expect("lock").push(name.to_owned());
            self.responses
                .get(name)
                .cloned()
                .unwrap_or_else(|| Err(format!("unexpected server {name}")))
        }
    }

    fn file_with(servers: &[(&str, bool)]) -> File {
        let mut file = File::new();
        for (name, enabled) in servers {
            file.upsert(
                *name,
                ServerConfig {
                    command: "true".to_owned(),
                    enabled: Some(*enabled),
                    ..ServerConfig::default()
                },
            );
        }
        file
    }

    #[tokio::test]
    async fn enumerate_returns_per_server_tools_and_caches() {
        let catalog = ToolCatalog::new();
        let file = file_with(&[("alpha", true), ("beta", true), ("off", false)]);
        let lister = MockLister {
            responses: BTreeMap::from([
                (
                    "alpha".to_owned(),
                    Ok(vec!["read_file".into(), "write_file".into()]),
                ),
                ("beta".to_owned(), Ok(vec!["search".into()])),
            ]),
            calls: Mutex::new(Vec::new()),
        };

        let first = catalog.enumerate_with(&file, &lister).await;
        assert_eq!(
            first.get("alpha").map(Vec::as_slice),
            Some(["read_file".to_string(), "write_file".to_string()].as_slice())
        );
        assert_eq!(
            first.get("beta").map(Vec::as_slice),
            Some(["search".to_string()].as_slice())
        );
        assert!(!first.contains_key("off"));
        assert_eq!(lister.calls.lock().expect("lock").len(), 2);
        assert_eq!(catalog.list_call_count(), 2);
        assert!(catalog.is_cached());

        // Second call must not re-hit tools/list.
        let second = catalog.enumerate_with(&file, &lister).await;
        assert_eq!(first, second);
        assert_eq!(lister.calls.lock().expect("lock").len(), 2);
        assert_eq!(catalog.list_call_count(), 2);
        assert_eq!(catalog.cached(), first);
    }

    #[tokio::test]
    async fn one_server_failure_is_isolated() {
        let catalog = ToolCatalog::new();
        let file = file_with(&[("ok", true), ("bad", true)]);
        let lister = MockLister {
            responses: BTreeMap::from([
                ("ok".to_owned(), Ok(vec!["a".into()])),
                ("bad".to_owned(), Err("boom".into())),
            ]),
            calls: Mutex::new(Vec::new()),
        };

        let map = catalog.enumerate_with(&file, &lister).await;
        assert_eq!(map["ok"], vec!["a".to_string()]);
        assert_eq!(map["bad"], Vec::<String>::new());
    }

    #[tokio::test]
    async fn invalidate_forces_reenumeration() {
        let catalog = ToolCatalog::new();
        let file = file_with(&[("s", true)]);
        let lister = MockLister {
            responses: BTreeMap::from([("s".to_owned(), Ok(vec!["t".into()]))]),
            calls: Mutex::new(Vec::new()),
        };

        let _ = catalog.enumerate_with(&file, &lister).await;
        catalog.invalidate();
        assert!(!catalog.is_cached());
        assert!(catalog.cached().is_empty());
        let _ = catalog.enumerate_with(&file, &lister).await;
        assert_eq!(lister.calls.lock().expect("lock").len(), 2);
        assert_eq!(catalog.list_call_count(), 2);
    }

    #[test]
    fn parse_tools_list_result_extracts_names() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [
                    { "name": "a", "inputSchema": {} },
                    { "name": "b" }
                ]
            }
        });
        assert_eq!(
            parse_tools_list_result(&msg).expect("parse"),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
