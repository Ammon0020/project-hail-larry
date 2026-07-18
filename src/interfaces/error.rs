//! Layer-specific typed errors and a single API-error mapper.
//!
//! Stale revisions, missing resources, unsupported capabilities, and validation
//! failures are distinguished by type (not by error-string scanning). The REST
//! layer maps these into the Go daemon's `{"error": "..."}` JSON body + HTTP
//! status via [`map_api_error`].
//!
//! Service packages may define their own finer-grained errors and convert into
//! [`AppError`] at the API boundary. Path containment failures reuse
//! [`crate::pathutil::PathError`].

use crate::pathutil::PathError;
use thiserror::Error;

/// Cross-cutting application error used at service / API boundaries.
///
/// Variants cover the stable failure classes the REST layer must distinguish:
/// missing resources (404), validation (400), conflict/stale revision (409),
/// unsupported capabilities (501), auth (401/403), rate limiting (429), and
/// internal failures (500).
#[derive(Debug, Error)]
pub enum AppError {
    /// Requested resource does not exist (workspace, session, device, upload, …).
    ///
    /// `resource` is the full client-facing message (Go `err.Error()`), e.g.
    /// `"session not found: nonexistent"` or `"stat file: lstat …"`.
    #[error("{resource}")]
    NotFound {
        /// Full Go-compatible error string returned in `{"error":…}`.
        resource: String,
        /// Optional underlying cause (not shown to clients).
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Optimistic concurrency failure: expected revision does not match current.
    #[error("stale revision: file has been modified since last read")]
    StaleRevision,

    /// Resource conflict (e.g. rename target exists, mkdir path is a file).
    #[error("{0}")]
    Conflict(String),

    /// Agent or feature is not supported (e.g. providers capability missing).
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// Caller-supplied input failed validation.
    #[error("validation failed: {0}")]
    Validation(String),

    /// Authentication required or credential invalid.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// Authenticated but not permitted (e.g. cross-origin, remote registration disabled).
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// Rate limit exceeded (pairing verify, unauthenticated endpoints).
    #[error("rate limited: {0}")]
    RateLimited(String),

    /// Path traversal / symlink containment failure.
    #[error(transparent)]
    Path(#[from] PathError),

    /// Configuration error bubbled from the config layer.
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),

    /// Catch-all internal failure (I/O, DB, unexpected).
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Convenience constructor for [`AppError::NotFound`] without a source error.
    ///
    /// Prefer the full Go-style message (`"session not found: {id}"`). Kind-only
    /// helpers [`Self::not_found_kind`] / [`Self::not_found_id`] build that text.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            resource: message.into(),
            source: None,
        }
    }

    /// `"{kind} not found"` (no id) — matches Go when the id is unavailable.
    pub fn not_found_kind(kind: &str) -> Self {
        Self::not_found(format!("{kind} not found"))
    }

    /// `"{kind} not found: {id}"` — matches Go `fmt.Errorf("%s not found: %s", …)`.
    pub fn not_found_id(kind: &str, id: &str) -> Self {
        Self::not_found(format!("{kind} not found: {id}"))
    }

    /// Convenience constructor for [`AppError::NotFound`] with a source error.
    pub fn not_found_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::NotFound {
            resource: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Convenience constructor for validation failures.
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    /// Convenience constructor for conflict failures (HTTP 409).
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    /// Convenience constructor for unsupported capabilities.
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }

    /// Convenience constructor for internal failures.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

/// HTTP status codes used by the Go daemon's API error responses.
///
/// Kept as raw `u16` so the interfaces layer does not depend on axum/http types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiStatusCode(pub u16);

impl ApiStatusCode {
    pub const BAD_REQUEST: Self = Self(400);
    pub const UNAUTHORIZED: Self = Self(401);
    pub const FORBIDDEN: Self = Self(403);
    pub const NOT_FOUND: Self = Self(404);
    pub const CONFLICT: Self = Self(409);
    pub const TOO_MANY_REQUESTS: Self = Self(429);
    pub const INTERNAL_SERVER_ERROR: Self = Self(500);
    pub const NOT_IMPLEMENTED: Self = Self(501);
}

/// API error body matching Go `writeError`: `{"error": "<msg>"}`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
}

/// Fully mapped API error: status + body ready for the REST layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: ApiStatusCode,
    pub body: ApiErrorBody,
}

impl ApiError {
    /// Construct from status + message (the message becomes the `error` field).
    pub fn new(status: ApiStatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                error: message.into(),
            },
        }
    }
}

/// Map a typed [`AppError`] to the Go-compatible HTTP status + JSON body.
///
/// Status mapping mirrors `internal/server` usage:
/// - validation / path traversal → 400
/// - unauthorized → 401
/// - forbidden → 403
/// - not found → 404
/// - stale revision / conflict → 409
/// - rate limited → 429
/// - unsupported → 501
/// - internal / config / other → 500
pub fn map_api_error(err: &AppError) -> ApiError {
    match err {
        AppError::NotFound { resource, .. } => {
            ApiError::new(ApiStatusCode::NOT_FOUND, resource.clone())
        }
        AppError::StaleRevision => ApiError::new(
            ApiStatusCode::CONFLICT,
            "stale revision: file has been modified since last read",
        ),
        AppError::Conflict(msg) => ApiError::new(ApiStatusCode::CONFLICT, msg.clone()),
        AppError::Unsupported(msg) => ApiError::new(ApiStatusCode::NOT_IMPLEMENTED, msg.clone()),
        AppError::Validation(msg) => ApiError::new(ApiStatusCode::BAD_REQUEST, msg.clone()),
        AppError::Unauthorized(msg) => ApiError::new(ApiStatusCode::UNAUTHORIZED, msg.clone()),
        AppError::Forbidden(msg) => ApiError::new(ApiStatusCode::FORBIDDEN, msg.clone()),
        AppError::RateLimited(msg) => ApiError::new(ApiStatusCode::TOO_MANY_REQUESTS, msg.clone()),
        AppError::Path(path_err) => {
            // Path traversal is a client error (bad input), not a server failure.
            ApiError::new(ApiStatusCode::BAD_REQUEST, path_err.to_string())
        }
        AppError::Config(cfg_err) => {
            ApiError::new(ApiStatusCode::INTERNAL_SERVER_ERROR, cfg_err.to_string())
        }
        AppError::Internal(msg) => ApiError::new(ApiStatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
    }
}
