//! WebSocket sync hub (Go `internal/sync/`).
//!
//! Blueprint references: Sec 12 (Multi-Client Synchronization).
//!
//! The server is authoritative. Connected devices are thin clients rendering
//! from the event stream via WebSockets. This module ports Go's `Hub`:
//!
//! - Per-client send queues ([`CLIENT_SEND_CAPACITY`] = 64) in a [`DashMap`]
//! - Auth-gated handshake (`deviceId` + `secret` query params) with loopback bypass
//! - Origin CSRF defense (empty Origin → 403; Origin host must match Host)
//! - Keepalive ping/pong (30s interval, 10s timeout)
//! - Live fan-out via [`Hub::broadcast`] (wired from [`crate::events::EventBus`]
//!   through [`crate::events::LiveFanout`] — Go `SyncHub.Broadcast` parity)
//! - Optional reconnect replay via durable query (`?after=` cursor), then live
//!   continues through [`Hub::broadcast`]
//! - Strict lagged recovery: durable resync instead of silently dropping events
//!
//! Serve with `into_make_service_with_connect_info::<SocketAddr>()` so
//! [`ConnectInfo`] can supply the peer address for loopback detection.

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::events::{LiveFanout, SharedEventBus};
use crate::interfaces::{Event, EventStore};

/// How often the keepalive task pings each client (matches Go `pingInterval`).
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Per-ping deadline; hung peers are closed before the next ping
/// (matches Go `pingTimeout`).
const PING_TIMEOUT: Duration = Duration::from_secs(10);

/// Bounded per-client outbound queue (matches Go `make(chan []byte, 64)`).
pub const CLIENT_SEND_CAPACITY: usize = 64;

/// Max inbound WebSocket message size (matches Go `SetReadLimit(1 << 20)`).
const MAX_MESSAGE_SIZE: usize = 1 << 20;

/// Validates a device credential pair.
///
/// Returns `true` when `device_id`/`secret` belong to a paired device. Wired
/// from pairing via [`Hub::set_auth_checker`]. When unset, auth is skipped
/// (tests); production always installs a checker.
pub type AuthChecker = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// Per-connection outbound handle stored in the hub registry.
struct ClientEntry {
    /// Non-blocking fan-out target for [`Hub::broadcast`].
    tx: mpsc::Sender<Vec<u8>>,
    /// Highest durable event ID delivered to this client (replay or live).
    last_seen_id: AtomicI64,
    /// Device ID authenticated at handshake (empty for loopback bypass).
    device_id: String,
    /// Per-connection cancel token so revocation can force-close the socket.
    cancel: CancellationToken,
}

/// WebSocket hub: client registry, broadcast, keepalive, reconnect replay.
///
/// Lifecycle is owned by [`CancellationToken`]: [`Hub::shutdown`] cancels it so
/// every client pump exits promptly instead of waiting on TCP timeouts.
pub struct Hub {
    clients: DashMap<u64, ClientEntry>,
    next_id: AtomicU64,
    auth: RwLock<Option<AuthChecker>>,
    cancel: CancellationToken,
    /// Optional durable bus for `after=` reconnect replay + lagged resync.
    bus: Option<SharedEventBus>,
}

impl Hub {
    /// Create a hub without an event bus (broadcast-only, Go parity).
    #[must_use]
    pub fn new() -> Arc<Self> {
        Self::with_bus(None)
    }

    /// Create a hub that can replay/resync via the event bus when clients pass
    /// `?after=<id>` (or when a slow client's send buffer fills).
    #[must_use]
    pub fn with_event_bus(bus: SharedEventBus) -> Arc<Self> {
        Self::with_bus(Some(bus))
    }

    fn with_bus(bus: Option<SharedEventBus>) -> Arc<Self> {
        Arc::new(Self {
            clients: DashMap::new(),
            next_id: AtomicU64::new(1),
            auth: RwLock::new(None),
            cancel: CancellationToken::new(),
            bus,
        })
    }

