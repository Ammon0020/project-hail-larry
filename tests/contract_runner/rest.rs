//! REST differential tests.
//!
//! Each test case mirrors a case from the Go fixture harness
//! (`tests/contract/go-fixtures/rest.go` → `buildRESTCases`). The runner:
//!
//! 1. Reads the golden fixture (`golden/rest/<name>.json`) to get the expected
//!    response envelope (method, path, status, contentType, body).
//! 2. Substitutes `<REDACTED_WORKSPACE_ID>` in the path with the real workspace
//!    ID discovered at runtime.
//! 3. Makes the HTTP request to the running backend.
//! 4. Redacts the response (same redaction rules as the Go harness).
//! 5. Compares the redacted response against the golden fixture:
//!    - Envelope fields (method, path, status, contentType): exact.
//!    - Body: semantic JSON for objects/arrays, exact bytes for text/errors.
//!
//! Cases that require non-loopback connections (the `_unauth` cases) are
//! skipped — the black-box runner connects via localhost, so the server's
//! loopback auth bypass always applies. These cases are documented as a known
//! limitation in the README.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Method;

use crate::compare;
use crate::harness::BackendHarness;
use crate::redactor::Redactor;

/// A single REST test case. Mirrors `restCase` in go-fixtures/rest.go.
struct RestCase {
    /// The golden fixture name (without extension).
    name: &'static str,
    /// The HTTP method.
    method: &'static str,
    /// The request path (may include query string and <REDACTED_WORKSPACE_ID>).
    path: &'static str,
    /// The request body for POST/PUT/PATCH, or empty for GET/DELETE.
    body: &'static str,
}

