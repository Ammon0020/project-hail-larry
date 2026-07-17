//! Safe discovery of the small, audited set of ACP harnesses.
//!
//! No user-configured command string is ever executed here: every executable
//! and every argument is declared in `KNOWN_AGENTS`, then passed directly to
//! `tokio::process::Command`.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::config::{AgentInfo, AgentModel};

const MAX_PROBE_OUTPUT: usize = 16 * 1024;
const MAX_DIAGNOSTIC: usize = 240;

#[derive(Clone, Copy)]
struct AgentSpec {
    id: &'static str,
    name: &'static str,
    commands: &'static [&'static str],
    args: &'static [&'static str],
    search_paths: &'static [&'static str],
    fallback_models: &'static [(&'static str, &'static str)],
    model_source: ModelSource,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModelSource {
    None,
    CodexCache,
    CursorCli,
    VibeConfig,
}

const KNOWN_AGENTS: &[AgentSpec] = &[
    AgentSpec {
        id: "claude-code",
        name: "Claude Code",
        commands: &["claude"],
        args: &[],
        search_paths: &[],
        fallback_models: &[
            ("claude-3-5-sonnet-20240620", "Claude 3.5 Sonnet"),
            ("claude-3-opus-20240229", "Claude 3 Opus"),
        ],
        model_source: ModelSource::None,
    },
    AgentSpec {
        id: "codex",
        name: "Codex CLI",
        // Never add the bare interactive `codex` TUI. Only its ACP adapter
        // speaks the stdio protocol and is safe to start with pipes.
        commands: &["codex-acp"],
        args: &[],
        search_paths: &[],
        fallback_models: &[("gpt-4o", "GPT-4o"), ("gpt-4-turbo", "GPT-4 Turbo")],
        model_source: ModelSource::CodexCache,
    },
    AgentSpec {
        id: "cursor",
        name: "Cursor Agent",
        commands: &["agent", "cursor-agent"],
        args: &["acp"],
        search_paths: &["%LOCALAPPDATA%\\cursor-agent", "~/.local/bin"],
        fallback_models: &[
            ("auto", "Auto"),
            ("composer-2.5-fast", "Composer 2.5 Fast (default)"),
            ("composer-2.5", "Composer 2.5"),
            ("gpt-5.2", "GPT-5.2"),
            ("claude-opus-4-8-high", "Opus 4.8 1M"),
            ("claude-4.6-sonnet-medium", "Sonnet 4.6 1M"),
            ("gemini-3.1-pro", "Gemini 3.1 Pro"),
            ("grok-4.3", "Grok 4.3 1M"),
        ],
        model_source: ModelSource::CursorCli,
    },
    AgentSpec {
        id: "devin",
        name: "Devin",
        commands: &["devin"],
        args: &["acp"],
        search_paths: &[
            "%LOCALAPPDATA%\\Programs\\Devin\\resources\\app\\extensions\\windsurf\\devin\\bin",
            "/Applications/Devin.app/Contents/Resources/app/extensions/windsurf/devin/bin",
            "~/.local/share/Devin/resources/app/extensions/windsurf/devin/bin",
            "%LOCALAPPDATA%\\Programs\\Windsurf\\resources\\app\\extensions\\windsurf\\devin\\bin",
            "/Applications/Windsurf.app/Contents/Resources/app/extensions/windsurf/devin/bin",
        ],
        fallback_models: &[
            ("claude-sonnet-4", "Claude Sonnet 4"),
            ("claude-opus-4.6", "Claude Opus 4.6"),
            ("opus", "Opus"),
            ("codex", "Codex"),
        ],
        model_source: ModelSource::None,
    },
    AgentSpec {
        id: "mistral-vibe",
        name: "Mistral Vibe",
        commands: &["vibe-acp", "vibe"],
        args: &[],
        search_paths: &[],
        fallback_models: &[
            ("mistral-large-latest", "Mistral Large"),
            ("mistral-small-latest", "Mistral Small"),
        ],
        model_source: ModelSource::VibeConfig,
    },
];

/// Whether the unstable ACP `providers/list` capability may be queried.
///
/// It is opt-in because providers/list is draft and several supported agents
/// do not implement it. The implementation is complete when enabled; callers
/// can deliberately use the deterministic file/fallback-only mode by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderProbe {
    /// Skip the unstable live query.
    #[default]
    Disabled,
    /// Send bounded ACP initialize and providers/list requests over stdio.
    Enabled,
}