    /// Cancel the hub lifecycle token, draining all connection pumps.
    ///
    /// Safe to call multiple times. Clients unregister themselves as pumps exit.
    pub fn shutdown(&self) {
        if !self.cancel.is_cancelled() {
            info!("sync hub shutting down; draining connections");
            self.cancel.cancel();
        }
    }

    /// Whether [`Hub::shutdown`] has been called.
    #[must_use]
    pub fn is_shutdown(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Install (or replace) the credential validator used by the WS handshake.
    pub fn set_auth_checker(&self, checker: AuthChecker) {
        match self.auth.write() {
            Ok(mut guard) => *guard = Some(checker),
            Err(poisoned) => {
                // Fail loudly: recover poisoned lock rather than leaving auth open.
                error!("sync auth lock poisoned; recovering checker");
                *poisoned.into_inner() = Some(checker);
            }
        }
    }

    /// Number of currently registered WebSocket clients.
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Serialize `event` as JSON and fan out to every connected client.
    ///
    /// Uses non-blocking `try_send` so a slow client cannot stall the hub.
    /// Events already delivered via reconnect replay (ID ≤ `last_seen_id`) are
    /// skipped to avoid duplicates. Unlike Go (which silently skips full
    /// buffers), a full buffer triggers a durable EventBus resync when a bus is
    /// configured; without a bus the slow client is dropped so loss is loud.
    pub fn broadcast(&self, event: &Event) {
        let data = match serde_json::to_vec(event) {
            Ok(bytes) => bytes,
            Err(err) => {
                error!(%err, "sync: marshal event for broadcast");
                return;
            }
        };

        // Snapshot keys first so we never hold a DashMap shard guard across
        // try_send / spawn work (avoids deadlock with unregister).
        let ids: Vec<u64> = self.clients.iter().map(|e| *e.key()).collect();
        for id in ids {
            let Some(entry) = self.clients.get(&id) else {
                continue;
            };
            // Dedupe against reconnect replay / prior delivery.
            if event.id > 0 && event.id <= entry.last_seen_id.load(Ordering::Relaxed) {
                continue;
            }
            match entry.tx.try_send(data.clone()) {
                Ok(()) => {
                    if event.id > 0 {
                        entry.last_seen_id.fetch_max(event.id, Ordering::Relaxed);
                    }
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    let cursor = entry.last_seen_id.load(Ordering::Relaxed);
                    warn!(
                        client_id = id,
                        cursor, "sync: client send buffer full; scheduling resync"
                    );
                    drop(entry);
                    self.schedule_resync(id, cursor);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    drop(entry);
                    self.unregister(id);
                }
            }
        }
    }

    /// Axum router exposing `GET /ws` for unit/integration tests and S-SERVER.
    ///
    /// Must be served with `into_make_service_with_connect_info::<SocketAddr>()`.
    pub fn into_router(self: Arc<Self>) -> Router {
        Router::new().route("/ws", get(handle_ws)).with_state(self)
    }

    fn child_token(&self) -> CancellationToken {
        self.cancel.child_token()
    }

    fn register(
        &self,
        tx: mpsc::Sender<Vec<u8>>,
        after_id: i64,
        device_id: String,
    ) -> (u64, CancellationToken) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel = self.cancel.child_token();
        self.clients.insert(
            id,
            ClientEntry {
                tx,
                last_seen_id: AtomicI64::new(after_id),
                device_id,
                cancel: cancel.clone(),
            },
        );
        debug!(client_id = id, after_id, "sync: client registered");
        (id, cancel)
    }

    fn unregister(&self, id: u64) {
        if self.clients.remove(&id).is_some() {
            debug!(client_id = id, "sync: client unregistered");
        }
    }