/// The full list of REST cases. This mirrors `buildRESTCases` in
/// go-fixtures/rest.go, excluding the `_unauth` cases (which require
/// non-loopback connections and can't be tested black-box).
const REST_CASES: &[RestCase] = &[
    // Health
    RestCase { name: "health_ok", method: "GET", path: "/health", body: "" },
    // Pairing
    RestCase { name: "pair_initiate_ok", method: "POST", path: "/api/pair/initiate", body: r#"{"host":"localhost","port":7337}"# },
    RestCase { name: "pair_initiate_bad_body", method: "POST", path: "/api/pair/initiate", body: r#"{not json"# },
    RestCase { name: "pair_verify_passcode_bad_body", method: "POST", path: "/api/pair/verify-passcode", body: r#"{not json"# },
    RestCase { name: "pair_verify_passcode_wrong", method: "POST", path: "/api/pair/verify-passcode", body: r#"{"passcode":"wrong wrong wrong wrong","deviceName":"dev"}"# },
    RestCase { name: "pair_verify_token_wrong", method: "POST", path: "/api/pair/verify-token", body: r#"{"token":"deadbeef","deviceName":"dev"}"# },
    // Devices
    RestCase { name: "devices_list_ok", method: "GET", path: "/api/devices", body: "" },
    RestCase { name: "devices_revoke_not_found", method: "DELETE", path: "/api/devices/nonexistent", body: "" },
    RestCase { name: "devices_cancel_revocation_bad_body", method: "POST", path: "/api/devices/cancel-revocation", body: r#"{not json"# },
    // Pending actions
    RestCase { name: "pending_actions_list_ok", method: "GET", path: "/api/pending-actions", body: "" },
    RestCase { name: "workspaces_cancel_registration_bad_body", method: "POST", path: "/api/workspaces/cancel-registration", body: r#"{not json"# },
    // Workspaces
    RestCase { name: "workspaces_list_ok", method: "GET", path: "/api/workspaces", body: "" },
    RestCase { name: "workspaces_register_remote_disabled", method: "POST", path: "/api/workspaces", body: r#"{"path":"/tmp"}"# },
    RestCase { name: "workspaces_register_bad_body", method: "POST", path: "/api/workspaces", body: r#"{not json"# },
    RestCase { name: "workspaces_files_ok", method: "GET", path: "/api/workspaces/<REDACTED_WORKSPACE_ID>/files", body: "" },
    RestCase { name: "workspaces_files_not_found", method: "GET", path: "/api/workspaces/nonexistent/files", body: "" },
    RestCase { name: "workspaces_read_ok", method: "GET", path: "/api/workspaces/<REDACTED_WORKSPACE_ID>/file?path=README.md", body: "" },
    RestCase { name: "workspaces_read_missing_path", method: "GET", path: "/api/workspaces/<REDACTED_WORKSPACE_ID>/file", body: "" },
    RestCase { name: "workspaces_read_not_found", method: "GET", path: "/api/workspaces/<REDACTED_WORKSPACE_ID>/file?path=nope.txt", body: "" },
    RestCase { name: "workspaces_raw_ok", method: "GET", path: "/api/workspaces/<REDACTED_WORKSPACE_ID>/raw?path=README.md", body: "" },
    RestCase { name: "workspaces_search_ok", method: "GET", path: "/api/workspaces/<REDACTED_WORKSPACE_ID>/search?pattern=hello", body: "" },
    RestCase { name: "workspaces_write_bad_body", method: "POST", path: "/api/workspaces/<REDACTED_WORKSPACE_ID>/file", body: r#"{not json"# },
    // Events
    RestCase { name: "events_list_ok", method: "GET", path: "/api/events", body: "" },
    RestCase { name: "events_session_ok", method: "GET", path: "/api/events/nonexistent", body: "" },
    // Agents
    RestCase { name: "agents_list_ok", method: "GET", path: "/api/agents", body: "" },
    RestCase { name: "agents_upsert_bad_body", method: "POST", path: "/api/agents", body: r#"{not json"# },
    RestCase { name: "agents_delete_ok", method: "DELETE", path: "/api/agents/fixture-agent", body: "" },
    RestCase { name: "agents_autodetect_ok", method: "POST", path: "/api/agents/autodetect", body: "" },
    // Sessions
    RestCase { name: "sessions_list_ok", method: "GET", path: "/api/sessions", body: "" },
    RestCase { name: "sessions_get_not_found", method: "GET", path: "/api/sessions/nonexistent", body: "" },
    RestCase { name: "sessions_export_not_found", method: "GET", path: "/api/sessions/nonexistent/export", body: "" },
    RestCase { name: "sessions_create_bad_body", method: "POST", path: "/api/sessions", body: r#"{not json"# },
    RestCase { name: "sessions_create_unknown_agent", method: "POST", path: "/api/sessions", body: r#"{"agentId":"no-such-agent","modelId":"m","workspaceId":""}"# },
    RestCase { name: "sessions_patch_bad_body", method: "PATCH", path: "/api/sessions/nonexistent", body: r#"{not json"# },
    RestCase { name: "sessions_prompt_not_found", method: "POST", path: "/api/sessions/nonexistent/prompt", body: r#"{"content":"hi"}"# },
    RestCase { name: "sessions_cancel_not_found", method: "POST", path: "/api/sessions/nonexistent/cancel", body: "" },
    RestCase { name: "sessions_close_not_found", method: "DELETE", path: "/api/sessions/nonexistent", body: "" },
    RestCase { name: "sessions_context_bad_body", method: "POST", path: "/api/sessions/nonexistent/context", body: r#"{not json"# },
    RestCase { name: "sessions_providers_not_found", method: "GET", path: "/api/sessions/nonexistent/providers", body: "" },
    // Permissions
    RestCase { name: "permissions_pending_ok", method: "GET", path: "/api/permissions/pending", body: "" },
    RestCase { name: "permissions_respond_bad_body", method: "POST", path: "/api/permissions/nonexistent/respond", body: r#"{not json"# },
    // MCP
    RestCase { name: "mcp_get_ok", method: "GET", path: "/api/mcp", body: "" },
    RestCase { name: "mcp_put_bad_body", method: "PUT", path: "/api/mcp", body: r#"{not json"# },
    RestCase { name: "mcp_patch_server_bad_body", method: "PATCH", path: "/api/mcp/servers/fixture", body: r#"{not json"# },
    RestCase { name: "mcp_status_ok", method: "GET", path: "/api/mcp/status", body: "" },
];

/// Find a REST case by name from the static list.
fn find_case(name: &str) -> Option<&'static RestCase> {
    REST_CASES.iter().find(|c| c.name == name)
}

/// Resolve the golden fixture directory path.
fn golden_dir(harness: &BackendHarness) -> PathBuf {
    harness.repo_root.join("tests").join("contract").join("golden").join("rest")
}

/// Run a single REST test case by name. Reads the golden fixture, makes the
/// HTTP request, redacts the response, and compares.
pub async fn run_case(harness: &BackendHarness, name: &str) {
    let case = find_case(name).unwrap_or_else(|| panic!("unknown REST case: {name}"));

    // Load the golden fixture.
    let fixture_path = golden_dir(harness).join(format!("{name}.json"));
    let fixture_text = std::fs::read_to_string(&fixture_path)
        .with_context(|| format!("read golden fixture {}", fixture_path.display()))
        .expect("read golden fixture");
    let fixture: serde_json::Value =
        serde_json::from_str(&fixture_text).expect("parse golden fixture JSON");

    // Discover the real workspace ID for path substitution.
    let ws_id = harness.workspace_id().await;

    // Build the redactor with the run's known secrets and paths.
    let mut redactor = Redactor::new();
    redactor.register_path(&harness.state_dir);
    if let Ok(home) = std::env::var("HOME") {
        redactor.register_path(&home);
    }
    redactor.register_path(&harness.repo_root);
    redactor.register_secret(&ws_id, crate::redactor::REDACTED_WORKSPACE_ID);
    // Register the ephemeral port so it is redacted in responses.
    redactor.register_secret(
        &format!("127.0.0.1:{}", harness.port),
        "127.0.0.1:<REDACTED_PORT>",
    );
    redactor.register_secret(
        &format!("localhost:{}", harness.port),
        "localhost:<REDACTED_PORT>",
    );

    // Substitute <REDACTED_WORKSPACE_ID> in the path with the real workspace ID.
    let actual_path = case.path.replace("<REDACTED_WORKSPACE_ID>", &ws_id);

    // Make the HTTP request.
    let method = Method::from_bytes(case.method.as_bytes()).expect("valid HTTP method");
    let url = format!("{}{}", harness.base_url, actual_path);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest client");

    let mut req = client.request(method, &url);
    if !case.body.is_empty() {
        req = req.body(case.body.to_string())
            .header("Content-Type", "application/json");
    }

    let resp = req.send().await.with_context(|| format!("send request {case.method} {url}"))
        .expect("send HTTP request");

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = resp.text().await.expect("read response body");

    // Redact the response body.
    let redacted_body = redactor.redact(&body);

    // Redact the actual path (it contains the real workspace ID which should
    // be replaced with <REDACTED_WORKSPACE_ID> for comparison).
    let redacted_path = redactor.redact(&actual_path);

    // Compare the envelope (method, path, status, contentType).
    if let Err(e) = compare::compare_envelope(
        case.method,
        &redacted_path,
        status,
        &content_type,
        &fixture,
    ) {
        eprintln!("[contract] FAIL: {name}\n{e}");
        panic!("REST case {name} failed: {e}");
    }

    // Compare the body.
    let expected_body = compare::extract_body(&fixture);
    if let Err(e) = compare::compare_body(&redacted_body, expected_body, &content_type) {
        eprintln!("[contract] FAIL: {name}\n{e}");
        panic!("REST case {name} body mismatch: {e}");
    }

    eprintln!("[contract] PASS: {name} ({case.method} {actual_path} → {status})");
}
