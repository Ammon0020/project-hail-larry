//! Rate limiting configuration (stub).
//!
//! S-ARCH acceptance criterion: rate limiting uses a supported
//! tower-compatible governor integration. We use `tower_governor`, which
//! wraps the `governor` crate as a `tower::Layer` and keys limits by client
//! IP — matching the Go pairing-endpoint rate limit semantics.
//!
//! The real per-route configuration (pairing endpoints, unauthenticated
//! surfaces, Go-equivalent burst/period values) lands in S-SERVER after
//! S-CONTRACT captures the exact Go limits. This stub exposes the builder
//! shape so downstream stories can wire it without redesign.

use axum::body::Body;
use governor::middleware::NoOpMiddleware;
use std::time::Duration;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::PeerIpKeyExtractor;
use tower_governor::GovernorLayer;

/// Default per-IP rate limit for unauthenticated endpoints (requests/sec).
///
/// Conservative placeholder; S-SERVER will pin this to the verified Go value
/// once S-CONTRACT captures it. Chosen to be permissive enough for legitimate
/// pairing flows while bounding brute-force attempts.
pub const DEFAULT_PER_IP_PER_SECOND: u64 = 4;

/// Default burst size absorbing a single device's pairing retry burst while
/// keeping the steady-state limit. S-SERVER will tune against Go's values.
pub const DEFAULT_BURST: u32 = 8;

/// A configured `GovernorLayer` keyed by peer IP with the no-op middleware
/// (stateless rate limiting — we don't need per-key state headers here).
/// `RespBody` is pinned to axum's `Body` so the layer can produce standard
/// 429 responses via the crate's `From<GovernorError>` impl.
pub type IpGovernorLayer = GovernorLayer<PeerIpKeyExtractor, NoOpMiddleware, Body>;

/// Build a `GovernorLayer` keyed by client IP with a per-second allowance.
///
/// Returns `None` if `per_second` is zero (governor requires a non-zero
/// quota); callers should treat that as "rate limiting disabled" and log it.
#[must_use]
pub fn build_ip_rate_limit_layer(per_second: u64, burst: u32) -> Option<IpGovernorLayer> {
    if per_second == 0 || burst == 0 {
        return None;
    }
    let config = GovernorConfigBuilder::default()
        .per_second(per_second)
        .burst_size(burst)
        .finish()?;
    Some(IpGovernorLayer::new(std::sync::Arc::new(config)))
}

/// Convenience: the default governor layer for unauthenticated endpoints.
#[must_use]
pub fn default_unauthenticated_layer() -> Option<IpGovernorLayer> {
    build_ip_rate_limit_layer(DEFAULT_PER_IP_PER_SECOND, DEFAULT_BURST)
}

/// Placeholder for the Go-equivalent idle timeout. Documented here so
/// S-SERVER can wire it without re-deriving the constant.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_mins(1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_default_layer() {
        // S-ARCH: the governor integration must construct cleanly.
        let layer = default_unauthenticated_layer();
        assert!(
            layer.is_some(),
            "default governor layer must build with non-zero quota"
        );
    }

    #[test]
    fn zero_per_second_disables() {
        // A zero quota is a misconfiguration; we surface it as `None` rather
        // than panicking inside governor.
        let layer = build_ip_rate_limit_layer(0, 8);
        assert!(layer.is_none());
    }

    #[test]
    fn zero_burst_disables() {
        let layer = build_ip_rate_limit_layer(4, 0);
        assert!(layer.is_none());
    }
}
