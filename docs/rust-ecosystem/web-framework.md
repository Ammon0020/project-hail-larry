# Web Framework Reference — axum

> **Crate:** `axum` (tokio-rs/axum) + `tower` / `tower-http`
> Context7 ID: `/tokio-rs/axum` (263 snippets, High reputation, v0.8.4)

Replaces `net/http` (server, routing, middleware) and `nhooyr.io/websocket`.

## Routing (replaces `http.ServeMux` pattern routing)

Go's `s.mux.HandleFunc("GET /api/workspaces/{id}/files", handler)` maps
directly to axum:

```rust
use axum::{Router, routing::{get, post, delete, patch}, extract::Path};

let app = Router::new()
    .route("/health", get(handle_health))
    .route("/api/workspaces", get(handle_list_workspaces).post(handle_register_workspace))
    .route("/api/workspaces/{id}/files", get(handle_file_tree))
    .route("/api/workspaces/{id}/file", get(handle_read_file).post(handle_write_file))
    .route("/api/sessions/{id}/prompt", post(handle_send_prompt))
    .route("/api/sessions/{id}/providers/{provider_id}", put(handle_set_provider).delete(handle_disable_provider));
```

Path params: `Path(id): Path<String>` or `Path((id, provider_id)): Path<(String, String)>`.

## State Sharing (replaces `Deps` struct passed to handlers)

```rust
#[derive(Clone)]
struct AppState {
    event_store: Arc<dyn EventStore>,
    workspace_mgr: Arc<dyn WorkspaceManager>,
    acp_client: Arc<AcpClient>,
    pairing_mgr: Arc<PairingManager>,
    sync_hub: Arc<SyncHub>,
    config: Arc<Config>,
    // ...
}

let app = Router::new()
    .route("/api/workspaces", get(handle_list_workspaces))
    .with_state(app_state);
```

Handlers extract state: `async fn handle_list_workspaces(State(state): State<AppState>)`.

## Middleware (replaces `requireAuth` wrapper)

```rust
use axum::middleware::{self, Next};
use axum::extract::Request;

async fn require_auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let token = req.headers().get("authorization")
        .and_then(|h| h.to_str().ok());
    // validate against pairing manager via state...
    req.extensions_mut().insert(DeviceId::new(id));
    Ok(next.run(req).await)
}

// Apply to a group of routes:
let api = Router::new()
    .route("/api/workspaces", get(/*...*/))
    .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
```

Rate limiting for pairing endpoints: use a bounded `governor`/tower-governor
policy keyed by client IP. Do not use `tower::limit::rate`; validate the chosen
integration and Go-equivalent limits in S-ARCH/S-CONTRACT.

## WebSocket (replaces `nhooyr.io/websocket` + `sync.Hub`)

axum has built-in WebSocket support via `axum::extract::ws`:

```rust
use axum::extract::ws::{WebSocket, WebSocketUpgrade, Message};

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    // Broadcast hub: subscribe to a tokio::broadcast::Sender<Event>
    // Read from receiver for client messages, write to sender for broadcasts
}
```

The Go `sync.Hub` (client registry, broadcast, keepalive ping/pong,
reconnection sync) maps to a `tokio::broadcast` channel per event stream +
a `DashMap` of connected client senders. Ping/pong is handled automatically
by tungstenite; the 30s keepalive becomes a `tokio::time::interval` task.

## TLS (replaces `http.Server.ListenAndServeTLS`)

```rust
use axum_server::tls_rustls::RustlsConfig;

let config = RustlsConfig::from_pem(cert_pem, key_pem).await?;
axum_server::bind_tls(addr, config).serve(app.into_make_service()).await?;
```

For dual HTTP+HTTPS listeners (the app runs both when TLS enabled), spawn
two coordinated tasks from one cancellation root. Before creating TLS config,
install exactly one selected rustls `CryptoProvider`; mixed provider features
can otherwise cause runtime selection failures. S-ARCH must choose and test
whether to use community-maintained `axum-server` or an explicit
`tokio-rustls` accept loop.

## Static / Embedded Frontend (replaces `go:embed` + `http.FileServer`)

Use `rust-embed` (see [build-and-embed.md](build-and-embed.md)) + a fallback
handler that serves `index.html` for SPA client-side routing:

```rust
// Fallback: serve embedded file or index.html for SPA routes
async fn serve_frontend(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    match EmbeddedAsset::get(path) {
        Some(content) => /* return with content-type */,
        None => EmbeddedAsset::get("index.html") /* SPA fallback */,
    }
}
```

## Fetching Live Docs

```
context7: query-docs /tokio-rs/axum "WebSocket upgrade broadcast"
context7: query-docs /tokio-rs/axum "middleware state sharing"
context7: query-docs /tokio-rs/axum "path params nested routes"
```
