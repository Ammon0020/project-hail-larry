//! Shared API handler test-state construction.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{HeaderValue, Request};
use axum::response::Response;
use serde_json::Value;
use tower::ServiceExt;

use crate::acp::{AgentRegistry, Client, ClientDeps, ConversationStore};
use crate::config::{Config, ConfigStore};
use crate::events::{EventBus, SharedEventBus};
use crate::pairing::Manager as PairingManager;
use crate::permissions::Manager as PermissionsManager;
use crate::sync::Hub;
use crate::uploads;
use crate::workspace::Manager as WorkspaceManagerImpl;

use super::AppState;

/// The 7 core service deps every handler test needs, built from a temp dir.
struct CoreDeps {
    config: ConfigStore,
    pairing: PairingManager,
    workspaces: Arc<WorkspaceManagerImpl>,
    events: SharedEventBus,
    hub: Arc<Hub>,
    acp: Arc<Client>,
    permissions: Arc<PermissionsManager>,
}

/// Build the 7 core service deps from a temp state dir.
fn core_deps(dir: &Path) -> CoreDeps {
    let config = ConfigStore::new(Config {
        data_dir: dir.display().to_string(),
        db_path: dir.join("events.db").display().to_string(),
        ..Config::default()
    });
    let pairing = PairingManager::new(dir, None).expect("pairing");
    let workspaces = Arc::new(WorkspaceManagerImpl::new());
    let events = Arc::new(EventBus::open(dir.join("events.db")).expect("event bus"));
    let hub = Hub::with_event_bus(Arc::clone(&events));
    let permissions = PermissionsManager::new(None);
    let registry = Arc::new(AgentRegistry::default());
    let acp = Arc::new(Client::new(ClientDeps {
        registry,
        workspaces: workspaces.clone(),
        permissions: permissions.clone(),
        event_bus: events.clone(),
        conversation_store: ConversationStore::new(None),
        mcp_config_path: None,
        cancel_grace_period: std::time::Duration::from_millis(50),
        agent_idle_timeout: std::time::Duration::from_mins(2),
    }));
    CoreDeps {
        config,
        pairing,
        workspaces,
        events,
        hub,
        acp,
        permissions,
    }
}

/// `AppState` with all optional fields set to `None`.
pub(crate) fn test_state(dir: &Path) -> AppState {
    let d = core_deps(dir);
    AppState::new(
        d.config,
        d.pairing,
        d.workspaces,
        d.events,
        d.hub,
        d.acp,
        d.permissions,
        None,
        None,
        None,
    )
}

/// Temporary API state directory and a corresponding in-memory application
/// state. Tests that trigger config persistence must pin the state-dir
/// environment with [`StateDirEnvGuard`] first.
pub(crate) fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().expect("temporary state directory");
    let state = test_state(dir.path());
    (dir, state)
}

/// RAII guard that pins `LOCAL_AGENT_STATE_DIR` for a persistence test,
/// restoring the previous setting on drop. The global lock prevents parallel
/// tests from racing on the process-wide environment variable.
pub(crate) struct StateDirEnvGuard {
    prior: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl StateDirEnvGuard {
    pub(crate) fn pin(dir: &Path) -> Self {
        let lock = crate::config::lock_state_dir_env();
        let prior = std::env::var_os(crate::config::STATE_DIR_ENV_VAR);
        std::env::set_var(crate::config::STATE_DIR_ENV_VAR, dir);
        Self { prior, _lock: lock }
    }
}

impl Drop for StateDirEnvGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(value) => std::env::set_var(crate::config::STATE_DIR_ENV_VAR, value),
            None => std::env::remove_var(crate::config::STATE_DIR_ENV_VAR),
        }
    }
}

/// Pair a device and set the grace and remote-registration configuration used
/// by pending-action handler tests.
pub(crate) fn pending_actions_state(
    grace_seconds: i64,
    allow_remote: bool,
) -> (
    tempfile::TempDir,
    AppState,
    crate::interfaces::DeviceCredential,
) {
    let (dir, state) = state();
    {
        let mut config = state.config.write();
        config.revocation_grace_period_seconds = grace_seconds;
        config.allow_remote_workspace_registration = allow_remote;
    }
    let session = state
        .pairing
        .create_session("localhost", 7337)
        .expect("pairing session");
    let credential = state
        .pairing
        .verify_passcode(&session.passcode, "Device", None)
        .expect("pair device");
    (dir, state, credential)
}

/// Dispatch a loopback request through the complete API router.
pub(crate) async fn oneshot(state: AppState, request: Request<Body>) -> Response {
    oneshot_peer(state, request, "127.0.0.1:9").await
}

/// Dispatch a request with an explicit peer address so auth behavior can be
/// verified without a running TCP listener.
pub(crate) async fn oneshot_peer(
    state: AppState,
    mut request: Request<Body>,
    peer: &str,
) -> Response {
    let address: SocketAddr = peer.parse().expect("peer address");
    request.extensions_mut().insert(ConnectInfo(address));
    super::router(state)
        .oneshot(request)
        .await
        .expect("response")
}

/// Format a paired-device credential for the API Authorization header.
pub(crate) fn bearer(credential: &crate::interfaces::DeviceCredential) -> HeaderValue {
    HeaderValue::from_str(&format!("Bearer {}:{}", credential.id, credential.secret))
        .expect("authorization header")
}

/// Decode an API JSON response body for handler assertions.
pub(crate) async fn json_body(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

/// `AppState` with `mcp_config_path` set (for MCP handler tests).
pub(crate) fn test_state_with_mcp(dir: &Path, mcp_path: PathBuf) -> AppState {
    let d = core_deps(dir);
    AppState::new(
        d.config,
        d.pairing,
        d.workspaces,
        d.events,
        d.hub,
        d.acp,
        d.permissions,
        Some(mcp_path),
        None,
        None,
    )
}

/// `AppState` with an `uploads` manager (for session upload tests).
pub(crate) fn test_state_with_uploads(dir: &Path) -> AppState {
    let d = core_deps(dir);
    let uploads = Arc::new(Mutex::new(
        uploads::Manager::new(dir.join("uploads")).expect("uploads"),
    ));
    AppState::new(
        d.config,
        d.pairing,
        d.workspaces,
        d.events,
        d.hub,
        d.acp,
        d.permissions,
        None,
        Some(uploads),
        None,
    )
}