/// Discovery policy. Defaults are safe for daemon startup.
#[derive(Debug, Clone, Copy)]
pub struct AutodetectOptions {
    pub provider_probe: ProviderProbe,
    pub probe_timeout: Duration,
}

impl Default for AutodetectOptions {
    fn default() -> Self {
        Self {
            provider_probe: ProviderProbe::Disabled,
            probe_timeout: Duration::from_secs(5),
        }
    }
}

/// Returns valid bare commands for a known agent, or `None` for custom agents.
#[must_use]
pub fn valid_commands_for_agent(id: &str) -> Option<Vec<&'static str>> {
    KNOWN_AGENTS
        .iter()
        .find(|spec| spec.id == id)
        .map(|spec| spec.commands.to_vec())
}

/// Detects installed known ACP agents in stable registry order.
pub async fn autodetect() -> Vec<AgentInfo> {
    autodetect_with(AutodetectOptions::default()).await
}

/// Detects installed known ACP agents using an explicit live-probe policy.
pub async fn autodetect_with(options: AutodetectOptions) -> Vec<AgentInfo> {
    let mut agents = Vec::new();
    for spec in KNOWN_AGENTS {
        let Some(command) = find_first_command(spec.commands, spec.search_paths) else {
            continue;
        };
        let (models, warning) = detect_models(spec, &command, options).await;
        agents.push(AgentInfo {
            id: spec.id.into(),
            name: spec.name.into(),
            command: command.to_string_lossy().into_owned(),
            args: spec.args.iter().map(|arg| (*arg).into()).collect(),
            models,
            warning,
        });
    }
    agents
}

async fn detect_models(
    spec: &AgentSpec,
    command: &Path,
    options: AutodetectOptions,
) -> (Vec<AgentModel>, String) {
    let provider_models = if options.provider_probe == ProviderProbe::Enabled {
        probe_providers(command, spec.args, options.probe_timeout)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if !provider_models.is_empty() {
        return (provider_models, String::new());
    }

    let file_models = match spec.model_source {
        ModelSource::CodexCache => codex_models_from_file(),
        ModelSource::VibeConfig => vibe_models_from_file(),
        ModelSource::CursorCli => cursor_models_from_cli(command, options.probe_timeout).await,
        ModelSource::None => Vec::new(),
    };
    if !file_models.is_empty() {
        return (file_models, String::new());
    }
    (
        spec.fallback_models
            .iter()
            .map(|(id, name)| AgentModel {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
        "Using fallback model list".into(),
    )
}

fn find_first_command(commands: &[&str], search_paths: &[&str]) -> Option<PathBuf> {
    let path_entries: Vec<PathBuf> = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect())
        .unwrap_or_default();
    for command in commands {
        for directory in &path_entries {
            if let Some(path) = executable_in(directory, command) {
                return Some(path);
            }
        }
    }
    for directory in search_paths.iter().filter_map(|path| expand_path(path)) {
        for command in commands {
            if let Some(path) = executable_in(&directory, command) {
                return Some(path);
            }
        }
    }
    None
}

fn executable_in(directory: &Path, command: &str) -> Option<PathBuf> {
    let candidates = if cfg!(windows) {
        vec![
            command.into(),
            format!("{command}.exe"),
            format!("{command}.cmd"),
        ]
    } else {
        vec![command.into()]
    };
    candidates
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| {
            path.is_file() && {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    path.metadata()
                        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
        })
}

fn expand_path(path: &str) -> Option<PathBuf> {
    let mut expanded = expand_windows_env(path);
    if expanded == "~" || expanded.starts_with("~/") || expanded.starts_with("~\\") {
        let home = dirs::home_dir()?;
        expanded = home.join(&expanded[1..]).to_string_lossy().into_owned();
    }
    Some(PathBuf::from(expanded))
}

fn expand_windows_env(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push('%');
            output.push_str(after_start);
            return output;
        };
        let key = &after_start[..end];
        if !key.is_empty()
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            output.push_str(&env::var(key).unwrap_or_default());
        } else {
            output.push('%');
            output.push_str(key);
            output.push('%');
        }
        rest = &after_start[end + 1..];
    }
    output.push_str(rest);
    output
}

