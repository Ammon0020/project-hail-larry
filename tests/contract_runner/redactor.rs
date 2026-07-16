//! Redaction logic for the contract differential runner.
//!
//! Ports the Go redactor (`tests/contract/go-fixtures/redact.go`) so the
//! runner applies the SAME redactions to its own output before comparing
//! against the golden fixtures. Redaction is comparison-neutral: both the Go
//! fixture generator and this runner redact with the same rules, so the
//! comparison is stable.
//!
//! Redaction order (matching the Go implementation):
//! 1. Registered secrets (pairing tokens, passcodes, device IDs, workspace IDs)
//! 2. Registered absolute path prefixes (longest first)
//! 3. Non-deterministic timestamps (ISO-8601 → <REDACTED_TIMESTAMP>)
//! 4. Long hex/base64 IDs (≥20 chars → <REDACTED_ID>)
//! 5. PIDs ("PID <number>" → "PID <REDACTED_PID>")
//! 6. Ephemeral ports ("Port:      <number>" → "Port:      <REDACTED_PORT>")
//! 7. CLI passcodes ("Passcode: <word>-<word>-<word>-<word>" → <REDACTED_PASSCODE>)

use regex::Regex;
use std::collections::HashMap;

/// Stable placeholder strings. These must match the Go redactor exactly.
pub const REDACTED_PATH: &str = "<REDACTED_PATH>";
pub const REDACTED_TIMESTAMP: &str = "<REDACTED_TIMESTAMP>";
pub const REDACTED_ID: &str = "<REDACTED_ID>";
pub const REDACTED_TOKEN: &str = "<REDACTED_TOKEN>";
pub const REDACTED_PASSCODE: &str = "<REDACTED_PASSCODE>";
pub const REDACTED_PID: &str = "<REDACTED_PID>";
pub const REDACTED_PORT: &str = "<REDACTED_PORT>";
pub const REDACTED_WORKSPACE_ID: &str = "<REDACTED_WORKSPACE_ID>";
pub const REDACTED_DEVICE_ID: &str = "<REDACTED_DEVICE_ID>";

/// Redactor scrubs secrets and absolute paths out of fixture text.
pub struct Redactor {
    /// Maps a raw secret string to the placeholder that replaces it.
    secrets: HashMap<String, String>,
    /// Absolute path prefixes to replace with <REDACTED_PATH>, longest first.
    paths: Vec<String>,
    /// Pre-compiled regex patterns for non-deterministic values.
    timestamp_re: Regex,
    hex_id_re: Regex,
    pid_re: Regex,
    port_re: Regex,
    passcode_re: Regex,
    hex_token_re: Regex,
}