    /// Force-close all WebSocket connections authenticated as `device_id`.
    /// Called when a device is revoked so it stops receiving events immediately.
    pub fn disconnect_device(&self, device_id: &str) {
        if device_id.is_empty() {
            return;
        }
        let ids: Vec<u64> = self
            .clients
            .iter()
            .filter(|e| e.device_id == device_id)
            .map(|e| *e.key())
            .collect();
        for id in ids {
            if let Some(entry) = self.clients.get(&id) {
                entry.cancel.cancel();
                debug!(
                    client_id = id,
                    device_id, "sync: disconnecting revoked device"
                );
            }
        }
    }

    /// Update the durable cursor for a registered client (EventBus feed path).
    fn note_delivered(&self, client_id: u64, event_id: i64) {
        if let Some(entry) = self.clients.get(&client_id) {
            entry.last_seen_id.fetch_max(event_id, Ordering::Relaxed);
        }
    }

    /// When a client's outbound buffer is full, either resync from the durable
    /// bus or drop the connection so the gap is not silently skipped.
    fn schedule_resync(&self, client_id: u64, after_id: i64) {
        let Some(bus) = self.bus.clone() else {
            warn!(
                client_id,
                "sync: dropping slow client (no event bus for resync)"
            );
            self.unregister(client_id);
            return;
        };
        let Some(entry) = self.clients.get(&client_id) else {
            return;
        };
        let tx = entry.tx.clone();
        drop(entry);

        let cancel = self.child_token();
        tokio::spawn(async move {
            match resync_replay_only(&bus, &tx, after_id, &cancel).await {
                Ok(last) => {
                    debug!(client_id, last, "sync: lagged resync completed");
                }
                Err(err) => {
                    warn!(client_id, %err, "sync: lagged resync failed");
                }
            }
        });
    }
}

impl LiveFanout for Hub {
    fn fanout(&self, event: &Event) {
        self.broadcast(event);
    }
}

/// Query parameters on `GET /ws` (browser-compatible credential channel).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsQuery {
    /// Paired device id (`deviceId` on the wire).
    pub device_id: Option<String>,
    /// Device secret paired with `device_id`.
    pub secret: Option<String>,
    /// Optional durable cursor: replay events with `id > after`, then live.
    pub after: Option<i64>,
}

/// Auth + Origin gate matching Go `Hub.HandleWS` order exactly.
///
/// 1. Non-loopback + AuthChecker set → require valid `deviceId`/`secret` (401)
/// 2. Origin empty or host ≠ Host header → 403
///
/// Loopback bypasses auth but still requires a valid Origin (CSRF defense
/// against malicious pages opening `ws://localhost/...`).
pub fn authorize_handshake(
    auth: Option<&AuthChecker>,
    remote_addr: &str,
    device_id: Option<&str>,
    secret: Option<&str>,
    origin: Option<&str>,
    host: &str,
) -> Result<(), (StatusCode, &'static str)> {
    // Auth before Origin — LAN without creds must get 401 even if Origin is bad.
    if let Some(checker) = auth {
        if !is_loopback_addr(remote_addr) {
            let id = device_id.unwrap_or("");
            let sec = secret.unwrap_or("");
            if id.is_empty() || sec.is_empty() || !checker(id, sec) {
                return Err((StatusCode::UNAUTHORIZED, "unauthorized\n"));
            }
        }
    }

    if !origin_allowed(origin, host) {
        return Err((StatusCode::FORBIDDEN, "origin not allowed\n"));
    }
    Ok(())
}

