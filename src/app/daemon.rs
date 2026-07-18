//! Daemon composition root and lifecycle ownership.
//!
//! All concrete services are constructed here in dependency order. Handler
//! modules receive the already-built [`crate::api::AppState`] and therefore do
//! not own process state, listeners, or storage lifetimes.

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::acp::{AgentRegistry, Client, ClientDeps, ConversationStore};
use crate::api::{self, AppState};
use crate::config::{Config, ConfigStore};
use crate::events::{EventBus, SharedEventBus};
use crate::files::FileSync;
use crate::fswatch;
use crate::interfaces::WorkspaceManager;
use crate::pairing::{Manager as PairingManager, PairingError, WorkspaceRegistrar};
use crate::permissions::{EventBusPermissionSink, Manager as PermissionsManager, PermissionSink};
use crate::sync::Hub;
use crate::uploads::Manager as UploadsManager;
use crate::workspace::Manager as WorkspaceManagerImpl;

use super::{listen, process};

const PID_FILE_NAME: &str = "daemon.pid";

/// The addresses bound by a running daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundAddresses {
    /// Cleartext HTTP listener, always present.
    pub http: SocketAddr,
    /// Self-signed HTTPS listener, present only with TLS enabled.
    pub https: Option<SocketAddr>,
}

/// Process status reported by the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonStatus {
    /// Whether the PID file names a live process.
    pub running: bool,
    /// PID when `running` is true.
    pub pid: Option<u32>,
    /// Configured HTTP address.
    pub http: String,
    /// Configured HTTPS address when TLS is enabled.
    pub https: Option<String>,
}

/// Fully composed host daemon. It owns service lifetime and cancellation.
pub struct Daemon {
    config: Config,
    state: AppState,
    cancel: CancellationToken,
    // Held explicitly so shutdown ordering is visible and survives API state
    // shape changes while upload and MCP routes are implemented in parallel.
    events: SharedEventBus,
    pairing: PairingManager,
    hub: Arc<Hub>,
    uploads: Arc<Mutex<UploadsManager>>,
    mcp_config_path: PathBuf,
    _files: Arc<FileSync>,
    /// External on-disk change watcher. `None` when notify init fails (non-fatal).
    fs_watcher: Option<Arc<fswatch::Watcher>>,
    permission_sweeper: tokio::task::JoinHandle<()>,
}

impl Daemon {
    /// Load configuration and construct every manager in dependency order.
    pub async fn load() -> Result<Self> {
        let config = Config::load().context("load local-agent configuration")?;
        Self::new(config).await
    }

