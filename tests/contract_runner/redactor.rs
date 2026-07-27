//! Redaction logic for the contract differential runner.
//!
//! Ports the redaction rules from the original Go fixture harness (removed at
//! the Rust cutover) so the runner applies the SAME redactions to its own
//! output before comparing against the golden fixtures. Redaction is
//! comparison-neutral: the golden fixtures were generated with these same
//! rules, so the comparison is stable.
//!
//! Redaction order (matching the original Go implementation):
//! 1. Registered secrets (pairing tokens, passcodes, device IDs, workspace IDs)
//! 2. Registered absolute path prefixes (longest first)
//! 3. Non-deterministic timestamps (ISO-8601 → <REDACTED_TIMESTAMP>)
//! 4. Long hex/base64 IDs (≥20 chars → <REDACTED_ID>)
//! 5. PIDs ("PID <number>" → "PID <REDACTED_PID>")
//! 6. Ephemeral ports ("Port:      <number>" → "Port:      <REDACTED_PORT>")
//! 7. CLI passcodes ("Passcode: <word>-<word>-<word>-<word>" → <REDACTED_PASSCODE>)

use regex::Regex;
use std::collections::HashMap;

/// Stable placeholder strings. These must match the original Go redactor exactly.
/// Some are not referenced directly (they're produced by regex replacement)
/// but are exported for documentation and potential future use.
#[allow(dead_code)]
pub const REDACTED_PATH: &str = "<REDACTED_PATH>";
#[allow(dead_code)]
pub const REDACTED_TIMESTAMP: &str = "<REDACTED_TIMESTAMP>";
#[allow(dead_code)]
pub const REDACTED_ID: &str = "<REDACTED_ID>";
pub const REDACTED_TOKEN: &str = "<REDACTED_TOKEN>";
pub const REDACTED_PASSCODE: &str = "<REDACTED_PASSCODE>";
#[allow(dead_code)]
pub const REDACTED_PID: &str = "<REDACTED_PID>";
#[allow(dead_code)]
pub const REDACTED_PORT: &str = "<REDACTED_PORT>";
pub const REDACTED_WORKSPACE_ID: &str = "<REDACTED_WORKSPACE_ID>";
#[allow(dead_code)]
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
            hex_token_re: Regex::new(
                r#""(token|secret|secretHash)"\s*:\s*"([A-Za-z0-9_\-]{16,})""#,
            )
            .expect("valid hex token regex"),
        }
    }

    /// Register a raw secret value and the placeholder to substitute for it.
    pub fn register_secret(&mut self, raw: &str, placeholder: &str) {
        if !raw.is_empty() {
            self.secrets
                .insert(raw.to_string(), placeholder.to_string());
        }
    }

    /// Register an absolute path prefix to replace with <REDACTED_PATH>.
    /// Paths are matched longest-first so nested directories are scrubbed
    /// before their parents.
    ///
    /// On Windows, path prefixes contain backslashes. JSON response bodies
    /// escape backslashes as `\\`, so a raw prefix `C:\Users\...` would not
    /// match the JSON-escaped `C:\\Users\\...` in the response. To handle
    /// both, the JSON-escaped variant is also registered. Additionally, a
    /// forward-slash variant is registered so paths normalized to `/` by the
    /// daemon (e.g. workspace paths via `path_to_slash`) are also redacted.
    pub fn register_path(&mut self, prefix: &str) {
        if prefix.is_empty() {
            return;
        }
        self.paths.push(prefix.to_string());
        // JSON-escaped variant (backslashes doubled) for paths inside JSON
        // string values on Windows.
        let escaped = prefix.replace('\\', "\\\\");
        // Forward-slash variant for daemon-normalized paths (path_to_slash).
        let slashed = prefix.replace('\\', "/");
        let has_distinct_slashed_variant = slashed != prefix && slashed != escaped;
        if escaped != prefix {
            self.paths.push(escaped);
        }
        if has_distinct_slashed_variant {
            self.paths.push(slashed);
        }
        // Sort longest-first so the most specific prefix wins during replacement.
        self.paths.sort_by_key(|b| std::cmp::Reverse(b.len()));
    }

    /// Redact a string: apply secrets, paths, and regex patterns in order.
    pub fn redact(&self, s: &str) -> String {
        let mut result = s.to_string();

        // 1. Registered secrets.
        for (raw, placeholder) in &self.secrets {
            result = result.replace(raw, placeholder);
        }

        // 2. Registered path prefixes (longest first). All variants
        //    (raw backslash, JSON-escaped double-backslash, forward-slash)
        //    are registered so matching works regardless of how the path
        //    appears in the response.
        for prefix in &self.paths {
            result = result.replace(prefix, REDACTED_PATH);
        }
        // Normalize any remaining backslash separators adjacent to the
        // redacted-path placeholder so the output matches the Unix-generated
        // golden fixtures (which use forward slashes).
        // JSON-escaped Windows paths leave two literal backslashes beside the
        // placeholder. Normalize those before single raw-path separators to
        // preserve valid JSON after prefix replacement.
        while result.contains("\\\\<REDACTED_PATH>") {
            result = result.replace("\\\\<REDACTED_PATH>", "/<REDACTED_PATH>");
        }
        while result.contains("<REDACTED_PATH>\\\\") {
            result = result.replace("<REDACTED_PATH>\\\\", "<REDACTED_PATH>/");
        }
        while result.contains("\\<REDACTED_PATH>") {
            result = result.replace("\\<REDACTED_PATH>", "/<REDACTED_PATH>");
        }
        while result.contains("<REDACTED_PATH>\\") {
            result = result.replace("<REDACTED_PATH>\\", "<REDACTED_PATH>/");
        }

        // 3. Scrub token/secret/secretHash/passcode JSON fields BEFORE the
        //    hex_id_re runs. This is critical: the pairing token is a long hex
        //    string that would be replaced with <REDACTED_ID> by hex_id_re,
        //    but the golden fixture expects <REDACTED_TOKEN>. By scrubbing
        //    these fields first, the token value is replaced with
        //    <REDACTED_TOKEN> before hex_id_re can touch it.
        //
        //    The original Go in-process harness registered the token as a
        //    secret during capture (registerPairingSecrets), so its
        //    ScrubUnregisteredTokens ran after hex_id_re as a
        //    defense-in-depth backstop. The black-box runner can't register
        //    the token (it doesn't parse the response before redacting), so
        //    it must scrub these fields first.
        result = self.scrub_secret_fields(&result);

        // 4. Timestamps.
        result = self
            .timestamp_re
            .replace_all(&result, REDACTED_TIMESTAMP)
            .to_string();

        // 5. Long hex IDs.
        result = self.hex_id_re.replace_all(&result, REDACTED_ID).to_string();

        // 6. PIDs.
        result = self
            .pid_re
            .replace_all(&result, "PID <REDACTED_PID>")
            .to_string();

        // 7. Ports.
        result = self
            .port_re
            .replace_all(&result, "Port:      <REDACTED_PORT>")
            .to_string();

        // 8. CLI passcodes (Passcode: word-word-word-word format).
        result = self
            .passcode_re
            .replace_all(&result, "Passcode: <REDACTED_PASSCODE>")
            .to_string();

        result
    }

    /// Scrub token/secret/secretHash/passcode JSON fields that were not
    /// explicitly registered. This runs BEFORE hex_id_re so that token values
    /// are replaced with <REDACTED_TOKEN> (matching the golden fixtures)
    /// rather than <REDACTED_ID>.
    fn scrub_secret_fields(&self, s: &str) -> String {
        // Scrub token/secret/secretHash fields with ≥16-char values.
        let result = self
            .hex_token_re
            .replace_all(s, r#""$1":"<REDACTED_TOKEN>""#)
            .to_string();

        // Scrub passcode fields (four-word mnemonic: word-word-word-word).
        // These are temporary secrets that must not appear in fixtures.
        let passcode_json_re = Regex::new(r#""passcode"\s*:\s*"([a-z]+-[a-z]+-[a-z]+-[a-z]+)""#)
            .expect("valid passcode JSON regex");
        passcode_json_re
            .replace_all(&result, r#""passcode":"<REDACTED_PASSCODE>""#)
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
    fn test_json_escaped_windows_path_redaction_preserves_valid_json() {
        let mut r = Redactor::new();
        r.register_path(r"C:\Users\adama\AppData\Local\Temp\.tmp123");
        let input =
            r#"{"qrPath":"C:\\Users\\adama\\AppData\\Local\\Temp\\.tmp123\\pairing-id.png"}"#;
        let output = r.redact(input);

        assert_eq!(output, r#"{"qrPath":"<REDACTED_PATH>/pairing-id.png"}"#);
        serde_json::from_str::<serde_json::Value>(&output).expect("redacted JSON remains valid");
    }

    #[test]
    fn test_forward_slash_windows_path_redaction() {
        let mut r = Redactor::new();
        r.register_path(r"C:\Users\adama\project-hail-larry");
        let input = "C:/Users/adama/project-hail-larry/tests/contract";

        assert_eq!(r.redact(input), "<REDACTED_PATH>/tests/contract");
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
    fn test_token_field_scrub_before_hex_id() {
        // A token that is ≥20 hex chars would be replaced by <REDACTED_ID> if
        // hex_id_re ran first. The redactor scrubs token fields first so the
        // result is <REDACTED_TOKEN>, matching the golden fixtures.
        let r = Redactor::new();
        let input = r#"{"token":"aabbccddeeff00112233445566778899"}"#;
        let output = r.redact(input);
        assert!(output.contains(REDACTED_TOKEN));
        assert!(!output.contains(REDACTED_ID));
    }

    #[test]
    fn test_passcode_json_field_scrub() {
        let r = Redactor::new();
        let input = r#"{"passcode":"juice-army-pioneer-digital"}"#;
        let output = r.redact(input);
        assert!(output.contains(REDACTED_PASSCODE));
        assert!(!output.contains("juice-army-pioneer-digital"));
    }
}