impl Redactor {
    /// Create an empty redactor with the standard regex patterns pre-compiled.
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
            paths: Vec::new(),
            timestamp_re: Regex::new(
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})",
            )
            .expect("valid timestamp regex"),
            hex_id_re: Regex::new(r"\b[A-Fa-f0-9]{20,}\b").expect("valid hex ID regex"),
            pid_re: Regex::new(r"PID\s+\d+").expect("valid PID regex"),
            port_re: Regex::new(r"Port:\s+\d+").expect("valid port regex"),
            passcode_re: Regex::new(r"Passcode:\s+[a-z]+-[a-z]+-[a-z]+-[a-z]+")
                .expect("valid passcode regex"),
            hex_token_re: Regex::new(r#""(token|secret|secretHash)"\s*:\s*"([A-Za-z0-9_\-]{16,})""#)
                .expect("valid hex token regex"),
        }
    }

    /// Register a raw secret value and the placeholder to substitute for it.
    pub fn register_secret(&mut self, raw: &str, placeholder: &str) {
        if !raw.is_empty() {
            self.secrets.insert(raw.to_string(), placeholder.to_string());
        }
    }

    /// Register an absolute path prefix to replace with <REDACTED_PATH>.
    /// Paths are matched longest-first so nested directories are scrubbed
    /// before their parents.
    pub fn register_path(&mut self, prefix: &str) {
        if prefix.is_empty() {
            return;
        }
        self.paths.push(prefix.to_string());
        // Sort longest-first so the most specific prefix wins during replacement.
        self.paths.sort_by(|a, b| b.len().cmp(&a.len()));
    }

    /// Redact a string: apply secrets, paths, and regex patterns in order.
    pub fn redact(&self, s: &str) -> String {
        let mut result = s.to_string();

        // 1. Registered secrets.
        for (raw, placeholder) in &self.secrets {
            result = result.replace(raw, placeholder);
        }

        // 2. Registered path prefixes (longest first).
        for prefix in &self.paths {
            result = result.replace(prefix, REDACTED_PATH);
        }

        // 3. Timestamps.
        result = self.timestamp_re.replace_all(&result, REDACTED_TIMESTAMP).to_string();

        // 4. Long hex IDs.
        result = self.hex_id_re.replace_all(&result, REDACTED_ID).to_string();

        // 5. PIDs.
        result = self.pid_re.replace_all(&result, "PID <REDACTED_PID>").to_string();

        // 6. Ports.
        result = self.port_re.replace_all(&result, "Port:      <REDACTED_PORT>").to_string();

        // 7. CLI passcodes.
        result = self.passcode_re.replace_all(&result, "Passcode: <REDACTED_PASSCODE>").to_string();

        // 8. Defense-in-depth: scrub unregistered token/secret/secretHash fields.
        result = self.scrub_unregistered_tokens(&result);

        result
    }

    /// Scrub any remaining token/secret/secretHash JSON fields that were not
    /// explicitly registered. Mirrors `ScrubUnregisteredTokens` in the Go code.
    fn scrub_unregistered_tokens(&self, s: &str) -> String {
        self.hex_token_re
            .replace_all(s, r#""$1":"<REDACTED_TOKEN>""#)
            .to_string()
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_redaction() {
        let r = Redactor::new();
        let input = r#"{"createdAt":"2026-07-16T04:49:42.091338754Z"}"#;
        let output = r.redact(input);
        assert!(output.contains(REDACTED_TIMESTAMP));
        assert!(!output.contains("2026-07-16"));
    }

    #[test]
    fn test_hex_id_redaction() {
        let r = Redactor::new();
        let input = r#"{"id":"aabbccddeeff00112233445566778899"}"#;
        let output = r.redact(input);
        assert!(output.contains(REDACTED_ID));
    }

    #[test]
    fn test_short_id_preserved() {
        let r = Redactor::new();
        // 16-char workspace ID should NOT be redacted by hex_id_re (threshold is 20).
        let input = r#"{"id":"66c9bb27f0c7a337"}"#;
        let output = r.redact(input);
        assert!(output.contains("66c9bb27f0c7a337"));
    }

    #[test]
    fn test_secret_redaction() {
        let mut r = Redactor::new();
        r.register_secret("my-secret-token", REDACTED_TOKEN);
        let input = r#"{"token":"my-secret-token"}"#;
        let output = r.redact(input);
        assert!(output.contains(REDACTED_TOKEN));
        assert!(!output.contains("my-secret-token"));
    }

    #[test]
    fn test_path_redaction() {
        let mut r = Redactor::new();
        r.register_path("/home/user/.local-agent");
        r.register_path("/home/user");
        let input = "path: /home/user/.local-agent/config.json";
        let output = r.redact(input);
        // The longer prefix should win, so "/home/user/.local-agent" is replaced
        // first, then the remaining "/home/user" (if any) is replaced.
        assert!(output.contains(REDACTED_PATH));
        assert!(!output.contains("/home/user"));
    }

    #[test]
    fn test_pid_redaction() {
        let r = Redactor::new();
        let input = "Running (PID 800296)";
        let output = r.redact(input);
        assert!(output.contains("PID <REDACTED_PID>"));
        assert!(!output.contains("800296"));
    }

    #[test]
    fn test_port_redaction() {
        let r = Redactor::new();
        let input = "Port:      45869";
        let output = r.redact(input);
        assert!(output.contains("Port:      <REDACTED_PORT>"));
        assert!(!output.contains("45869"));
    }

    #[test]
    fn test_passcode_redaction() {
        let r = Redactor::new();
        let input = "Passcode: eye-detect-stomach-firm";
        let output = r.redact(input);
        assert!(output.contains("Passcode: <REDACTED_PASSCODE>"));
        assert!(!output.contains("eye-detect-stomach-firm"));
    }

    #[test]
    fn test_unregistered_token_scrub() {
        let r = Redactor::new();
        let input = r#"{"token":"abcdefghijklmnop"}"#;
        let output = r.redact(input);
        assert!(output.contains(REDACTED_TOKEN));
    }
}
