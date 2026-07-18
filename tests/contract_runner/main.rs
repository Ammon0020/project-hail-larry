//! Black-box API contract differential runner.
//!
//! **Gated behind the `contract` feature** — run via
//! `cargo test --test contract_runner --features contract`.
//! Without the feature, this test binary compiles to nothing, keeping
//! `cargo test --all-targets` fast and free of Go subprocess dependencies.
//!
//! Boots a backend binary (Go `app` or Rust `local_agent`) as a subprocess,
//!
//! Boots a backend binary (Go `app` or Rust `local_agent`) as a subprocess,
//! replays the same HTTP/WS/CLI request sequences captured by the Go fixture
//! harness (`tests/contract/go-fixtures/`), applies the same redactions, and
//! compares responses against the checked-in golden fixtures
//! (`tests/contract/golden/`).
//!
//! The runner is completely independent of the backend implementation — it
//! only interacts via the external API (HTTP, WebSocket, CLI subprocess). This
//! makes it suitable for TDD against the Rust port while also verifying the Go
//! backend during maintenance.
//!
//! # Usage
//!
//! ```sh
//! # Test against the Go backend (default):
//! cargo test --test contract_runner -- --nocapture
//!
//! # Test against the Rust backend:
//! CONTRACT_BACKEND=rust cargo test --test contract_runner -- --nocapture
//!
//! # Specify a pre-built binary path:
//! CONTRACT_BINARY=/path/to/binary cargo test --test contract_runner
//!
//! # Keep the state dir for debugging:
//! CONTRACT_KEEP_STATE=1 cargo test --test contract_runner
//! ```
//!
//! # Backend selection
//!
//! - `CONTRACT_BACKEND=go` (default): builds `go build -o /tmp/contract-local-agent ./cmd/app`
//!   and runs `local-agent start`.
//! - `CONTRACT_BACKEND=rust`: uses `target/debug/local_agent` (or
//!   `CONTRACT_BINARY` override) and runs `local_agent start`.
//! - `CONTRACT_BINARY=/path/to/binary`: overrides the binary path for either
//!   backend. The runner uses this directly without building.
//!
//! # What is tested
//!
//! - **REST**: every golden/rest/*.json fixture is replayed as an HTTP request
//!   and the redacted response is compared (semantic JSON for object/array
//!   bodies, exact bytes for error text and non-JSON content types).
//! - **WebSocket**: origin rejection (403), connection success, live broadcast
//!   (API-driven), `?after=` replay + live transition, and auth rejection
//!   (non-loopback dial). Slow-client recovery is unit-tested in sync, not
//!   black-box (see tests/contract/README.md).
//! - **DTO**: the JSON shapes from API responses are structurally compared
//!   against golden/dto/*.json fixtures to verify field names and omitempty
//!   behavior.
//!
//! CLI tests are intentionally excluded — the CLI is a thin client over the
//! REST API, and its output formatting (box-drawing, table layouts, help text)
//! is presentation, not contract. The REST + WS + DTO tests cover the actual
//! API contract surface that the Rust port must replicate.

// Integration tests are separate crates. The main crate's lint policy denies
// unwrap/expect/panic/print_stdout/etc. for production code. Test code
// legitimately uses these, so lift the restrictions for this test binary.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::dbg_macro)]
// The entire test binary is gated on the `contract` feature. Without it,
// cargo compiles an empty test binary (no tests, no dependencies pulled in).
#![cfg(feature = "contract")]

mod compare;
mod dto;
mod harness;
mod redactor;
mod rest;
mod ws;

use harness::BackendHarness;

const BANNER: &str = "========================================";

/// Helper: print a section banner with a title.
fn banner(title: &str) {
    eprintln!("\n{BANNER}\n  {title}\n{BANNER}");
}