fn codex_models_from_file() -> Vec<AgentModel> {
    #[derive(Deserialize)]
    struct Cache {
        #[serde(default)]
        models: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        slug: String,
        #[serde(default)]
        display_name: String,
    }
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(home.join(".codex/models_cache.json")) else {
        return Vec::new();
    };
    serde_json::from_str::<Cache>(&contents)
        .map(|cache| {
            cache
                .models
                .into_iter()
                .filter(|model| !model.slug.is_empty())
                .map(|model| AgentModel {
                    name: if model.display_name.is_empty() {
                        model.slug.clone()
                    } else {
                        model.display_name
                    },
                    id: model.slug,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn vibe_models_from_file() -> Vec<AgentModel> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(default)]
        models: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        name: String,
        #[serde(default)]
        alias: String,
    }
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(home.join(".vibe/config.toml")) else {
        return Vec::new();
    };
    toml::from_str::<Config>(&contents)
        .map(|config| {
            config
                .models
                .into_iter()
                .filter(|model| !model.name.is_empty())
                .map(|model| AgentModel {
                    id: if model.alias.is_empty() {
                        model.name.clone()
                    } else {
                        model.alias
                    },
                    name: model.name,
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn cursor_models_from_cli(command: &Path, duration: Duration) -> Vec<AgentModel> {
    let Ok((stdout, _)) = run_bounded(command, &["--list-models"], duration).await else {
        return Vec::new();
    };
    parse_cursor_models(&stdout)
}

fn parse_cursor_models(output: &[u8]) -> Vec<AgentModel> {
    let text = String::from_utf8_lossy(output);
    if text.to_ascii_lowercase().contains("no models available") {
        return Vec::new();
    }
    text.lines()
        .filter_map(|line| {
            let line = strip_ansi(line).trim().to_owned();
            let (id, name) = line.split_once(" - ")?;
            (!id.trim().is_empty()).then(|| AgentModel {
                id: id.trim().into(),
                name: name.trim().into(),
            })
        })
        .collect()
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' && matches!(chars.next(), Some('[')) {
            for character in chars.by_ref() {
                if character.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
}

async fn probe_providers(
    command: &Path,
    args: &[&str],
    duration: Duration,
) -> Result<Vec<AgentModel>, String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("start failed: {error}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err("open stdin pipe failed".into());
    };
    let Some(stdout) = child.stdout.take() else {
        return Err("open stdout pipe failed".into());
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err("open stderr pipe failed".into());
    };

    let stderr_task = tokio::spawn(async move { read_capped(&mut stderr, MAX_PROBE_OUTPUT).await });
    let result = timeout(duration, provider_exchange(&mut stdin, stdout)).await;
    terminate(&mut child).await;
    let stderr = stderr_task.await.unwrap_or_default();
    match result {
        Ok(Ok(models)) => Ok(models),
        Ok(Err(error)) => Err(format!("{}{}", error, diagnostic_suffix(&stderr))),
        Err(_) => Err("ACP probe timed out".into()),
    }
}

async fn provider_exchange(
    stdin: &mut tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
) -> Result<Vec<AgentModel>, String> {
    write_json(
        stdin,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": 1, "clientInfo": {"name": "local-agent-autodetect", "version": "1.0"}, "clientCapabilities": {}}
        }),
    )
    .await?;
    let mut stdout = BufReader::new(stdout);
    wait_for_response(&mut stdout, 1).await?;
    write_json(
        stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "providers/list", "params": {}}),
    )
    .await?;
    let response = wait_for_response(&mut stdout, 2).await?;
    Ok(response
        .get("result")
        .and_then(|result| result.get("providers"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|provider| provider.get("id").and_then(Value::as_str))
        .map(|id| AgentModel {
            id: id.into(),
            name: id.into(),
        })
        .collect())
}

async fn write_json(stdin: &mut tokio::process::ChildStdin, value: Value) -> Result<(), String> {
    let mut bytes =
        serde_json::to_vec(&value).map_err(|error| format!("encode request: {error}"))?;
    bytes.push(b'\n');
    stdin
        .write_all(&bytes)
        .await
        .map_err(|error| format!("write request: {error}"))?;
    stdin
        .flush()
        .await
        .map_err(|error| format!("flush request: {error}"))
}

async fn wait_for_response(
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    expected_id: i64,
) -> Result<Value, String> {
    loop {
        let line = read_line_capped(stdout, MAX_PROBE_OUTPUT).await?;
        let value: Value = serde_json::from_slice(&line)
            .map_err(|error| format!("invalid ACP response: {error}"))?;
        if value.get("id").and_then(Value::as_i64) != Some(expected_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            if error.get("code").and_then(Value::as_i64) == Some(-32601) {
                return Err("providers/list not supported".into());
            }
            return Err("ACP request failed".into());
        }
        return Ok(value);
    }
}

async fn read_line_capped<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let count = reader
            .read(&mut byte)
            .await
            .map_err(|error| format!("read response: {error}"))?;
        if count == 0 {
            return Err("ACP peer disconnected".into());
        }
        if byte[0] == b'\n' {
            return Ok(output);
        }
        if output.len() == limit {
            return Err("ACP response exceeded limit".into());
        }
        output.push(byte[0]);
    }
}

async fn read_capped<R: AsyncRead + Unpin>(reader: &mut R, limit: usize) -> Vec<u8> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let Ok(count) = reader.read(&mut chunk).await else {
            return output;
        };
        if count == 0 {
            return output;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&chunk[..count.min(remaining)]);
    }
}

async fn run_bounded(
    command: &Path,
    args: &[&str],
    duration: Duration,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut child = Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("start failed: {error}"))?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err("open stdout pipe failed".into());
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err("open stderr pipe failed".into());
    };
    let probe = async {
        let (stdout, stderr, status) = tokio::join!(
            read_capped(&mut stdout, MAX_PROBE_OUTPUT),
            read_capped(&mut stderr, MAX_PROBE_OUTPUT),
            child.wait()
        );
        (stdout, stderr, status)
    };
    let result = timeout(duration, probe).await;
    if result.is_err() {
        terminate(&mut child).await;
        return Err("model probe timed out".into());
    }
    let (stdout, stderr, status) = result.map_err(|_| "model probe timed out")?;
    if status
        .map_err(|error| format!("wait failed: {error}"))?
        .success()
    {
        Ok((stdout, stderr))
    } else {
        Err(format!("model probe failed{}", diagnostic_suffix(&stderr)))
    }
}

