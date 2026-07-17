//! JSON comparison utilities for the contract differential runner.
//!
//! The Go fixture README defines two comparison modes:
//!
//! - **Semantic** (for JSON object/array bodies): parse both sides, compare
//!   structurally. Field order is irrelevant. This covers the bulk of
//!   list/detail responses.
//! - **Exact** (for contractually-significant text): byte-for-byte comparison.
//!   This covers error messages, markdown exports, and non-JSON content types.
//!
//! The REST envelope fields (`method`, `path`, `status`, `contentType`) are
//! always compared exactly.

use serde_json::{json, Value};

/// Compare two JSON strings semantically (parse both, compare structurally).
/// Returns Ok(()) if they are structurally equivalent, Err(message) otherwise.
pub fn json_semantic_eq(actual: &str, expected: &str) -> Result<(), String> {
    let actual_val: Value =
        serde_json::from_str(actual).map_err(|e| format!("actual is not valid JSON: {e}"))?;
    let expected_val: Value =
        serde_json::from_str(expected).map_err(|e| format!("expected is not valid JSON: {e}"))?;
    if values_equal(&actual_val, &expected_val) {
        Ok(())
    } else {
        Err(format!(
            "JSON mismatch:\n--- expected ---\n{}\n--- actual ---\n{}\n",
            serde_json::to_string_pretty(&expected_val).unwrap_or_default(),
            serde_json::to_string_pretty(&actual_val).unwrap_or_default(),
        ))
    }
}

/// Compare two JSON values structurally. Field order in objects is irrelevant.
/// Array order IS significant (the API defines ordered lists like event logs).
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(a_obj), Value::Object(b_obj)) => {
            if a_obj.len() != b_obj.len() {
                return false;
            }
            a_obj
                .iter()
                .all(|(k, v)| b_obj.get(k).is_some_and(|bv| values_equal(v, bv)))
        }
        (Value::Array(a_arr), Value::Array(b_arr)) => {
            a_arr.len() == b_arr.len() && a_arr.iter().zip(b_arr).all(|(x, y)| values_equal(x, y))
        }
        _ => a == b,
    }
}

/// Determine whether a response body should be compared semantically (JSON
/// object or array) or exactly (text). A body is semantic if:
/// - The content type is application/json, AND
/// - The body parses as a JSON object or array (not a scalar).
///
/// Scalar JSON values (strings, numbers, booleans) are compared exactly since
/// they represent contractually-significant text (error messages, etc.).
pub fn should_compare_semantically(content_type: &str, body: &str) -> bool {
    if !content_type.contains("application/json") {
        return false;
    }
    matches!(
        serde_json::from_str::<Value>(body),
        Ok(Value::Object(_) | Value::Array(_))
    )
}

/// Compare two strings exactly. Returns Ok(()) if equal, Err(diff) otherwise.
pub fn exact_eq(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        // Produce a simple line-by-line diff for readability.
        let diff = simple_diff(expected, actual);
        Err(format!("exact mismatch:\n{diff}"))
    }
}

/// A simple line-by-line diff for human-readable test failure output.
fn simple_diff(expected: &str, actual: &str) -> String {
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();
    let max_len = exp_lines.len().max(act_lines.len());

    let mut out = String::new();
    out.push_str("--- expected ---\n");
    for line in &exp_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("--- actual ---\n");
    for line in &act_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("--- differences ---\n");
    for i in 0..max_len {
        let exp = exp_lines.get(i).copied().unwrap_or("<missing>");
        let act = act_lines.get(i).copied().unwrap_or("<missing>");
        if exp != act {
            out.push_str(&format!(
                "line {}: expected={exp:?} actual={act:?}\n",
                i + 1
            ));
        }
    }
    out
}

/// Compare a REST response body using the appropriate mode (semantic or exact).
pub fn compare_body(actual: &str, expected: &str, content_type: &str) -> Result<(), String> {
    if should_compare_semantically(content_type, expected) {
        json_semantic_eq(actual, expected)
    } else {
        exact_eq(actual, expected)
    }
}

/// Compare a REST envelope (method, path, status, contentType) exactly.
pub fn compare_envelope(
    actual_method: &str,
    actual_path: &str,
    actual_status: u16,
    actual_content_type: &str,
    expected: &Value,
) -> Result<(), String> {
    let exp_method = expected
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let exp_path = expected.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let exp_status = expected.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
    let exp_ct = expected
        .get("contentType")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut errors = Vec::new();
    if actual_method != exp_method {
        errors.push(format!(
            "method: expected={exp_method:?} actual={actual_method:?}"
        ));
    }
    if actual_path != exp_path {
        errors.push(format!(
            "path: expected={exp_path:?} actual={actual_path:?}"
        ));
    }
    if actual_status as u64 != exp_status {
        errors.push(format!(
            "status: expected={exp_status} actual={actual_status}"
        ));
    }
    if actual_content_type != exp_ct {
        errors.push(format!(
            "contentType: expected={exp_ct:?} actual={actual_content_type:?}"
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("envelope mismatch:\n  {}\n", errors.join("\n  ")))
    }
}

/// Helper: extract the body field from a golden REST fixture JSON.
pub fn extract_body(fixture: &Value) -> &str {
    fixture.get("body").and_then(|v| v.as_str()).unwrap_or("")
}

/// Helper: build a restFixture-shaped JSON value for comparison.
#[allow(dead_code)]
pub fn make_fixture(
    method: &str,
    path: &str,
    status: u16,
    content_type: &str,
    body: &str,
) -> Value {
    json!({
        "method": method,
        "path": path,
        "status": status,
        "contentType": content_type,
        "body": body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_eq_same_order() {
        let a = r#"{"a":1,"b":2}"#;
        let b = r#"{"a":1,"b":2}"#;
        assert!(json_semantic_eq(a, b).is_ok());
    }

    #[test]
    fn test_semantic_eq_different_order() {
        let a = r#"{"a":1,"b":2}"#;
        let b = r#"{"b":2,"a":1}"#;
        assert!(json_semantic_eq(a, b).is_ok());
    }

    #[test]
    fn test_semantic_neq_different_values() {
        let a = r#"{"a":1,"b":2}"#;
        let b = r#"{"a":1,"b":3}"#;
        assert!(json_semantic_eq(a, b).is_err());
    }

    #[test]
    fn test_semantic_array_order_matters() {
        let a = r#"[1,2,3]"#;
        let b = r#"[3,2,1]"#;
        assert!(json_semantic_eq(a, b).is_err());
    }

    #[test]
    fn test_should_compare_semantically_json_object() {
        assert!(should_compare_semantically(
            "application/json",
            r#"{"a":1}"#
        ));
    }

    #[test]
    fn test_should_compare_semantically_json_array() {
        assert!(should_compare_semantically("application/json", r#"[1,2]"#));
    }

    #[test]
    fn test_should_compare_semantically_json_scalar() {
        assert!(!should_compare_semantically(
            "application/json",
            r#""hello""#
        ));
    }

    #[test]
    fn test_should_compare_semantically_non_json() {
        assert!(!should_compare_semantically("text/plain", "hello"));
    }

    #[test]
    fn test_exact_eq() {
        assert!(exact_eq("hello", "hello").is_ok());
        assert!(exact_eq("hello", "world").is_err());
    }
}
