//! Backend process management for the contract differential runner.
//!
//! The harness builds (or locates) a backend binary, creates an isolated state
//! directory with a seed config, starts the binary as a subprocess, waits for
//! the HTTP server to become ready (polls /health), and provides the base URL
//! + state dir path to the test modules. On shutdown it kills the subprocess.
//!
//! This mirrors the go-fixtures harness (`tests/contract/go-fixtures/daemon.go`)
//! but operates entirely black-box: the backend is a subprocess, not an
//! in-process daemon. This is what makes it backend-agnostic (Go or Rust).

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tempfile::TempDir;
use tokio::process::{Child, Command};

/// The seed agent JSON written into config.json so /api/agents returns a
/// populated entry. Mirrors `seedAgentJSON` in go-fixtures/seed.go.
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
    _state_dir: TempDir,
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
        let backend = std::env::var("CONTRACT_BACKEND").unwrap_or_else(|_| "go".to_string());

        eprintln!("[contract] backend = {backend}");
        eprintln!("[contract] repo root = {}", repo_root.display());

        // Locate or build the binary.
        let binary_path = if let Ok(p) = std::env::var("CONTRACT_BINARY") {
            PathBuf::from(p)
        } else {
            match backend.as_str() {
                "go" => build_go_binary(&repo_root).await,
                "rust" => build_rust_binary(&repo_root).await,
                other => panic!("unknown CONTRACT_BACKEND: {other} (expected 'go' or 'rust')"),
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
            _state_dir: state_dir,
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
        // Kill the subprocess. On Unix this sends SIGKILL (via tokio's kill);
        // for test cleanup this is acceptable — the backend doesn't need
        // graceful shutdown in tests.
        let _ = self.child.kill().await;

        // If CONTRACT_KEEP_STATE is set, persist the state dir by forgetting it.
        if std::env::var("CONTRACT_KEEP_STATE").is_ok() {
            eprintln!("[contract] keeping state dir: {}", self.state_dir.display());
            std::mem::forget(self._state_dir);
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

/// Find the repo root by walking up from CWD looking for Cargo.toml + go.mod.
fn find_repo_root() -> PathBuf {
    let cwd = std::env::current_dir().expect("get cwd");
    let mut dir = cwd.as_path();
    for _ in 0..10 {
        if dir.join("Cargo.toml").exists() && dir.join("go.mod").exists() {
            return dir.to_path_buf();
        }
        if let Some(parent) = dir.parent() {
            dir = parent;
        } else {
            break;
        }
    }
    panic!(
        "could not find repo root (Cargo.toml + go.mod) from {}",
        cwd.display()
    );
}

/// Build the Go binary (`go build -o /tmp/contract-local-agent ./cmd/app`).
async fn build_go_binary(repo_root: &Path) -> PathBuf {
    let bin_path = std::env::temp_dir().join("contract-local-agent");
    eprintln!(
        "[contract] building Go binary: go build -o {} ./cmd/app",
        bin_path.display()
    );

    let output = tokio::process::Command::new("go")
        .args(["build", "-o", bin_path.to_str().unwrap(), "./cmd/app"])
        .current_dir(repo_root)
        .output()
        .await
        .expect("run go build");

    if !output.status.success() {
        eprintln!("[contract] go build failed:");
        eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("go build failed");
    }
    bin_path
}

/// Build the Rust binary (`cargo build --bin local_agent`).
async fn build_rust_binary(repo_root: &Path) -> PathBuf {
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

    repo_root.join("target").join("debug").join("local_agent")
}

/// Find a free TCP port by binding to port 0 and reading the assigned port.
fn find_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to find free port");
    let port = listener.local_addr().expect("get local addr").port();
    drop(listener);
    port
}

/// Write seed config into the state dir. Mirrors `writeSeedConfig` in
/// go-fixtures/seed.go but with the dynamically-allocated port.
///
/// Writes both formats: `config.json` for the Go backend and `config.toml`
/// for the Rust backend (`Config::load` only reads TOML). Writing both is
/// harmless — each backend ignores the format it does not use.
fn write_seed_config(state_dir: &Path, repo_root: &Path, port: u16) -> Result<()> {
    let seed_ws_path = repo_root.join(SEED_WORKSPACE_REL);
    if !seed_ws_path.is_dir() {
        anyhow::bail!("seed workspace not found at {}", seed_ws_path.display());
    }

    let db_path = state_dir.join("local-agent.db");
    let agents: Vec<local_agent::config::AgentInfo> =
        serde_json::from_str(SEED_AGENT_JSON).context("parse seed agent JSON")?;

    // Go backend reads config.json (encoding/json camelCase tags).
    let json_config = serde_json::json!({
        "port": port,
        "host": "127.0.0.1",
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
        host: "127.0.0.1".to_string(),
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
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    // Neutralize autodetect so only the fixture-agent from the seed config
    // appears in /api/agents. Autodetect checks PATH first, then falls back
    // to absolute searchPaths that expand ~ to $HOME. Setting both PATH and
    // HOME to non-existent paths makes find_first_command return None for
    // every agent spec (Go and Rust). The binary path is absolute so it
    // doesn't need PATH.
    cmd.env("PATH", "/dev/null");
    cmd.env("HOME", "/dev/null");

    cmd.spawn().context("failed to spawn backend subprocess")
}
