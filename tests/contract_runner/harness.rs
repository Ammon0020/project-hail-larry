//! Backend process management for the contract differential runner.
//!
//! The harness builds (or locates) a backend binary, creates an isolated state
//! directory with a seed config, starts the binary as a subprocess, waits for
//! the HTTP server to become ready (polls /health), and provides the base URL
//! + state dir path to the test modules. On shutdown it kills the subprocess.
//!
//! This supersedes the original Go in-process fixture harness (removed at the
//! Rust cutover) and operates entirely black-box: the backend is a subprocess,
//! not an in-process daemon. Only the Rust backend is supported now.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tempfile::TempDir;
use tokio::process::{Child, Command};

/// The seed agent JSON written into config.json so /api/agents returns a
/// populated entry.
const SEED_AGENT_JSON: &str = r#"[
  {
    "id": "fixture-agent",
    "name": "Fixture Agent",
    "command": "fixture-agent-binary-not-on-path",
    "args": [],
    "models": [
      {"id": "fixture-model", "name": "Fixture Model"}
    ]
  }
]"#;

/// The seed workspace fixture path relative to the repo root.
const SEED_WORKSPACE_REL: &str = "tests/contract/fixtures/seed-workspace";

/// BackendHarness manages the backend subprocess and its isolated state.
pub struct BackendHarness {
    /// The isolated state directory. Kept alive so it is not deleted until
    /// shutdown (TempDir drops on shutdown unless CONTRACT_KEEP_STATE is set).
    /// `Option` so `shutdown` can `take()` it (for `CONTRACT_KEEP_STATE`)
    /// without conflicting with the `Drop` impl that signals the child to die.
    _state_dir: Option<TempDir>,
    /// Path to the state directory (borrowed from _state_dir for convenience).
    pub state_dir: PathBuf,
    /// The repo root (where Cargo.toml / go.mod live).
    pub repo_root: PathBuf,
    /// The base URL of the running backend (e.g. "http://127.0.0.1:12345").
    pub base_url: String,
    /// The TCP port the backend is listening on.
    pub port: u16,
    /// The backend subprocess. Killed on shutdown.
    child: Child,
    /// The backend type ("go" or "rust") for logging.
    pub backend: String,
    /// The binary path used to start the daemon and to run CLI commands.
    pub binary_path: PathBuf,
}

impl BackendHarness {
    /// Build (or locate) the backend binary, create an isolated state dir,
    /// write the seed config, start the backend, and wait for /health.
    pub async fn start() -> Self {
        let repo_root = find_repo_root();
        let backend = std::env::var("CONTRACT_BACKEND").unwrap_or_else(|_| "rust".to_string());

        eprintln!("[contract] backend = {backend}");
        eprintln!("[contract] repo root = {}", repo_root.display());

        // Locate or build the binary.
        let binary_path = if let Ok(p) = std::env::var("CONTRACT_BINARY") {
            PathBuf::from(p)
        } else {
            match backend.as_str() {
                "rust" => build_rust_binary(&repo_root).await,
                "go" => panic!(
                    "CONTRACT_BACKEND=go is no longer supported — the Go daemon \
                     (cmd/app, internal/) was removed at Rust cutover. Use \
                     CONTRACT_BACKEND=rust (default)."
                ),
                other => panic!("unknown CONTRACT_BACKEND: {other} (expected 'rust')"),
            }
        };

        eprintln!("[contract] binary = {}", binary_path.display());

        // Create an isolated state directory.
        let state_dir = tempfile::tempdir().expect("create state dir");
        let state_dir_path = state_dir.path().to_path_buf();

        // Find a free TCP port for the backend to listen on.
        let port = find_free_port();
        eprintln!("[contract] port = {port}");

        // Write the seed config.json into the state dir.
        write_seed_config(&state_dir_path, &repo_root, port).expect("write seed config");

        // Remove any stale PID file so IsRunning doesn't think a daemon is
        // already running.
        let pid_file = state_dir_path.join("daemon.pid");
        let _ = std::fs::remove_file(&pid_file);

        // Start the backend subprocess.
        let child = start_backend(&binary_path, &state_dir_path, &backend)
            .await
            .expect("start backend");

        let base_url = format!("http://127.0.0.1:{port}");

        let harness = Self {
            _state_dir: Some(state_dir),
            state_dir: state_dir_path,
            repo_root,
            base_url,
            port,
            child,
            backend,
            binary_path,
        };

        // Wait for the backend to become ready (poll /health).
        harness.wait_for_ready().await;

        eprintln!("[contract] backend ready at {}", harness.base_url);
        harness
    }

