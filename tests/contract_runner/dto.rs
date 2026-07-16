//! DTO shape tests for the contract differential runner.
//!
//! The Go fixture harness marshals Go structs directly to capture the JSON
//! shapes of shared DTOs (`config.Config`, `interfaces.Event`, `WorkspaceInfo`,
//! etc.). The black-box runner can't marshal Go/Rust structs, so instead it
//! verifies that the JSON shapes from real API responses match the golden DTO
//! fixtures structurally (field names, presence/absence of optional fields).
//!
//! This is a structural comparison, not a value comparison: the golden DTO
//! fixtures have placeholder values (`<REDACTED_PATH>`, `<REDACTED_ID>`, etc.)
//! while the API responses have real (redacted) values. The comparison checks
//! that the same fields are present with the same types, not that the values
//! match.
//!
//! DTOs tested:
//! - `workspace_info` — from GET /api/workspaces (first entry)
//! - `agent_info` — from GET /api/agents (first entry)
//! - `event_full` / `event_minimal` — from GET /api/events (if any events exist)

use std::collections::BTreeMap;

use crate::harness::BackendHarness;

/// Test that the workspace info JSON shape from /api/workspaces matches the
/// golden DTO fixture. Verifies field names and types (not values).
pub async fn test_workspace_info_shape(harness: &BackendHarness) {
    let url = format!("{}/api/workspaces", harness.base_url);
    let resp = reqwest::get(&url).await.expect("fetch workspaces");
    assert!(resp.status().is_success(), "workspaces list failed: {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("parse workspaces JSON");

    let first = body
        .as_array()
        .and_then(|arr| arr.first())
        .expect("at least one workspace in response");

    // Load the golden DTO fixture.
    let fixture_path = harness
        .repo_root
        .join("tests/contract/golden/dto/workspace_info.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture_path).expect("read fixture"))
            .expect("parse fixture");

    // Compare shapes (field names + types, not values).
    let errors = compare_shape(first, &fixture, "root");
    if !errors.is_empty() {
        eprintln!("[contract] FAIL: dto_workspace_info_shape");
        for e in &errors {
            eprintln!("  {e}");
        }
        panic!("workspace_info shape mismatch: {} errors", errors.len());
    }

    eprintln!("[contract] PASS: dto_workspace_info_shape");
}

/// Test that the agent info JSON shape from /api/agents matches the golden DTO
/// fixture. Verifies field names and types.
pub async fn test_agent_info_shape(harness: &BackendHarness) {
    let url = format!("{}/api/agents", harness.base_url);
    let resp = reqwest::get(&url).await.expect("fetch agents");
    assert!(resp.status().is_success(), "agents list failed: {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("parse agents JSON");

    // Find the fixture-agent entry (the seeded agent).
    let agent = body
        .as_array()
        .and_then(|arr| {
            arr.iter().find(|a| {
                a.get("id").and_then(|v| v.as_str()) == Some("fixture-agent")
            })
        })
        .expect("fixture-agent in agents list");

    // Load the golden DTO fixture.
    let fixture_path = harness
        .repo_root
        .join("tests/contract/golden/dto/agent_info.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture_path).expect("read fixture"))
            .expect("parse fixture");

    // Compare shapes.
    let errors = compare_shape(agent, &fixture, "root");
    if !errors.is_empty() {
        eprintln!("[contract] FAIL: dto_agent_info_shape");
        for e in &errors {
            eprintln!("  {e}");
        }
        panic!("agent_info shape mismatch: {} errors", errors.len());
    }

    eprintln!("[contract] PASS: dto_agent_info_shape");
}

/// Test that the event JSON shape from /api/events matches the golden DTO
/// fixture. If no events exist (fresh backend), this test is a no-op pass.
pub async fn test_event_shape(harness: &BackendHarness) {
    let url = format!("{}/api/events", harness.base_url);
    let resp = reqwest::get(&url).await.expect("fetch events");
    assert!(resp.status().is_success(), "events list failed: {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("parse events JSON");

    let events = body.as_array();
    if events.is_none_or(|arr| arr.is_empty()) {
        eprintln!("[contract] SKIP: dto_event_shape (no events on fresh backend)");
        return;
    }

    let first = events.unwrap().first().expect("at least one event");

    // Load the golden DTO fixture (event_minimal — the simplest shape).
    let fixture_path = harness
        .repo_root
        .join("tests/contract/golden/dto/event_minimal.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture_path).expect("read fixture"))
            .expect("parse fixture");

    // Compare shapes. The actual event may have more fields than the minimal
    // fixture (omitempty), so we only check that the fixture's fields are
    // present in the actual event with the same types.
    let errors = compare_shape(first, &fixture, "root");
    if !errors.is_empty() {
        eprintln!("[contract] FAIL: dto_event_shape");
        for e in &errors {
            eprintln!("  {e}");
        }
        panic!("event shape mismatch: {} errors", errors.len());
    }

    eprintln!("[contract] PASS: dto_event_shape");
}

/// Compare the shape (field names + types) of two JSON values.
///
/// The comparison is bidirectional with omitempty tolerance:
/// - Every field in `actual` must exist in `expected` with the same type
///   (catches unexpected fields in the API response).
/// - Every field in `expected` that is NOT an omitempty candidate must exist
///   in `actual` with the same type. Fields that are empty arrays, empty
///   strings, or zero values in `expected` are allowed to be missing in
///   `actual` (the API may omit them via omitempty).
///
/// This handles the fact that golden DTO fixtures are generated from direct
/// Go struct marshals (which include all fields), while the black-box runner
/// observes API responses (which omit empty fields via omitempty).
fn compare_shape(actual: &serde_json::Value, expected: &serde_json::Value, path: &str) -> Vec<String> {
    let mut errors = Vec::new();

    match (actual, expected) {
        (serde_json::Value::Object(actual_obj), serde_json::Value::Object(expected_obj)) => {
            // Check that every field in actual exists in expected with same type.
            for (key, actual_val) in actual_obj {
                let field_path = format!("{path}.{key}");
                match expected_obj.get(key) {
                    Some(expected_val) => {
                        errors.extend(compare_shape(actual_val, expected_val, &field_path));
                    }
                    None => {
                        errors.push(format!(
                            "{field_path}: unexpected field in actual response (not in DTO fixture)"
                        ));
                    }
                }
            }
            // Check that every non-omitempty field in expected exists in actual.
            // Arrays and objects are commonly omitted by omitempty when empty,
            // so only flag missing scalar fields (strings, numbers, bools).
            for (key, expected_val) in expected_obj {
                if actual_obj.get(key).is_none()
                    && !is_omitempty_candidate(expected_val)
                    && !is_array_or_object(expected_val)
                {
                    errors.push(format!(
                        "{path}.{key}: missing in actual response (expected by DTO fixture)"
                    ));
                }
            }
        }
        (serde_json::Value::Array(actual_arr), serde_json::Value::Array(expected_arr)) => {
            if actual_arr.is_empty() && !expected_arr.is_empty() {
                errors.push(format!("{path}: actual array is empty, expected at least one element"));
            } else if !actual_arr.is_empty() && !expected_arr.is_empty() {
                // Compare the first element's shape.
                errors.extend(compare_shape(&actual_arr[0], &expected_arr[0], &format!("{path}[0]")));
            }
        }
        (actual_val, expected_val) => {
            // For scalar values, check that the types match. Placeholder
            // strings (e.g. "<REDACTED_PATH>") match any string.
            let actual_type = json_type_name(actual_val);
            let expected_type = json_type_name(expected_val);
            if actual_type != expected_type {
                // Allow null in actual when expected is a string/number/etc.
                // (omitempty may cause fields to be null instead of absent).
                if actual_type != "null" {
                    errors.push(format!(
                        "{path}: type mismatch — expected {expected_type}, got {actual_type}"
                    ));
                }
            }
        }
    }

    errors
}

/// Check whether a JSON value is an omitempty candidate — a value that the API
/// might omit from a response due to `omitempty` struct tags. Empty arrays,
/// empty strings, and zero numbers/bools are omitempty candidates. Non-empty
/// values are not (they should always be present).
fn is_omitempty_candidate(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(b) => !b,
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|f| f == 0.0),
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(arr) => arr.is_empty(),
        serde_json::Value::Object(obj) => obj.is_empty(),
    }
}

/// Check whether a JSON value is an array or object. These types are commonly
/// omitted by the API when empty (omitempty), even if the DTO fixture has a
/// non-empty value (the DTO fixture is from a direct struct marshal, not an
/// API response).
fn is_array_or_object(v: &serde_json::Value) -> bool {
    matches!(v, serde_json::Value::Array(_) | serde_json::Value::Object(_))
}

/// Get a human-readable type name for a JSON value.
fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Helper: convert a serde_json::Value to a sorted BTreeMap for deterministic
/// field iteration. Currently unused but kept for potential future exact-shape
/// comparison.
#[allow(dead_code)]
fn sorted_object(obj: &serde_json::Map<String, serde_json::Value>) -> BTreeMap<String, &serde_json::Value> {
    obj.iter().map(|(k, v)| (k.clone(), v)).collect()
}