    /// Construct a daemon from a validated configuration.
    pub async fn new(config: Config) -> Result<Self> {
        fs::create_dir_all(&config.data_dir)
            .with_context(|| format!("create state directory {}", config.data_dir))?;

        // 1. Config / durable events. Autodetect may rewrite agents before the
        // store is published so REST and ACP share one registry snapshot.
        let mut config = config;
        refresh_agents_on_startup(&mut config).await;
        let config_store = ConfigStore::new(config.clone());
        let events = Arc::new(EventBus::open(&config.db_path).context("open SQLite event store")?);

        // 2. Filesystem watcher before workspace load so startup + grace
        // registrations can attach watches. Init failure is non-fatal (Go parity).
        let fs_watcher = match new_filesystem_watcher(Arc::clone(&events)) {
            Ok(watcher) => Some(Arc::new(watcher)),
            Err(error) => {
                warn!(%error, "filesystem watcher unavailable");
                None
            }
        };

        // 3. Workspace manager first so grace-delayed registrations can call it.
        let workspaces = Arc::new(WorkspaceManagerImpl::new());
        // Suppress fswatch echoes of our own writes (Go SetOnWrite → NoteAppWrite).
        if let Some(watcher) = fs_watcher.clone() {
            workspaces.set_on_write(Arc::new(move |abs_path: &str| {
                watcher.note_app_write(abs_path);
            }));
        }
        let registrar: Arc<dyn WorkspaceRegistrar> = Arc::new(DaemonWorkspaceRegistrar {
            workspaces: Arc::clone(&workspaces),
            fs_watcher: fs_watcher.clone(),
        });
        let pairing =
            PairingManager::new(&config.data_dir, Some(registrar)).context("open pairing state")?;
        configure_pairing(&pairing, &config);
        load_workspaces(&workspaces, &config.workspaces).await;
        watch_available_workspaces(&workspaces, fs_watcher.as_deref()).await;
        let files = Arc::new(FileSync::new());

        // 4. Permissions / ACP / sync. Permission notifications persist to the
        // event bus before they reach the hub's reconnect stream.
        let permission_sink: Arc<dyn PermissionSink> =
            Arc::new(EventBusPermissionSink::new(Arc::clone(&events)));
        let permissions = PermissionsManager::new(Some(permission_sink));
        let permission_sweeper = permissions.start_sweeper();
        let registry = Arc::new(AgentRegistry::from_agents(config.agents.clone()));
        let mcp_config_path = Path::new(&config.data_dir).join("mcp.json");
        let acp = Arc::new(Client::new(ClientDeps {
            registry,
            workspaces: workspaces.clone(),
            permissions: permissions.clone(),
            event_bus: events.clone(),
            conversation_store: ConversationStore::new(Some(
                Path::new(&config.data_dir).join("conversations.json"),
            )),
            mcp_config_path: Some(mcp_config_path.clone()),
        }));
        // Metadata only — actors start lazily on prompt/cancel/providers/rebind.
        acp.load_conversations()
            .context("load persisted ACP conversations")?;
        let hub = Hub::with_event_bus(Arc::clone(&events));

        // 5. Supporting stores consumed by REST upload/MCP routes.
        let uploads = Arc::new(Mutex::new(
            UploadsManager::new(Path::new(&config.data_dir).join("uploads"))
                .context("open upload store")?,
        ));

        // 6. Router state only receives fully constructed dependencies.
        let state = AppState::new(
            config_store,
            pairing.clone(),
            workspaces,
            events.clone(),
            hub.clone(),
            acp,
            permissions,
            Some(mcp_config_path.clone()),
            Some(uploads.clone()),
            fs_watcher.clone(),
        );
        let cancel = CancellationToken::new();

        Ok(Self {
            config,
            state,
            cancel,
            events,
            pairing,
            hub,
            uploads,
            mcp_config_path,
            _files: files,
            fs_watcher,
            permission_sweeper,
        })
    }

    /// Serve until `cancellation` is triggered, draining HTTP before storage.
    pub async fn run(self, cancellation: CancellationToken) -> Result<()> {
        let listeners = listen::bind(&self.config).await?;
        let addresses = listeners.addresses();
        write_pid(&self.config.data_dir)?;
        info!(
            http = %addresses.http,
            https = ?addresses.https,
            "local-agent daemon started"
        );

        let router = api::router(self.state.clone());
        let root_cancel = CancellationToken::new();
        let cancellation_forwarder = {
            let root_cancel = root_cancel.clone();
            let external = cancellation.clone();
            let local = self.cancel.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = external.cancelled() => {}
                    _ = local.cancelled() => {}
                }
                root_cancel.cancel();
            })
        };
        let serve_result = listen::serve(listeners, router, root_cancel.clone()).await;
        root_cancel.cancel();
        cancellation_forwarder.abort();
        self.shutdown().await;
        remove_pid(&self.config.data_dir);
        serve_result
    }

    /// Trigger graceful shutdown of a running in-process daemon.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Execute shutdown in dependency-safe order.
    async fn shutdown(&self) {
        // Listener drain is complete when `listen::serve` returns. Stop event
        // producers before dropping the SQLite-backed EventBus.
        if let Some(watcher) = &self.fs_watcher {
            if let Err(error) = watcher.close() {
                warn!(%error, "close filesystem watcher during shutdown");
            }
        }
        self.pairing.close();
        self.hub.shutdown();
        self.permission_sweeper.abort();
        if let Ok(manager) = self.uploads.lock() {
            if let Err(error) = manager.remove_all() {
                warn!(%error, "remove temporary upload data during shutdown");
            }
        } else {
            warn!("uploads manager lock poisoned during shutdown");
        }
        info!(
            mcp_config = %self.mcp_config_path.display(),
            "daemon services stopped; dropping event store"
        );
        // `EventBus` owns the rusqlite connection. The last references drop as
        // this daemon returns, closing SQLite only after HTTP/WS work stops.
        let _ = &self.events;
    }
}

