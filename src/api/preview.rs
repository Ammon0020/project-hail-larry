//! Preview-session tickets, preview authorization, and preview file handlers.

use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, Uri};
use axum::response::Response;
use axum::Json;
use rand::Rng;
use serde::Serialize;

use crate::interfaces::WorkspaceManager;

use super::files::serve_workspace_file;
use super::{app_error, ApiResponseError, AppState};

/// Preview ticket and cookie lifetime.
pub(super) const PREVIEW_TOKEN_TTL: Duration = Duration::from_secs(30 * 60);

pub(super) struct PreviewToken {
    workspace_id: String,
    expires_at: Instant,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewSessionResponse {
    token: String,
    expires_in_seconds: u64,
}

/// Creates a one-time ticket used only to bootstrap a preview cookie.
pub(super) async fn create_preview_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PreviewSessionResponse>, ApiResponseError> {
    let exists = state
        .workspaces
        .list()
        .await
        .map_err(app_error)?
        .into_iter()
        .any(|workspace| workspace.id == id);
    if !exists {
        return Err(ApiResponseError::not_found(format!(
            "workspace {id} not found"
        )));
    }
    let token = new_preview_token();
    let now = Instant::now();
    let mut tokens = state
        .preview_tokens
        .lock()
        .map_err(|_| ApiResponseError::internal("preview token store lock poisoned"))?;
    tokens.retain(|_, ticket| ticket.expires_at > now);
    tokens.insert(
        token.clone(),
        PreviewToken {
            workspace_id: id,
            expires_at: now + PREVIEW_TOKEN_TTL,
        },
    );
    Ok(Json(PreviewSessionResponse {
        token,
        expires_in_seconds: PREVIEW_TOKEN_TTL.as_secs(),
    }))
}

/// Creates a 256-bit opaque value for a preview ticket or cookie.
fn new_preview_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// 404 for `/preview/{id}` with no file path — never fall through to the SPA.
pub(super) async fn preview_empty() -> Result<Response, ApiResponseError> {
    Err(ApiResponseError::not_found("preview path required"))
}

/// Serves a workspace file at `/preview/{id}/{*path}` so relative URLs in an
/// iframe (CSS/JS/images) resolve against the preview root rather than a
/// query-string raw endpoint.
pub(super) async fn preview_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response, ApiResponseError> {
    // Trailing slash / empty wildcard → no file to serve.
    let rel = path.trim_matches('/');
    if rel.is_empty() {
        return Err(ApiResponseError::not_found("preview path required"));
    }
    // Select the CSP based on workspace trust state. Trusted = permissive
    // (cross-origin resources allowed for legit local dev previews); untrusted
    // or unknown = restrictive (exfil channels blocked). The opaque origin from
    // `sandbox` without `allow-same-origin` protects IDE storage in both cases.
    let trusted = state.config.read().workspace_trust(&id).unwrap_or(false);
    serve_workspace_file(&state, &id, rel)
        .await
        .map(|mut response| {
            // Preview HTML is loaded inside a frontend iframe (BrowsePreview or
            // FileViewer) whose own `sandbox` attribute is the primary script
            // authority. The CSP here guards the direct-navigation case (a user
            // clicking a `/preview/{id}/evil.html` link outside any iframe):
            // `sandbox allow-scripts` without `allow-same-origin` forces an
            // opaque origin, so agent-written scripts that run cannot read IDE
            // cookies/localStorage or call authenticated `/api/*` as the IDE.
            // `allow-scripts` (not `allow-same-origin`) is chosen so the
            // BrowsePreview iframe's `sandbox="allow-scripts"` combines into
            // "scripts run, opaque origin" — script-driven static-site previews
            // work while staying contained. `frame-ancestors 'self'` restricts
            // who can embed the response to the IDE.
            //
            // Trusted workspaces get the permissive CSP (cross-origin CDNs,
            // APIs, WebSockets all work — the developer chose to trust the
            // workspace). Untrusted/unknown workspaces get `default-src 'none'
            // + per-type 'self' allows, which blocks all cross-origin resource
            // loading and exfil channels (`connect-src 'none'`, `form-action
            // 'none'`, etc.) while allowing relative subresources from
            // `/preview/{id}/` (`'self'` matches the response URL's origin per
            // CSP3 §2.2.2, not the sandboxed opaque origin).
            //
            // The shared `serve_workspace_file` CSP (`sandbox
            // allow-same-origin`, no allow-scripts) is overridden here because
            // it would combine with the iframe sandbox to block scripts
            // entirely (union of restrictions), breaking BrowsePreview.
            let csp = if trusted {
                HeaderValue::from_static("frame-ancestors 'self'; sandbox allow-scripts")
            } else {
                HeaderValue::from_static(
                    "frame-ancestors 'self'; sandbox allow-scripts; \
                     default-src 'none'; \
                     script-src 'self' 'unsafe-inline'; \
                     style-src 'self' 'unsafe-inline'; \
                     img-src 'self' data: blob:; \
                     font-src 'self' data:; \
                     media-src 'self' blob:; \
                     connect-src 'none'; \
                     frame-src 'none'; \
                     object-src 'none'; \
                     form-action 'none'; \
                     base-uri 'none'",
                )
            };
            response
                .headers_mut()
                .insert(header::CONTENT_SECURITY_POLICY, csp);
            response
        })
}

pub(super) struct PreviewAuthorization {
    pub(super) workspace_id: String,
    /// A fresh cookie value when an entry ticket was consumed.
    pub(super) cookie_token: Option<String>,
}

/// Authenticates a preview read and exchanges an entry ticket for a cookie.
///
/// Query tickets are intentionally one-time: URLs may reach browser history or
/// server logs, while the replacement value is `HttpOnly` and path-scoped.
pub(super) fn preview_authorization(
    state: &AppState,
    uri: &Uri,
    headers: &HeaderMap,
) -> Option<PreviewAuthorization> {
    let mut segments = uri.path().split('/').filter(|segment| !segment.is_empty());
    if segments.next()? != "preview" {
        return None;
    }
    let workspace_id = segments.next()?.to_string();
    let entry_token = uri.query().and_then(|query| {
        query.split('&').find_map(|part| {
            let (name, value) = part.split_once('=')?;
            (name == "previewToken").then_some(value)
        })
    });
    if let Some(token) = entry_token {
        return exchange_preview_ticket(state, &workspace_id, token).map(|cookie_token| {
            PreviewAuthorization {
                workspace_id,
                cookie_token: Some(cookie_token),
            }
        });
    }
    let cookie_token = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == "preview_token").then_some(value)
            })
        });
    let token = cookie_token?;
    let tokens = state.preview_tokens.lock().ok()?;
    let ticket = tokens.get(token)?;
    (ticket.expires_at > Instant::now() && ticket.workspace_id == workspace_id).then_some(
        PreviewAuthorization {
            workspace_id,
            cookie_token: None,
        },
    )
}

