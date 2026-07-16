//! CLI tests for the contract differential runner.
//!
//! Each CLI command is run as a subprocess of the backend binary with
//! LOCAL_AGENT_STATE_DIR pointing at the harness's isolated state dir. The
//! running backend (started by the harness) serves the API calls that CLI
//! commands like `pair`, `devices`, and `revoke` make.
//!
//! The captured output (stdout, stderr, exit code) is formatted into the same
//! envelope shape as the Go fixture harness and compared against the golden
//! fixture:
//!
//! ```text
//! $ app <args...>
//! exit: <code>
//! --- stdout ---
//! <stdout>
//! --- stderr ---
//! <stderr>
//! ```
//!
//! All captured text is redacted (secrets + absolute paths + PIDs + ports)
//! before comparison.
//!
//! Note: the `revoke` and `devices` commands need a paired device. The Go
//! harness pairs a device through the in-process server before running CLI
//! commands. The black-box runner pairs a device through the live API before
//! running these commands. The `revoke` command's golden fixture expects a
//! 202 response (grace-period revocation), so the runner must pair a device
//! first and then revoke it.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;

use crate::harness::BackendHarness;
use crate::redactor::Redactor;

/// A CLI test case. Mirrors `cliCase` in go-fixtures/cli.go.
struct CliCase {
    /// The golden fixture name (without extension).
    name: &'static str,
    /// The argument vector passed to the binary (excluding the binary name).
    args: &'static [&'static str],
}

/// The full list of CLI cases. Mirrors `buildCLICases` in go-fixtures/cli.go.
/// The `revoke` case is handled specially: the runner pairs a device first and
/// passes the real device ID as an argument.
const CLI_CASES: &[CliCase] = &[
    CliCase { name: "root_help", args: &["--help"] },
    CliCase { name: "status", args: &["status"] },
    CliCase { name: "add_folder", args: &["add-folder", "tests/contract/fixtures/seed-workspace"] },
    CliCase { name: "list_folders", args: &["list-folders"] },
    CliCase { name: "remove_folder_not_found", args: &["remove-folder", "nonexistent-id"] },
    CliCase { name: "pair", args: &["pair"] },
    CliCase { name: "devices", args: &["devices"] },
    // revoke is handled specially — see run_case.
    CliCase { name: "revoke", args: &["revoke"] },
    CliCase { name: "logs", args: &["logs"] },
    CliCase { name: "stop_not_running", args: &["stop"] },
    CliCase { name: "start_help", args: &["start", "--help"] },
    CliCase { name: "install_service_help", args: &["install-service", "--help"] },
    CliCase { name: "uninstall_service_help", args: &["uninstall-service", "--help"] },
];

/// Find a CLI case by name.
fn find_case(name: &str) -> Option<&'static CliCase> {
    CLI_CASES.iter().find(|c| c.name == name)
}

/// Resolve the golden CLI fixture directory path.
fn golden_dir(harness: &BackendHarness) -> PathBuf {
    harness.repo_root.join("tests").join("contract").join("golden").join("cli")
}