/// Return PID-backed daemon status without contacting the HTTP listener.
pub fn status(config: &Config) -> Result<DaemonStatus> {
    let pid = read_live_pid(&config.data_dir)?;
    let http = configured_address(config.host.as_str(), config.port)?;
    let https = if config.tls_enabled {
        Some(configured_address(
            config.host.as_str(),
            resolved_https_port(config)?,
        )?)
    } else {
        None
    };
    Ok(DaemonStatus {
        running: pid.is_some(),
        pid,
        http,
        https,
    })
}

/// Signal the PID recorded in the configured data directory.
pub fn stop(config: &Config) -> Result<()> {
    let Some(pid) = read_live_pid(&config.data_dir)? else {
        return Err(anyhow!("daemon is not running"));
    };
    process::stop(pid)?;
    remove_pid(&config.data_dir);
    Ok(())
}

/// Resolve the configured HTTPS port, defaulting to HTTP + 1.
pub fn resolved_https_port(config: &Config) -> Result<i64> {
    let port = if config.https_port == 0 {
        config
            .port
            .checked_add(1)
            .ok_or_else(|| anyhow!("HTTPS port overflow"))?
    } else {
        config.https_port
    };
    validate_port(port)?;
    Ok(port)
}

fn configure_pairing(pairing: &PairingManager, config: &Config) {
    if config.pairing_ttl_seconds > 0 {
        pairing.set_session_ttl(Duration::from_secs(config.pairing_ttl_seconds as u64));
    }
    if config.credential_inactivity_ttl_seconds > 0 {
        pairing.set_inactivity_ttl(Duration::from_secs(
            config.credential_inactivity_ttl_seconds as u64,
        ));
    }
}

/// Bridges pairing's sync grace callback onto the async workspace manager.
///
/// Matches Go's `SetWorkspaceRegisterFn` wiring: the grace timer thread only
/// needs fire-and-forget registration; failures are logged, not rethrown into
/// the timer task.
struct DaemonWorkspaceRegistrar {
    workspaces: Arc<WorkspaceManagerImpl>,
    fs_watcher: Option<Arc<fswatch::Watcher>>,
}

impl WorkspaceRegistrar for DaemonWorkspaceRegistrar {
    fn register_workspace(&self, path: &str) -> Result<(), PairingError> {
        let workspaces = Arc::clone(&self.workspaces);
        let fs_watcher = self.fs_watcher.clone();
        let path = path.to_string();
        tokio::spawn(async move {
            match workspaces.register(&path).await {
                Ok(info) => {
                    if let Some(watcher) = fs_watcher.as_ref() {
                        watcher.add_workspace(&info.id, &info.path);
                    }
                }
                Err(error) => {
                    tracing::error!(
                        workspace = %path,
                        error = %error,
                        "grace-delayed workspace registration failed"
                    );
                }
            }
        });
        Ok(())
    }
}

/// Create a watcher whose emit path persists + publishes on the event bus.
///
/// The notify callback runs off the Tokio runtime; spawn onto the current
/// handle so `append_and_publish` stays async without blocking the emit thread.
fn new_filesystem_watcher(
    events: SharedEventBus,
) -> Result<fswatch::Watcher, crate::interfaces::AppError> {
    let handle = tokio::runtime::Handle::current();
    fswatch::Watcher::new(move |event| {
        let events = Arc::clone(&events);
        handle.spawn(async move {
            if let Err(error) = events.append_and_publish(event).await {
                warn!(%error, "failed to publish filesystem change event");
            }
        });
    })
}

async fn load_workspaces(workspaces: &Arc<WorkspaceManagerImpl>, configured: &[String]) {
    for path in configured {
        if let Err(error) = workspaces.register(path).await {
            // Keep the path in config (caller never prunes) and surface it in
            // list() so CLI/UI can warn — do not silently drop registrations.
            warn!(workspace = %path, %error, "configured workspace unavailable");
            if let Err(retain_error) = workspaces.retain_unavailable(path, error.to_string()) {
                warn!(
                    workspace = %path,
                    %retain_error,
                    "failed to retain unavailable workspace in registry"
                );
            }
        }
    }
}