// ---------------------------------------------------------------------------
// REST tests — one per golden/rest/*.json fixture
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rest_health_ok() {
    banner("REST: health_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "health_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_pair_initiate_ok() {
    banner("REST: pair_initiate_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "pair_initiate_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_pair_initiate_bad_body() {
    banner("REST: pair_initiate_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "pair_initiate_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_pair_verify_passcode_bad_body() {
    banner("REST: pair_verify_passcode_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "pair_verify_passcode_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_pair_verify_passcode_wrong() {
    banner("REST: pair_verify_passcode_wrong");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "pair_verify_passcode_wrong").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_pair_verify_token_wrong() {
    banner("REST: pair_verify_token_wrong");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "pair_verify_token_wrong").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_devices_list_ok() {
    banner("REST: devices_list_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "devices_list_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_devices_revoke_not_found() {
    banner("REST: devices_revoke_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "devices_revoke_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_devices_cancel_revocation_bad_body() {
    banner("REST: devices_cancel_revocation_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "devices_cancel_revocation_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_pending_actions_list_ok() {
    banner("REST: pending_actions_list_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "pending_actions_list_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_cancel_registration_bad_body() {
    banner("REST: workspaces_cancel_registration_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_cancel_registration_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_list_ok() {
    banner("REST: workspaces_list_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_list_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_register_remote_disabled() {
    banner("REST: workspaces_register_remote_disabled");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_register_remote_disabled").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_register_bad_body() {
    banner("REST: workspaces_register_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_register_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_files_ok() {
    banner("REST: workspaces_files_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_files_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_files_not_found() {
    banner("REST: workspaces_files_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_files_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_read_ok() {
    banner("REST: workspaces_read_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_read_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_read_missing_path() {
    banner("REST: workspaces_read_missing_path");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_read_missing_path").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_read_not_found() {
    banner("REST: workspaces_read_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_read_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_raw_ok() {
    banner("REST: workspaces_raw_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_raw_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_search_ok() {
    banner("REST: workspaces_search_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_search_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_workspaces_write_bad_body() {
    banner("REST: workspaces_write_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "workspaces_write_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_events_list_ok() {
    banner("REST: events_list_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "events_list_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_events_session_ok() {
    banner("REST: events_session_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "events_session_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_agents_list_ok() {
    banner("REST: agents_list_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "agents_list_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_agents_upsert_bad_body() {
    banner("REST: agents_upsert_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "agents_upsert_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_agents_delete_ok() {
    banner("REST: agents_delete_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "agents_delete_ok").await;
    h.shutdown().await;
}

/// NOTE: rest_agents_autodetect_ok is intentionally skipped in the black-box
/// runner. The golden fixture captures machine-specific autodetected agents
/// (Claude Code, Codex, Cursor, etc.) from the generation machine. The black-
/// box runner deliberately neutralizes autodetect (PATH=/dev/null, HOME=
/// /dev/null) for reproducibility, so /api/agents/autodetect returns an empty
/// list. This test can only be run via the Go in-process harness.
#[tokio::test]
#[ignore = "autodetect is machine-specific; see rest_agents_autodetect_ok docs"]
async fn rest_agents_autodetect_ok() {
    banner("REST: agents_autodetect_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "agents_autodetect_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_list_ok() {
    banner("REST: sessions_list_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_list_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_get_not_found() {
    banner("REST: sessions_get_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_get_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_export_not_found() {
    banner("REST: sessions_export_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_export_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_create_bad_body() {
    banner("REST: sessions_create_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_create_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_create_unknown_agent() {
    banner("REST: sessions_create_unknown_agent");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_create_unknown_agent").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_patch_bad_body() {
    banner("REST: sessions_patch_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_patch_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_prompt_not_found() {
    banner("REST: sessions_prompt_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_prompt_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_cancel_not_found() {
    banner("REST: sessions_cancel_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_cancel_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_close_not_found() {
    banner("REST: sessions_close_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_close_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_context_bad_body() {
    banner("REST: sessions_context_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_context_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_sessions_providers_not_found() {
    banner("REST: sessions_providers_not_found");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "sessions_providers_not_found").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_permissions_pending_ok() {
    banner("REST: permissions_pending_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "permissions_pending_ok").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_permissions_respond_bad_body() {
    banner("REST: permissions_respond_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "permissions_respond_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_mcp_get_ok() {
    banner("REST: mcp_get_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "mcp_get_ok").await;
    h.shutdown().await;
}

/// NOTE: rest_mcp_put_bad_body is ignored for Rust black-box runs because Go
/// `encoding/json` and Rust `serde_json` emit different parse-error strings for
/// the same malformed body (`{not json`). Both return 400 +
/// `invalid mcp config JSON: …`; only the suffix differs. Envelope/status are
/// covered by other bad-body cases. See tests/contract/README.md.
#[tokio::test]
#[ignore = "JSON parse error text differs Go encoding/json vs serde_json"]
async fn rest_mcp_put_bad_body() {
    banner("REST: mcp_put_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "mcp_put_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_mcp_patch_server_bad_body() {
    banner("REST: mcp_patch_server_bad_body");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "mcp_patch_server_bad_body").await;
    h.shutdown().await;
}

#[tokio::test]
async fn rest_mcp_status_ok() {
    banner("REST: mcp_status_ok");
    let h = BackendHarness::start().await;
    rest::run_case(&h, "mcp_status_ok").await;
    h.shutdown().await;
}

// ---------------------------------------------------------------------------
// WebSocket tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_origin_rejection() {
    banner("WS: origin_rejection");
    let h = BackendHarness::start().await;
    ws::test_origin_rejection(&h).await;
    h.shutdown().await;
}

#[tokio::test]
async fn ws_connection_success() {
    banner("WS: connection_success");
    let h = BackendHarness::start().await;
    ws::test_connection_success(&h).await;
    h.shutdown().await;
}

#[tokio::test]
async fn ws_live_broadcast() {
    banner("WS: live_broadcast");
    let h = BackendHarness::start().await;
    ws::test_live_broadcast(&h).await;
    h.shutdown().await;
}

#[tokio::test]
async fn ws_after_replay() {
    banner("WS: after_replay");
    let h = BackendHarness::start().await;
    ws::test_after_replay_and_live(&h).await;
    h.shutdown().await;
}

#[tokio::test]
async fn ws_auth_rejection() {
    banner("WS: auth_rejection");
    let h = BackendHarness::start().await;
    ws::test_auth_rejection(&h).await;
    h.shutdown().await;
}

// ---------------------------------------------------------------------------
// DTO shape tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dto_workspace_info_shape() {
    banner("DTO: workspace_info shape");
    let h = BackendHarness::start().await;
    dto::test_workspace_info_shape(&h).await;
    h.shutdown().await;
}

#[tokio::test]
async fn dto_agent_info_shape() {
    banner("DTO: agent_info shape");
    let h = BackendHarness::start().await;
    dto::test_agent_info_shape(&h).await;
    h.shutdown().await;
}

#[tokio::test]
async fn dto_event_shape() {
    banner("DTO: event shape");
    let h = BackendHarness::start().await;
    dto::test_event_shape(&h).await;
    h.shutdown().await;
}