/// Consumes an entry ticket and returns a fresh cookie token for its workspace.
fn exchange_preview_ticket(
    state: &AppState,
    workspace_id: &str,
    entry_token: &str,
) -> Option<String> {
    let now = Instant::now();
    let mut tokens = state.preview_tokens.lock().ok()?;
    tokens.retain(|_, ticket| ticket.expires_at > now);
    let expires_at = match tokens.get(entry_token) {
        Some(ticket) if ticket.workspace_id == workspace_id => ticket.expires_at,
        _ => return None,
    };
    tokens.remove(entry_token);
    let cookie_token = new_preview_token();
    tokens.insert(
        cookie_token.clone(),
        PreviewToken {
            workspace_id: workspace_id.to_owned(),
            expires_at,
        },
    );
    Some(cookie_token)
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Method, Request, StatusCode};

    use crate::api::test_support::{
        bearer, json_body, oneshot, oneshot_peer, pending_actions_state, state, StateDirEnvGuard,
    };
    use crate::api::TlsConnection;
    use crate::interfaces::WorkspaceManager;

    /// Registers a temp workspace with a small static site fixture.
    async fn preview_fixture_workspace(
        state: &crate::api::AppState,
    ) -> (tempfile::TempDir, crate::interfaces::WorkspaceInfo) {
        let site = tempfile::tempdir().expect("preview fixture directory");
        tokio::fs::write(
            site.path().join("index.html"),
            b"<!DOCTYPE html><html><link rel=\"stylesheet\" href=\"./styles.css\"><body>hi</body></html>",
        )
        .await
        .expect("write index.html");
        tokio::fs::write(site.path().join("styles.css"), b"body{color:red}")
            .await
            .expect("write styles.css");
        let workspace = state
            .workspaces
            .register(site.path().to_str().expect("UTF-8 path"))
            .await
            .expect("register workspace");
        (site, workspace)
    }

    #[tokio::test]
    async fn preview_serves_html_inline_with_content_type() {
        let (_state_dir, state) = state();
        let (_site, workspace) = preview_fixture_workspace(&state).await;

        let response = oneshot(
            state,
            Request::builder()
                .uri(format!("/preview/{}/index.html", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(
            content_type.starts_with("text/html"),
            "expected text/html, got {content_type}"
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .and_then(|value| value.to_str().ok()),
            Some("inline")
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .and_then(|value| value.to_str().ok()),
            Some(concat!(
                "frame-ancestors 'self'; sandbox allow-scripts; ",
                "default-src 'none'; ",
                "script-src 'self' 'unsafe-inline'; ",
                "style-src 'self' 'unsafe-inline'; ",
                "img-src 'self' data: blob:; ",
                "font-src 'self' data:; ",
                "media-src 'self' blob:; ",
                "connect-src 'none'; ",
                "frame-src 'none'; ",
                "object-src 'none'; ",
                "form-action 'none'; ",
                "base-uri 'none'",
            ))
        );
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(
            String::from_utf8_lossy(&bytes).contains("styles.css"),
            "body should be the fixture HTML"
        );
    }

    #[tokio::test]
    async fn preview_rejects_path_traversal() {
        let (_state_dir, state) = state();
        let (_site, workspace) = preview_fixture_workspace(&state).await;

        let response = oneshot(
            state.clone(),
            Request::builder()
                .uri(format!("/preview/{}/../../../etc/passwd", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert!(
            matches!(
                response.status(),
                StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
            ),
            "traversal must be rejected, got {}",
            response.status()
        );

        let response = oneshot(
            state,
            Request::builder()
                .uri(format!(
                    "/preview/{}/{}etc/passwd",
                    workspace.id, "%2e%2e/%2e%2e/%2e%2e/"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert!(
            matches!(
                response.status(),
                StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
            ),
            "encoded traversal must be rejected, got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn preview_empty_path_returns_404() {
        let (_state_dir, state) = state();
        let (_site, workspace) = preview_fixture_workspace(&state).await;

        let response = oneshot(
            state,
            Request::builder()
                .uri(format!("/preview/{}", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn preview_requires_auth_for_non_loopback() {
        let (_state_dir, state) = state();
        let (_site, workspace) = preview_fixture_workspace(&state).await;

        let response = oneshot_peer(
            state,
            Request::builder()
                .uri(format!("/preview/{}/index.html", workspace.id))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn preview_session_cookie_authenticates_relative_asset() {
        let (_state_dir, state, credential) = pending_actions_state(0, false);
        let (_site, workspace) = preview_fixture_workspace(&state).await;
        let session = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/preview-session", workspace.id))
                .header(header::AUTHORIZATION, bearer(&credential))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        let token = json_body(session).await["token"]
            .as_str()
            .expect("preview token")
            .to_owned();

        let entry = oneshot_peer(
            state.clone(),
            Request::builder()
                .uri(format!(
                    "/preview/{}/index.html?previewToken={token}",
                    workspace.id
                ))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(entry.status(), StatusCode::OK);
        let cookie = entry
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("preview cookie");
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains(&format!("Path=/preview/{}/", workspace.id)));
        assert_ne!(
            cookie.split(';').next(),
            Some(format!("preview_token={token}").as_str())
        );

        let asset = oneshot_peer(
            state.clone(),
            Request::builder()
                .uri(format!("/preview/{}/styles.css", workspace.id))
                .header(
                    header::COOKIE,
                    cookie.split(';').next().expect("cookie pair"),
                )
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(asset.status(), StatusCode::OK);

        let replay = oneshot_peer(
            state,
            Request::builder()
                .uri(format!(
                    "/preview/{}/index.html?previewToken={token}",
                    workspace.id
                ))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tls_preview_entry_sets_secure_cookie() {
        let (_state_dir, state, credential) = pending_actions_state(0, false);
        let (_site, workspace) = preview_fixture_workspace(&state).await;
        let session = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/preview-session", workspace.id))
                .header(header::AUTHORIZATION, bearer(&credential))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        let token = json_body(session).await["token"]
            .as_str()
            .expect("preview token")
            .to_owned();
        let mut request = Request::builder()
            .uri(format!(
                "/preview/{}/index.html?previewToken={token}",
                workspace.id
            ))
            .body(Body::empty())
            .expect("request");
        request.extensions_mut().insert(TlsConnection);
        let entry = oneshot_peer(state, request, "10.0.0.1:9").await;
        let cookie = entry
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("preview cookie");
        assert!(cookie.contains("; Secure"));
    }

    #[tokio::test]
    async fn preview_token_wrong_workspace_is_rejected() {
        let (_state_dir, state, credential) = pending_actions_state(0, false);
        let (_site, first) = preview_fixture_workspace(&state).await;
        let (_other_site, second) = preview_fixture_workspace(&state).await;
        let session = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/preview-session", first.id))
                .header(header::AUTHORIZATION, bearer(&credential))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        let token = json_body(session).await["token"]
            .as_str()
            .expect("preview token")
            .to_owned();
        let response = oneshot_peer(
            state,
            Request::builder()
                .uri(format!(
                    "/preview/{}/index.html?previewToken={token}",
                    second.id
                ))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn preview_csp_reflects_trust_state() {
        let (state_dir, state) = state();
        let _env = StateDirEnvGuard::pin(state_dir.path());
        let (_site, workspace) = preview_fixture_workspace(&state).await;

        let workspace_id = workspace.id.clone();
        let csp = move |state: crate::api::AppState| {
            let workspace_id = workspace_id.clone();
            async move {
                let response = oneshot(
                    state,
                    Request::builder()
                        .uri(format!("/preview/{workspace_id}/index.html"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                response
                    .headers()
                    .get(header::CONTENT_SECURITY_POLICY)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_string()
            }
        };

        let restrictive = csp(state.clone()).await;
        assert!(
            restrictive.contains("default-src 'none'"),
            "unknown trust should get restrictive CSP, got: {restrictive}"
        );

        let update = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/workspaces/{}/trust", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"trusted":true}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(update.status(), StatusCode::OK);

        let permissive = csp(state.clone()).await;
        assert_eq!(
            permissive, "frame-ancestors 'self'; sandbox allow-scripts",
            "trusted workspace should get permissive CSP, got: {permissive}"
        );
        assert!(
            !permissive.contains("default-src 'none'"),
            "trusted CSP must not contain default-src 'none', got: {permissive}"
        );

        let update = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/workspaces/{}/trust", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"trusted":false}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(update.status(), StatusCode::OK);

        let restrictive_again = csp(state).await;
        assert!(
            restrictive_again.contains("default-src 'none'"),
            "untrusted workspace should get restrictive CSP, got: {restrictive_again}"
        );
    }
}