async fn terminate(child: &mut Child) {
    if child.id().is_some() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
}

fn diagnostic_suffix(stderr: &[u8]) -> String {
    let diagnostic = String::from_utf8_lossy(stderr);
    let redacted = diagnostic
        .lines()
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            !line.contains("token") && !line.contains("password") && !line.contains("api_key")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let diagnostic = redacted.trim();
    if diagnostic.is_empty() {
        String::new()
    } else {
        format!(" (agent stderr: {})", truncate(diagnostic, MAX_DIAGNOSTIC))
    }
}

fn truncate(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let output: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{output}…")
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::{
        expand_windows_env, parse_cursor_models, strip_ansi, valid_commands_for_agent,
        ProviderProbe,
    };

    #[test]
    fn codex_never_allows_the_bare_tui() {
        let commands = valid_commands_for_agent("codex").expect("known codex agent");
        assert_eq!(commands, ["codex-acp"]);
        assert!(!commands.contains(&"codex"));
        assert!(valid_commands_for_agent("custom").is_none());
    }

    #[test]
    fn windows_environment_expansion_is_deterministic() {
        // An unset variable is intentionally replaced with empty text.
        assert_eq!(
            expand_windows_env("before-%ACP_UNKNOWN_TEST%-after"),
            "before--after"
        );
        assert_eq!(expand_windows_env("100% literal"), "100% literal");
    }

    #[test]
    fn cursor_model_parser_strips_ansi_and_ignores_noise() {
        let models = parse_cursor_models(
            b"\x1b[2Kauto - Auto\nLoading models\ncomposer - Composer\nNo separator\n",
        );
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "auto");
        assert_eq!(models[1].name, "Composer");
        assert_eq!(strip_ansi("\x1b[2Kauto"), "auto");
    }

    #[test]
    fn provider_probe_is_explicitly_opt_in() {
        assert_eq!(ProviderProbe::default(), ProviderProbe::Disabled);
    }
}