    /// Poll /health until it returns 200 or the timeout expires.
    async fn wait_for_ready(&self) {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("build reqwest client");

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                eprintln!("[contract] ERROR: backend did not become ready within 30s");
                eprintln!(
                    "[contract] backend = {} binary = {}",
                    self.backend,
                    self.binary_path.display()
                );
                eprintln!("[contract] state dir = {}", self.state_dir.display());
                eprintln!("[contract] base url  = {}", self.base_url);
                panic!("backend did not become ready within 30s");
            }

            let url = format!("{}/health", self.base_url);
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => return,
                _ => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }

    /// Shut down the backend subprocess and clean up the state dir.
    pub async fn shutdown(mut self) {
        // Kill + reap the subprocess. On Unix `kill` sends SIGKILL; on Windows
        // `TerminateProcess`. Waiting afterward ensures the OS process is
        // actually gone before the test binary moves on, so no daemons linger
        // on Windows (where `TerminateProcess` is async w.r.t. handle close).
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;

        // If CONTRACT_KEEP_STATE is set, persist the state dir by forgetting it.
        if std::env::var("CONTRACT_KEEP_STATE").is_ok() {
            if let Some(state_dir) = self._state_dir.take() {
                eprintln!("[contract] keeping state dir: {}", self.state_dir.display());
                std::mem::forget(state_dir);
            }
        }
    }

    /// The workspace ID for the seed workspace. The runner queries
    /// /api/workspaces to discover the real ID at runtime (it is a hash of the
    /// absolute path and therefore machine-specific).
    pub async fn workspace_id(&self) -> String {
        let url = format!("{}/api/workspaces", self.base_url);
        let resp = reqwest::get(&url).await.expect("fetch workspaces");
        let body: serde_json::Value = resp.json().await.expect("parse workspaces JSON");
        body.as_array()
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("id"))
            .and_then(|id| id.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| panic!("no workspace ID found in /api/workspaces response"))
    }
}

/// Safety net so a forgotten `shutdown()` (e.g. when a test panics before
/// reaching it) does not orphan the backend daemon. `start_kill` is the
/// synchronous, non-async variant of `kill` — it just signals the process to
/// exit without waiting for it. The explicit `shutdown()` path still does the
/// full kill + wait + state-dir handling; this only guarantees the daemon is
/// told to die.
impl Drop for BackendHarness {
    fn drop(&mut self) {
        // `start_kill` is sync: signals the process to terminate without
        // waiting. Best-effort — ignore errors (process may already be dead).
        let _ = self.child.start_kill();
    }
}

/// First non-loopback IPv4 address on this host, for dialing the daemon so the
/// server sees a non-loopback peer (WS auth gate). Returns `None` when the host
/// has no usable LAN/global IPv4 (rare; skip auth-rejection in that case).
pub fn first_non_loopback_ipv4() -> Option<String> {
    use std::net::{IpAddr, Ipv4Addr, UdpSocket};

    // UDP connect does not send packets; it selects a route and reveals the
    // local address the kernel would use for egress — typically a LAN IP.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    match ip {
        IpAddr::V4(v4) if !v4.is_loopback() && v4 != Ipv4Addr::UNSPECIFIED => Some(v4.to_string()),
        _ => None,
    }
}

/// Find the repo root by walking up from CWD looking for Cargo.toml.
fn find_repo_root() -> PathBuf {
    let cwd = std::env::current_dir().expect("get cwd");
    let mut dir = cwd.as_path();
    for _ in 0..10 {
        if dir.join("Cargo.toml").exists() {
            return dir.to_path_buf();
        }
        if let Some(parent) = dir.parent() {
            dir = parent;
        } else {
            break;
        }
    }
    panic!(
        "could not find repo root (Cargo.toml) from {}",
        cwd.display()
    );
}

/// Build the Rust binary (`cargo build --bin local_agent`).
///
/// Pre-checks `web/dist/index.html` before invoking cargo: `build.rs` requires
/// it (rust-embed) and exits with a generic "missing" error that is confusing
/// when triggered by a parallel `cargo build` race. Failing here with a clear
/// message points the user at `make build-frontend` (or `make test-contract`,
/// which depends on it).
async fn build_rust_binary(repo_root: &Path) -> PathBuf {
    let dist_index = repo_root.join("web").join("dist").join("index.html");
    if !dist_index.is_file() {
        panic!(
            "web/dist/index.html is missing — the Rust binary embeds the \
             frontend via rust-embed. Build it first:\n  make build-frontend \
             (or `cd web && npm run build`),\nor run the contract suite via \
             `make test-contract` which depends on build-frontend."
        );
    }

    eprintln!("[contract] building Rust binary: cargo build --bin local_agent");

    let output = tokio::process::Command::new("cargo")
        .args(["build", "--bin", "local_agent"])
        .current_dir(repo_root)
        .output()
        .await
        .expect("run cargo build");

    if !output.status.success() {
        eprintln!("[contract] cargo build failed:");
        eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("cargo build failed");
    }

    let exe = if cfg!(windows) {
        "local_agent.exe"
    } else {
        "local_agent"
    };
    repo_root.join("target").join("debug").join(exe)
}

