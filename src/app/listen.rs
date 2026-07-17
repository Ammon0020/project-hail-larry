//! HTTP listener composition for the browser smoke-test server.
//!
//! This is deliberately a small composition root, not the future full daemon:
//! it loads local config, constructs the services required by the first browser
//! path, and binds one HTTP listener. TLS dual-listening remains S-DAEMON work.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::net::TcpListener;
use tracing::info;

use crate::acp::{AgentRegistry, Client, ClientDeps};
use crate::api::{self, AppState};
use crate::config::{Config, ConfigStore};
use crate::events::EventBus;
use crate::interfaces::WorkspaceManager;
use crate::pairing::Manager as PairingManager;
use crate::permissions::Manager as PermissionsManager;
use crate::sync::Hub;
use crate::workspace::Manager as WorkspaceManagerImpl;

/// Build the first-batch HTTP state from persistent local configuration.
///
/// The state directory is created before SQLite and pairing state are opened;
/// a failure is returned to the CLI rather than making a partially configured
/// listener available.
pub async fn build_http_state() -> Result<AppState> {
    let config = Config::load().context("load local-agent configuration")?;
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("create state directory {}", config.data_dir))?;

    let store = ConfigStore::new(config.clone());
    let workspaces = Arc::new(WorkspaceManagerImpl::new());
    for path in &config.workspaces {
        if let Err(error) = workspaces.register(path).await {
            // A missing workspace must not stop the local UI from showing the
            // rest of its state. It is surfaced loudly for repair by the host.
            tracing::warn!(workspace = %path, %error, "skipping unavailable configured workspace");
        }
    }

    let pairing =
        PairingManager::new(Path::new(&config.data_dir), None).context("open pairing state")?;
    if config.pairing_ttl_seconds > 0 {
        pairing.set_session_ttl(Duration::from_secs(config.pairing_ttl_seconds as u64));
    }
    if config.credential_inactivity_ttl_seconds > 0 {
        pairing.set_inactivity_ttl(Duration::from_secs(
            config.credential_inactivity_ttl_seconds as u64,
        ));
    }

    let events = Arc::new(EventBus::open(&config.db_path).context("open event store")?);
    let hub = Hub::with_event_bus(Arc::clone(&events));
    let permissions = PermissionsManager::new(None);
    // Detaching is intentional: the task holds only a weak reference and exits
    // when AppState drops, while its timer prevents stale ACP permission waits.
    let _permission_sweeper = permissions.start_sweeper();
    let registry = Arc::new(AgentRegistry::from_agents(config.agents));
    let acp = Arc::new(Client::new(ClientDeps {
        registry,
        workspaces: workspaces.clone(),
        permissions: permissions.clone(),
        event_bus: events.clone(),
    }));

    Ok(AppState::new(
        store,
        pairing,
        workspaces,
        events,
        hub,
        acp,
        permissions,
    ))
}

/// Bind the configured HTTP address and serve the UI-smoke router.
///
/// Axum/hyper exposes header/body and request cancellation controls rather
/// than Go's exact `http.Server` timeout fields. The 10 MB router limit and
/// WebSocket-level limits are active now; read/write/idle timeout parity is
/// deferred with TLS dual-listening.
pub async fn serve_http() -> Result<()> {
    let state = build_http_state().await?;
    let config = state.config.read().clone();
    let port = u16::try_from(config.port)
        .map_err(|_| anyhow!("invalid configured port {}", config.port))?;
    let address: SocketAddr = format!("{}:{port}", config.host)
        .parse()
        .with_context(|| format!("parse HTTP listen address {}:{port}", config.host))?;
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("bind HTTP listener at {address}"))?;
    info!(address = %address, "serving local-agent UI smoke HTTP endpoint");
    axum::serve(
        listener,
        api::router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("HTTP listener exited")
}