/// Reports whether `remote_addr` (host:port or bare host) is loopback.
///
/// Matches Go `isLoopbackAddr`: `127.0.0.1`, `::1`, and `localhost`. Also
/// recognizes IPv4-mapped IPv6 (`::ffff:127.0.0.1`), which is how a dual-stack
/// IPv6 listener reports localhost IPv4 connections via `ConnectInfo`.
#[must_use]
pub fn is_loopback_addr(remote_addr: &str) -> bool {
    // Prefer std parsing for host:port (handles IPv6 `[::1]:1234`).
    if let Ok(addr) = remote_addr.parse::<SocketAddr>() {
        // `to_canonical` maps IPv4-mapped IPv6 (`::ffff:127.0.0.1`) to IPv4 so
        // `Ipv4Addr::is_loopback` (127.0.0.0/8) applies; for `::1` and plain
        // IPv4 it is identity.
        return addr.ip().to_canonical().is_loopback();
    }
    let host = match remote_addr.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => remote_addr,
    };
    host == "127.0.0.1"
        || host == "::1"
        || host == "localhost"
        || host
            .strip_prefix("::ffff:")
            .is_some_and(|v4| v4 == "127.0.0.1")
}

/// CSRF Origin check matching Go `originAllowed`.
///
/// Empty Origin is rejected (browser-facing endpoint). Origin URL host must
/// equal the request Host (case-insensitive), including port.
#[must_use]
pub fn origin_allowed(origin: Option<&str>, host: &str) -> bool {
    let Some(origin) = origin.filter(|o| !o.is_empty()) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    let Some(origin_host) = url.host_str() else {
        return false;
    };
    // Reconstruct host:port like Go's `url.URL.Host`.
    let origin_host_port = match url.port() {
        Some(port) => format!("{origin_host}:{port}"),
        None => origin_host.to_string(),
    };
    origin_host_port.eq_ignore_ascii_case(host)
}

/// Axum `GET /ws` handler: gate → upgrade → register → pump.
async fn handle_ws(
    State(hub): State<Arc<Hub>>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
) -> Response {
    if hub.is_shutdown() {
        return (StatusCode::SERVICE_UNAVAILABLE, "hub shut down\n").into_response();
    }

    let auth = match hub.auth.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            error!("sync auth lock poisoned during handshake");
            poisoned.into_inner().clone()
        }
    };

    let remote = addr.to_string();
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());

    if let Err((status, body)) = authorize_handshake(
        auth.as_ref(),
        &remote,
        query.device_id.as_deref(),
        query.secret.as_deref(),
        origin,
        host,
    ) {
        debug!(%status, remote = %remote, "sync: websocket handshake rejected");
        return (status, body).into_response();
    }

    // `after` is optional: when present (and a bus is configured), the client
    // gets durable replay then live EventBus delivery with ID dedupe.
    let after = query.after;
    let device_id = query.device_id.unwrap_or_default();
    ws.max_message_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| run_client_pumps(hub, socket, after, device_id))
}

/// Unified read / write / keepalive / optional EventBus feed for one client.
async fn run_client_pumps(hub: Arc<Hub>, socket: WebSocket, after: Option<i64>, device_id: String) {
    let (sink, stream) = socket.split();
    let (tx, rx) = mpsc::channel::<Vec<u8>>(CLIENT_SEND_CAPACITY);
    let after_id = after.unwrap_or(0);
    let (client_id, cancel) = hub.register(tx.clone(), after_id, device_id);

    // Updated on every inbound frame / pong so keepalive can detect dead peers.
    let last_pong = Arc::new(tokio::sync::Mutex::new(Instant::now()));

    let write_cancel = cancel.clone();
    let last_pong_w = Arc::clone(&last_pong);
    let mut write_task = tokio::spawn(async move {
        write_pump(sink, rx, write_cancel, last_pong_w, client_id).await;
    });

    let read_cancel = cancel.clone();
    let last_pong_r = Arc::clone(&last_pong);
    let mut read_task = tokio::spawn(async move {
        read_pump(stream, read_cancel, last_pong_r).await;
    });

    // Reconnect replay when the client supplied `after` and a bus exists.
    // Live delivery always goes through Hub::broadcast (EventBus LiveFanout);
    // replaying here only fills the gap, then this task exits.
    let feed_task = match (after, hub.bus.as_ref()) {
        (Some(cursor), Some(bus)) => {
            let bus = Arc::clone(bus);
            let tx_feed = tx;
            let cancel_feed = cancel.clone();
            let hub_feed = Arc::clone(&hub);
            Some(tokio::spawn(async move {
                match resync_replay_only(&bus, &tx_feed, cursor, &cancel_feed).await {
                    Ok(last) => {
                        hub_feed.note_delivered(client_id, last);
                        debug!(
                            client_id,
                            last, "sync: reconnect replay complete; live via hub broadcast"
                        );
                    }
                    Err(err) => {
                        warn!(client_id, %err, "sync: reconnect replay error");
                    }
                }
            }))
        }
        _ => None,
    };

    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = &mut write_task => {}
        _ = &mut read_task => {}
    }

    // Abort siblings so pumps do not leak after one side exits.
    write_task.abort();
    read_task.abort();
    if let Some(t) = feed_task {
        t.abort();
    }
    hub.unregister(client_id);
    debug!(client_id, "sync: client session ended");
}