/// Run a single CLI test case by name. Runs the CLI command as a subprocess,
/// redacts the output, and compares against the golden fixture.
pub async fn run_case(harness: &BackendHarness, name: &str) {
    let case = find_case(name).unwrap_or_else(|| panic!("unknown CLI case: {name}"));

    // The "stop_not_running" case must NOT see a PID file, otherwise it would
    // SIGTERM the running backend. Remove it for this case only.
    let pid_file = harness.state_dir.join("daemon.pid");
    let pid_backup = if name == "stop_not_running" {
        std::fs::read(&pid_file).ok()
    } else {
        None
    };
    if name == "stop_not_running" {
        let _ = std::fs::remove_file(&pid_file);
    }

    // For the "devices" and "revoke" cases, pair a device first so the
    // commands have a real target. The Go harness pairs a device via
    // pairDeviceForCLI before running CLI commands; the black-box runner does
    // the same via the live API. For "revoke", the device ID is passed as an
    // extra argument.
    let mut args: Vec<String> = case.args.iter().map(|s| s.to_string()).collect();
    let paired_device_id: Option<String> = if name == "devices" || name == "revoke" {
        Some(pair_device_for_cli(harness).await)
    } else {
        None
    };
    if name == "revoke" {
        args.push(paired_device_id.clone().unwrap());
    }

    // Run the CLI command. The working directory is set to
    // tests/contract/go-fixtures/ to match the Go harness's behavior — the
    // Go harness runs CLI commands from that directory, so relative paths in
    // CLI args (like add-folder's "tests/contract/fixtures/seed-workspace")
    // resolve differently than they would from the repo root. This affects
    // the "Workspace registered" vs "already registered" message and the
    // redacted absolute path in the output.
    let cli_cwd = harness
        .repo_root
        .join("tests")
        .join("contract")
        .join("go-fixtures");
    let output = tokio::process::Command::new(&harness.binary_path)
        .args(&args)
        .env("LOCAL_AGENT_STATE_DIR", &harness.state_dir)
        .current_dir(&cli_cwd)
        .output()
        .await
        .with_context(|| format!("run CLI command: {} {args:?}", harness.binary_path.display()))
        .expect("run CLI subprocess");

    // Restore the PID file if we removed it.
    if name == "stop_not_running" {
        if let Some(backup) = pid_backup {
            let _ = std::fs::write(&pid_file, backup);
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    // Build the redactor. Register the workspace ID as a secret so it is
    // replaced with <REDACTED_WORKSPACE_ID> in CLI output (e.g. list-folders
    // prints "ID\tname\tpath" lines that contain the workspace ID).
    let ws_id = harness.workspace_id().await;
    let mut redactor = Redactor::new();
    redactor.register_path(harness.state_dir.to_str().unwrap());
    if let Ok(home) = std::env::var("HOME") {
        redactor.register_path(&home);
    }
    redactor.register_path(harness.repo_root.to_str().unwrap());
    redactor.register_secret(&ws_id, crate::redactor::REDACTED_WORKSPACE_ID);
    redactor.register_secret(
        &format!("127.0.0.1:{}", harness.port),
        "127.0.0.1:<REDACTED_PORT>",
    );
    redactor.register_secret(
        &format!("localhost:{}", harness.port),
        "localhost:<REDACTED_PORT>",
    );
    // Register the paired device ID (if any) so it is redacted in CLI output.
    // The full 32-char hex ID is replaced with <REDACTED_DEVICE_ID> by the
    // registered secret (matching the golden fixture). The `devices` table
    // shows only the first 12 chars (below the hex_id_re threshold of 20),
    // which is handled by redact_device_id_in_table before comparison.
    if let Some(ref dev_id) = paired_device_id {
        redactor.register_secret(dev_id, crate::redactor::REDACTED_DEVICE_ID);
    }

    // Format the envelope (same shape as go-fixtures/cli.go formatCLIEnvelope).
    let envelope = format_cli_envelope(&args, exit_code, &stdout, &stderr, &redactor);

    // Load and compare the golden fixture.
    let fixture_path = golden_dir(harness).join(format!("{name}.txt"));
    let expected = std::fs::read_to_string(&fixture_path)
        .with_context(|| format!("read golden fixture {}", fixture_path.display()))
        .expect("read golden fixture");

    // Some CLI outputs contain box-drawing art (e.g. the `pair` command draws
    // a Unicode box with the passcode, URL, QR path, and expiry). The box
    // padding is calculated from the ACTUAL passcode length, but the golden
    // fixture was generated with a different random passcode. After redaction
    // both sides have <REDACTED_PASSCODE> but different amounts of trailing
    // whitespace. Normalize runs of 2+ spaces to a single space for the `pair`
    // case so the comparison is stable.
    let actual_normalized = if name == "pair" {
        normalize_box_whitespace(&envelope)
    } else {
        envelope.clone()
    };
    let expected_normalized = if name == "pair" {
        normalize_box_whitespace(&expected)
    } else {
        expected.clone()
    };

    // The "devices" and "revoke" cases contain machine-specific device IDs
    // that differ between the golden fixture (generated on one machine) and
    // the runner (random each run). Redact the device IDs in both the actual
    // and expected output before comparison so the match is stable.
    if name == "revoke" {
        let actual_redacted = redact_device_id_in_cmdline(&actual_normalized);
        let expected_redacted = redact_device_id_in_cmdline(&expected_normalized);
        if actual_redacted != expected_redacted {
            eprintln!("[contract] FAIL: {name}");
            eprintln!("--- expected ---\n{expected_redacted}");
            eprintln!("--- actual ---\n{actual_redacted}");
            panic!("CLI case {name} mismatch (device ID redacted for comparison)");
        }
    } else if name == "devices" {
        let actual_redacted = redact_device_id_in_table(&actual_normalized);
        let expected_redacted = redact_device_id_in_table(&expected_normalized);
        if actual_redacted != expected_redacted {
            eprintln!("[contract] FAIL: {name}");
            eprintln!("--- expected ---\n{expected_redacted}");
            eprintln!("--- actual ---\n{actual_redacted}");
            panic!("CLI case {name} mismatch (device ID redacted for comparison)");
        }
    } else if actual_normalized != expected_normalized {
        eprintln!("[contract] FAIL: {name}");
        eprintln!("--- expected ---\n{expected_normalized}");
        eprintln!("--- actual ---\n{actual_normalized}");
        panic!("CLI case {name} mismatch");
    }

    eprintln!("[contract] PASS: {name} (exit: {exit_code})");
}

/// Format the CLI output envelope. Mirrors `formatCLIEnvelope` in
/// go-fixtures/cli.go.
fn format_cli_envelope(
    args: &[String],
    exit: i32,
    stdout: &str,
    stderr: &str,
    redactor: &Redactor,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("$ app {}\n", args.join(" ")));
    out.push_str(&format!("exit: {exit}\n"));
    out.push_str("--- stdout ---\n");
    let redacted_stdout = redactor.redact(stdout);
    out.push_str(&redacted_stdout);
    if !stdout.is_empty() && !stdout.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("--- stderr ---\n");
    let redacted_stderr = redactor.redact(stderr);
    out.push_str(&redacted_stderr);
    if !stderr.is_empty() && !stderr.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Redact the device ID in a CLI command line for the revoke case. The device
/// ID is a 32-char hex string that appears as the last argument in
/// "$ app revoke <deviceID>". Replace it with <REDACTED_ID> so the comparison
/// is stable across runs.
fn redact_device_id_in_cmdline(text: &str) -> String {
    // Match "$ app revoke " followed by a 32-char hex string.
    let re = regex::Regex::new(r"\$ app revoke [a-f0-9]{32}").expect("valid regex");
    re.replace_all(text, "$ app revoke <REDACTED_ID>").to_string()
}

/// Normalize whitespace in box-drawing output for stable comparison. The `pair`
/// command draws a Unicode box whose padding depends on the random passcode
/// length. After redaction both sides have <REDACTED_PASSCODE> but different
/// amounts of trailing spaces. This function collapses runs of 2+ spaces to a
/// single space so the comparison is stable regardless of passcode length.
fn normalize_box_whitespace(text: &str) -> String {
    let re = regex::Regex::new(r" {2,}").expect("valid whitespace regex");
    re.replace_all(text, " ").to_string()
}

/// Redact the device ID in the `devices` CLI table output. The table shows
/// the first 12 chars of the device ID in the first column. Replace any
/// 12-char hex string that appears at the start of a data row with
/// <REDACTED_ID> so the comparison is stable across runs.
fn redact_device_id_in_table(text: &str) -> String {
    // Match a 12-char hex string at the start of a line followed by whitespace
    // (the device ID column in the table).
    let re = regex::Regex::new(r"(?m)^([a-f0-9]{12})\s").expect("valid regex");
    re.replace_all(text, "<REDACTED_ID> ").to_string()
}

/// Pair a device through the live API so the `revoke` CLI command has a real
/// target. Returns the device ID.
async fn pair_device_for_cli(harness: &BackendHarness) -> String {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build reqwest client");

    // Step 1: initiate pairing.
    let initiate_url = format!("{}/api/pair/initiate", harness.base_url);
    let initiate_body = serde_json::json!({
        "host": "localhost",
        "port": harness.port,
    });
    let resp = client
        .post(&initiate_url)
        .json(&initiate_body)
        .send()
        .await
        .expect("initiate pairing");
    assert!(
        resp.status().is_success(),
        "pair initiate failed: {}",
        resp.status()
    );
    let session: serde_json::Value = resp.json().await.expect("parse pairing session");
    let passcode = session
        .get("passcode")
        .and_then(|v| v.as_str())
        .expect("passcode in pairing session");

    // Step 2: verify passcode to get a device credential.
    let verify_url = format!("{}/api/pair/verify-passcode", harness.base_url);
    let verify_body = serde_json::json!({
        "passcode": passcode,
        "deviceName": "fixture-device",
    });
    let resp = client
        .post(&verify_url)
        .json(&verify_body)
        .send()
        .await
        .expect("verify passcode");
    assert!(
        resp.status().is_success(),
        "pair verify failed: {}",
        resp.status()
    );
    let cred: serde_json::Value = resp.json().await.expect("parse device credential");
    cred.get("id")
        .and_then(|v| v.as_str())
        .expect("device ID in credential")
        .to_string()
}