/// Prune stale known-agent commands, merge PATH autodetection, persist if needed.
///
/// Mirrors Go `autodetectAndRegisterAgents` so an empty `agents = []` config
/// still populates the harness selector on first boot.
async fn refresh_agents_on_startup(config: &mut Config) {
    let (pruned, pruned_changed) =
        crate::acp::prune_stale_known_agents(std::mem::take(&mut config.agents));
    config.agents = pruned;

    let detected = crate::acp::autodetect().await;
    let (merged, merge_changed) = crate::acp::merge_autodetected_agents(&config.agents, detected);
    config.agents = merged;

    if !(pruned_changed || merge_changed) {
        return;
    }
    if let Err(error) = config.save() {
        warn!(%error, "failed to persist autodetected agents");
    } else {
        info!(
            count = config.agents.len(),
            "persisted autodetected ACP agents"
        );
    }
}

async fn watch_available_workspaces(
    workspaces: &Arc<WorkspaceManagerImpl>,
    watcher: Option<&fswatch::Watcher>,
) {
    let Some(watcher) = watcher else {
        return;
    };
    match workspaces.list().await {
        Ok(list) => {
            for ws in list {
                if ws.available {
                    watcher.add_workspace(&ws.id, &ws.path);
                }
            }
        }
        Err(error) => warn!(%error, "list workspaces for fswatch attach failed"),
    }
}

fn configured_address(host: &str, port: i64) -> Result<String> {
    validate_port(port)?;
    let host = if host.is_empty() { "0.0.0.0" } else { host };
    Ok(format!("{host}:{port}"))
}

fn validate_port(port: i64) -> Result<()> {
    if !(1..=i64::from(u16::MAX)).contains(&port) {
        return bail_invalid_port(port);
    }
    Ok(())
}

fn bail_invalid_port(port: i64) -> Result<()> {
    Err(anyhow!("invalid TCP port {port}; expected 1..=65535"))
}

fn pid_path(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join(PID_FILE_NAME)
}

fn write_pid(data_dir: &str) -> Result<()> {
    let pid = std::process::id().to_string();
    crate::fsutil::atomic_write(&pid_path(data_dir), pid.as_bytes(), Some(0o600))
        .context("write daemon PID file")
}

fn read_live_pid(data_dir: &str) -> Result<Option<u32>> {
    let path = pid_path(data_dir);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let pid = raw
        .trim()
        .parse::<u32>()
        .with_context(|| format!("parse daemon PID file {}", path.display()))?;
    if process::is_running(pid) {
        Ok(Some(pid))
    } else {
        remove_pid(data_dir);
        Ok(None)
    }
}

fn remove_pid(data_dir: &str) {
    if let Err(error) = fs::remove_file(pid_path(data_dir)) {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(%error, "remove daemon PID file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_port_defaults_to_http_plus_one() {
        let config = Config {
            port: 7337,
            https_port: 0,
            ..Config::default()
        };
        assert_eq!(resolved_https_port(&config).expect("resolve port"), 7338);
    }

    #[tokio::test]
    async fn daemon_composes_and_cancels_cleanly() {
        let state = tempfile::tempdir().expect("temporary state");
        let config = Config {
            data_dir: state.path().display().to_string(),
            db_path: state.path().join("events.db").display().to_string(),
            tls_cert_dir: state.path().join("tls").display().to_string(),
            port: 0,
            tls_enabled: false,
            ..Config::default()
        };
        // Construction opens every durable manager without binding sockets.
        let daemon = Daemon::new(config).await.expect("compose daemon");
        daemon.cancel();
        assert!(daemon.cancel.is_cancelled());
    }

    #[tokio::test]
    async fn daemon_start_stop_smoke_drains_http_listener() {
        let state = tempfile::tempdir().expect("temporary state");
        let config = Config {
            data_dir: state.path().display().to_string(),
            db_path: state.path().join("events.db").display().to_string(),
            tls_cert_dir: state.path().join("tls").display().to_string(),
            host: "127.0.0.1".to_string(),
            // Port zero asks the OS for an isolated ephemeral listener.
            port: 0,
            tls_enabled: false,
            ..Config::default()
        };
        let daemon = Daemon::new(config).await.expect("compose daemon");
        let cancellation = CancellationToken::new();
        let stop = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            stop.cancel();
        });
        daemon
            .run(cancellation)
            .await
            .expect("start and stop daemon");
    }
}
