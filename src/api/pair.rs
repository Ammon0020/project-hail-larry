//! Unauthenticated pairing endpoints and their per-peer abuse controls.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, State};
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tracing::{debug, error};

use crate::pairing::{Manager as PairingManager, PairingError};

use super::{decode_json_body, pairing_error, ApiResponseError, AppState};

const PAIR_RATE_PER_MINUTE: f64 = 5.0;
const PAIR_RATE_BURST: f64 = 5.0;
/// Idle window after which a per-IP bucket has fully refilled to
/// `PAIR_RATE_BURST` and can be evicted without changing observable behavior.
const PAIR_RATE_IDLE_TTL: Duration = Duration::from_mins(1);
/// Only run eviction after the map grows past this size. This avoids an O(n)
/// pass for normal traffic while bounding memory under many-source-IP floods.
const PAIR_RATE_EVICT_THRESHOLD: usize = 1024;

/// Maximum characters allowed in a paired device name.
const MAX_DEVICE_NAME_CHARS: usize = 64;
/// HTML-significant characters rejected in device names to prevent stored XSS
/// when the name is rendered in the browser UI.
const HTML_SIGNIFICANT_CHARS: &[char] = &['<', '>', '&', '"', '\''];

/// Per-IP token bucket matching Go's five-request burst and 5/minute refill.
pub(super) struct PairRateBucket {
    tokens: f64,
    updated_at: Instant,
}

/// Validates a paired device name before it reaches pairing state.
///
/// Device names are attacker-controlled and may be rendered in the UI, so both
/// denial-of-service and stored-XSS surfaces are bounded here.
fn validate_device_name(name: &str) -> Result<(), ApiResponseError> {
    if name.is_empty() {
        return Err(ApiResponseError::bad_request(
            "device name must not be empty",
        ));
    }
    let len = name.chars().count();
    if len > MAX_DEVICE_NAME_CHARS {
        return Err(ApiResponseError::bad_request(format!(
            "device name exceeds {MAX_DEVICE_NAME_CHARS} characters"
        )));
    }
    if let Some(ch) = name
        .chars()
        .find(|c| *c < ' ' || HTML_SIGNIFICANT_CHARS.contains(c))
    {
        return Err(ApiResponseError::bad_request(format!(
            "device name contains forbidden character `{ch}`"
        )));
    }
    Ok(())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct PairInitiateRequest {
    host: Option<String>,
    port: Option<u16>,
}

pub(super) async fn pair_initiate(
    State(state): State<AppState>,
    body: Result<Json<PairInitiateRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::PairingSession>, ApiResponseError> {
    // Empty/missing JSON is allowed (defaults); syntax errors are 400.
    let request = match body {
        Ok(Json(request)) => request,
        Err(rejection) => {
            if matches!(
                rejection,
                JsonRejection::MissingJsonContentType(_) | JsonRejection::BytesRejection(_)
            ) {
                PairInitiateRequest {
                    host: None,
                    port: None,
                }
            } else {
                return Err(ApiResponseError::bad_request("invalid request body"));
            }
        }
    };
    let configured = state.config.read().clone();
    let mut host = request
        .host
        .filter(|host| !host.is_empty())
        .unwrap_or(configured.host);
    if host == "0.0.0.0" || host == "::" {
        host = "localhost".to_string();
    }
    let port = request
        .port
        .unwrap_or_else(|| u16::try_from(configured.port).unwrap_or(7337));
    state
        .pairing
        .create_session(&host, port)
        .map(Json)
        .map_err(pairing_error)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PairVerifyRequest {
    passcode: Option<String>,
    token: Option<String>,
    device_name: String,
}

pub(super) async fn pair_verify_passcode(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let peer_key = pair_rate_key(&addr.ip());
    pair_verify(&state, body, |pairing, request| {
        pairing.verify_passcode(
            request.passcode.as_deref().unwrap_or_default(),
            request.device_name,
            Some(&peer_key),
        )
    })
}

pub(super) async fn pair_verify_token(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let peer_key = pair_rate_key(&addr.ip());
    pair_verify(&state, body, |pairing, request| {
        pairing.verify_token(
            request.token.as_deref().unwrap_or_default(),
            request.device_name,
            Some(&peer_key),
        )
    })
}

/// Shared decode and verification path for passcode and QR-token pairing.
fn pair_verify(
    state: &AppState,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
    verify: impl FnOnce(
        &PairingManager,
        PairVerifyRequest,
    ) -> Result<crate::interfaces::DeviceCredential, PairingError>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    validate_device_name(&request.device_name)?;
    verify(&state.pairing, request)
        .map(Json)
        .map_err(pairing_error)
}

/// Limit unauthenticated pairing requests before they allocate QR sessions or
/// enter passcode verification.
pub(super) async fn require_pair_rate_limit(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(
            || "127.0.0.1".to_string(),
            |connect| pair_rate_key(&connect.0.ip()),
        );
    if !allow_pair_request(&state, &peer) {
        debug!(peer, "pairing request rate limited");
        return ApiResponseError::rate_limited("pairing rate limit exceeded, try again later")
            .into_response();
    }
    next.run(request).await
}

/// Normalize a peer IP for pair-rate-limit bucketing. IPv6 addresses are
/// collapsed to their /64 prefix so rotating within one subnet cannot mint a
/// bucket per /128 address.
fn pair_rate_key(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V6(addr) => {
            let octets = addr.octets();
            let mut prefix = [0_u8; 16];
            prefix[..8].copy_from_slice(&octets[..8]);
            std::net::Ipv6Addr::from(prefix).to_string()
        }
        IpAddr::V4(_) => ip.to_string(),
    }
}

fn allow_pair_request(state: &AppState, peer: &str) -> bool {
    let mut buckets = match state.pair_rate.lock() {
        Ok(buckets) => buckets,
        Err(poisoned) => {
            error!("pairing rate-limit lock poisoned; recovering state");
            poisoned.into_inner()
        }
    };
    let now = Instant::now();
    // A bucket idle for at least 60 seconds has refilled to the burst size, so
    // recreating it on the next request is equivalent.
    if buckets.len() > PAIR_RATE_EVICT_THRESHOLD {
        buckets.retain(|_, bucket| now.duration_since(bucket.updated_at) < PAIR_RATE_IDLE_TTL);
    }
    let bucket = buckets
        .entry(peer.to_string())
        .or_insert_with(|| PairRateBucket {
            tokens: PAIR_RATE_BURST,
            updated_at: now,
        });
    bucket.tokens = (bucket.tokens
        + now.duration_since(bucket.updated_at).as_secs_f64() * PAIR_RATE_PER_MINUTE / 60.0)
        .min(PAIR_RATE_BURST);
    bucket.updated_at = now;
    if bucket.tokens < 1.0 {
        return false;
    }
    bucket.tokens -= 1.0;
    true
}

#[cfg(test)]
mod tests {
    use super::{allow_pair_request, AppState};

    #[test]
    fn pair_request_bucket_allows_a_five_request_burst() {
        let dir = tempfile::tempdir().expect("temporary state directory");
        let state: AppState = crate::api::test_support::test_state(dir.path());
        for _ in 0..5 {
            assert!(allow_pair_request(&state, "192.168.1.2"));
        }
        assert!(!allow_pair_request(&state, "192.168.1.2"));
    }
}
