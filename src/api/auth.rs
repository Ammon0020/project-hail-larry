//! Authentication middleware, peer extraction, and WebSocket credential checks.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::{debug, warn};

use crate::pairing::Manager as PairingManager;
use crate::sync::{is_loopback_addr, AuthChecker};

use super::preview::{preview_authorization, PREVIEW_TOKEN_TTL};
use super::{ApiResponseError, AppState, TlsConnection};

/// Peer address for loopback checks. Missing `ConnectInfo` (unit tests) is
/// treated as loopback, matching `require_auth`.
pub(super) struct PeerAddr(pub(super) String);

impl<S> FromRequestParts<S> for PeerAddr
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(peer_addr_string(&parts.extensions)))
    }
}

/// Auth is deliberately performed before handlers, not in individual routes,
/// so every new protected route inherits the same LAN and CSRF policy.
pub(super) async fn require_auth(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    // peer_addr_string fails closed (non-loopback) when ConnectInfo is
    // absent in production; tests insert it via `oneshot_peer`.
    let remote_addr = peer_addr_string(request.extensions());
    match authorize_request(
        &state.pairing,
        &remote_addr,
        request.method(),
        request.headers(),
    ) {
        Ok(()) => next.run(request).await,
        Err(error) if request.method() == Method::GET || request.method() == Method::HEAD => {
            let Some(preview_auth) =
                preview_authorization(&state, request.uri(), request.headers())
            else {
                return error.into_response();
            };
            let secure = request.extensions().get::<TlsConnection>().is_some();
            let mut response = next.run(request).await;
            if let Some(token) = preview_auth.cookie_token {
                let mut cookie = format!(
                    "preview_token={token}; Path=/preview/{}/; HttpOnly; SameSite=Lax; Max-Age={}",
                    preview_auth.workspace_id,
                    PREVIEW_TOKEN_TTL.as_secs()
                );
                if secure {
                    cookie.push_str("; Secure");
                }
                if let Ok(value) = HeaderValue::from_str(&cookie) {
                    response.headers_mut().append(header::SET_COOKIE, value);
                }
            }
            response
        }
        Err(error) => error.into_response(),
    }
}

pub(super) fn pairing_auth_checker(manager: &PairingManager) -> AuthChecker {
    let manager = manager.clone();
    Arc::new(move |device_id, secret| manager.validate_credential(device_id, secret))
}

/// Apply Go-compatible loopback bypass and Origin/credential checks.
fn authorize_request(
    pairing: &PairingManager,
    remote_addr: &str,
    method: &Method,
    headers: &HeaderMap,
) -> Result<(), ApiResponseError> {
    if is_loopback_addr(remote_addr) {
        if is_mutating(method) && !loopback_origin_allowed(headers.get(header::ORIGIN)) {
            warn!(
                remote = remote_addr,
                "rejected cross-origin loopback API mutation"
            );
            return Err(ApiResponseError::forbidden(
                "cross-origin request not allowed",
            ));
        }
        return Ok(());
    }
    let (device_id, secret) = extract_credential(headers);
    if device_id.is_empty()
        || secret.is_empty()
        || !pairing.validate_credential(&device_id, &secret)
    {
        debug!(
            remote = remote_addr,
            "rejected unauthenticated remote API request"
        );
        return Err(ApiResponseError::unauthorized("unauthorized"));
    }
    Ok(())
}

fn is_mutating(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn loopback_origin_allowed(origin: Option<&HeaderValue>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    // Loopback Origin allowlist. `0.0.0.0` is intentionally excluded: it is
    // a wildcard, not a loopback address, and is not listed in
    // `is_loopback_addr` (src/sync/mod.rs). A page whose origin is
    // `http://0.0.0.0:<port>` should connect via `127.0.0.1`/`localhost`
    // instead.
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn extract_credential(headers: &HeaderMap) -> (String, String) {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|value| value.split_once(':'))
        .map(|(id, secret)| (id.to_string(), secret.to_string()))
        .unwrap_or_default()
}

/// Socket address string for loopback checks. Missing `ConnectInfo` is a
/// misconfiguration signal: in production we fail closed by returning a
/// non-loopback address so `authorize_request` requires a device credential
/// rather than silently treating the request as trusted loopback. Tests
/// insert `ConnectInfo` explicitly via `oneshot_peer`, but the loopback
/// fallback is kept under `cfg(test)` as defense-in-depth.
fn peer_addr_string(extensions: &axum::http::Extensions) -> String {
    extensions.get::<ConnectInfo<SocketAddr>>().map_or_else(
        || {
            if cfg!(test) {
                "127.0.0.1:0".to_string()
            } else {
                // Fail closed: a non-loopback address forces credential checks.
                "0.0.0.0:0".to_string()
            }
        },
        |connect| connect.0.to_string(),
    )
}

/// Device ID from the `Authorization: Bearer` header — empty on loopback-only
/// requests (where `authorize_request` bypasses credential checks) or when no
/// bearer credential is present.
pub(super) fn device_id_from_request(headers: &HeaderMap) -> String {
    extract_credential(headers).0
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, Method, StatusCode};

    use super::authorize_request;

    #[test]
    fn remote_request_without_a_credential_is_rejected() {
        let dir = tempfile::tempdir().expect("temporary state directory");
        let state = crate::api::test_support::test_state(dir.path());
        let headers = HeaderMap::new();
        let error = authorize_request(&state.pairing, "192.168.1.2:9000", &Method::GET, &headers)
            .expect_err("missing remote credential must fail");
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }
}