/// Find a free TCP port by binding to port 0 and reading the assigned port.
fn find_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to find free port");
    let port = listener.local_addr().expect("get local addr").port();
    drop(listener);
    port
}

/// Write seed config into the state dir with the dynamically-allocated port.
///
/// Writes `config.toml` (read by the Rust backend's `Config::load`) plus a
/// leftover `config.json` from the removed Go backend. Writing both is
/// harmless — the Rust backend ignores `config.json`.
fn write_seed_config(state_dir: &Path, repo_root: &Path, port: u16) -> Result<()> {
    let seed_ws_path = repo_root.join(SEED_WORKSPACE_REL);
    if !seed_ws_path.is_dir() {
        anyhow::bail!("seed workspace not found at {}", seed_ws_path.display());
    }

    let db_path = state_dir.join("local-agent.db");
    let agents: Vec<local_agent::config::AgentInfo> =
        serde_json::from_str(SEED_AGENT_JSON).context("parse seed agent JSON")?;

    // Bind 0.0.0.0 so WS auth-rejection can dial a non-loopback local address
    // (peer RemoteAddr is then non-loopback). REST/WS success cases still use
    // 127.0.0.1 and keep the loopback auth bypass.
    let json_config = serde_json::json!({
        "port": port,
        "host": "0.0.0.0",
        "dataDir": state_dir,
        "dbPath": &db_path,
        "workspaces": [&seed_ws_path],
        "agents": &agents,
        "tlsEnabled": false,
        "pairingTtlSeconds": 300,
        "revocationGracePeriodSeconds": 300,
        "credentialInactivityTtlSeconds": 2592000,
    });
    std::fs::write(
        state_dir.join("config.json"),
        serde_json::to_string_pretty(&json_config)?,
    )?;

    // Rust backend reads config.toml (same camelCase field names via serde).
    let toml_config = local_agent::config::Config {
        port: i64::from(port),
        host: "0.0.0.0".to_string(),
        data_dir: state_dir.to_string_lossy().to_string(),
        db_path: db_path.to_string_lossy().to_string(),
        workspaces: vec![seed_ws_path.to_string_lossy().to_string()],
        agents,
        tls_enabled: false,
        tls_cert_dir: String::new(),
        https_port: 0,
        pairing_ttl_seconds: 300,
        credential_inactivity_ttl_seconds: 2_592_000,
        allow_remote_workspace_registration: false,
        revocation_grace_period_seconds: 300,
        prompt_context: local_agent::config::PromptContextSettings::default(),
        workspace_trust: std::collections::HashMap::new(),
        cancel_grace_period_seconds: 0,
        permission_timeout_seconds: 0,
        agent_idle_timeout_seconds: 0,
        extra: toml::Table::new(),
    };
    std::fs::write(
        state_dir.join("config.toml"),
        toml::to_string_pretty(&toml_config).context("serialize seed config.toml")?,
    )?;
    Ok(())
}

/// Start the backend as a subprocess with LOCAL_AGENT_STATE_DIR pointing at
/// the isolated state dir. The subprocess inherits stderr so startup errors
/// are visible in the test output.
async fn start_backend(binary: &Path, state_dir: &Path, backend: &str) -> Result<Child> {
    eprintln!(
        "[contract] starting {backend} backend: {} start (state dir: {})",
        binary.display(),
        state_dir.display()
    );

    let mut cmd = Command::new(binary);
    cmd.arg("start")
        .env("LOCAL_AGENT_STATE_DIR", state_dir)
        // Disable ACP agent autodetect so /api/agents reflects only the
        // fixture-agent from the seed config. On Windows the Known Folder API
        // and %LOCALAPPDATA% bypass PATH=/dev/null + USERPROFILE, so this env
        // gate is the reliable neutralization.
        .env("LOCAL_AGENT_NO_AUTODETECT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    // Neutralize autodetect so only the fixture-agent from the seed config
    // appears in /api/agents. Autodetect checks PATH first, then falls back
    // to absolute searchPaths that expand ~ via dirs::home_dir (passwd
    // fallback — NOT just $HOME). A fake home under the isolated state dir
    // keeps dirs::home_dir from resolving the real user home if any code path
    // ever drops LOCAL_AGENT_STATE_DIR. PATH=/dev/null still blocks PATH hits.
    let fake_home = state_dir.join("fake-home");
    std::fs::create_dir_all(&fake_home)
        .with_context(|| format!("create fake home {}", fake_home.display()))?;
    cmd.env("PATH", "/dev/null");
    cmd.env("HOME", &fake_home);
    // Windows Known Folder / USERPROFILE fallback for dirs::home_dir.
    cmd.env("USERPROFILE", &fake_home);

    cmd.spawn().context("failed to spawn backend subprocess")
}