/// Outbound pump: queued events + periodic Ping with pong deadline.
async fn write_pump(
    mut sink: futures_util::stream::SplitSink<WebSocket, Message>,
    mut rx: mpsc::Receiver<Vec<u8>>,
    cancel: CancellationToken,
    last_pong: Arc<tokio::sync::Mutex<Instant>>,
    client_id: u64,
) {
    let mut interval = tokio::time::interval(PING_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await; // consume immediate tick

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = sink.send(Message::Close(None)).await;
                return;
            }
            msg = rx.recv() => {
                match msg {
                    Some(data) => {
                        let text = match String::from_utf8(data) {
                            Ok(s) => s,
                            Err(err) => {
                                error!(client_id, %err, "sync: non-utf8 event payload");
                                continue;
                            }
                        };
                        if sink.send(Message::Text(text.into())).await.is_err() {
                            return;
                        }
                    }
                    None => {
                        let _ = sink.send(Message::Close(None)).await;
                        return;
                    }
                }
            }
            _ = interval.tick() => {
                let sent_at = Instant::now();
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
                let deadline = sent_at + PING_TIMEOUT;
                loop {
                    if cancel.is_cancelled() {
                        return;
                    }
                    {
                        let guard = last_pong.lock().await;
                        if *guard >= sent_at {
                            break;
                        }
                    }
                    if Instant::now() >= deadline {
                        warn!(client_id, "sync: keepalive ping failed (no pong)");
                        let _ = sink.send(Message::Close(None)).await;
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    }
}

/// Inbound pump: update liveness on frames; app messages ignored (Phase 1).
async fn read_pump(
    mut stream: futures_util::stream::SplitStream<WebSocket>,
    cancel: CancellationToken,
    last_pong: Arc<tokio::sync::Mutex<Instant>>,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            frame = stream.next() => {
                match frame {
                    Some(Ok(Message::Pong(_)))
                    | Some(Ok(Message::Ping(_)))
                    | Some(Ok(Message::Text(_)))
                    | Some(Ok(Message::Binary(_))) => {
                        *last_pong.lock().await = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Err(err)) => {
                        debug!(%err, "sync: websocket read error");
                        return;
                    }
                }
            }
        }
    }
}

/// Drain durable events (`id > after_id`) into `tx` for a one-shot catch-up.
async fn resync_replay_only(
    bus: &SharedEventBus,
    tx: &mpsc::Sender<Vec<u8>>,
    after_id: i64,
    cancel: &CancellationToken,
) -> Result<i64, String> {
    let events = bus
        .query_all(after_id, 0)
        .await
        .map_err(|e| format!("resync query_all: {e}"))?;
    let mut last = after_id;
    for event in events {
        if cancel.is_cancelled() {
            break;
        }
        last = event.id;
        let data = serde_json::to_vec(&event).map_err(|e| format!("marshal event: {e}"))?;
        if tx.send(data).await.is_err() {
            break;
        }
    }
    Ok(last)
}
