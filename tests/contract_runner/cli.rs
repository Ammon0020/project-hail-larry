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
};

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

    // For the "revoke" case, pair a device first so the revoke command has a
    // real target. The device ID is passed as an extra argument.
    let mut args: Vec<String> = case.args.iter().map(|s| s.to_string()).collect();
    if name == "revoke" {
        let device_id = pair_device_for_cli(harness).await;
        args.push(device_id);
    }

    // Run the CLI command.
    let output = tokio::process::Command::new(&harness.binary_path)
        .args(&args)
        .env("LOCAL_AGENT_STATE_DIR", &harness.state_dir)
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

    // Build the redactor.
    let mut redactor = Redactor::new();
    redactor.register_path(&harness.state_dir);
    if let Ok(home) = std::env::var("HOME") {
        redactor.register_path(&home);
    }
    redactor.register_path(&harness.repo_root);
    redactor.register_secret(
        &format!("127.0.0.1:{}", harness.port),
        "127.0.0.1:<REDACTED_PORT>",
    );
    redactor.register_secret(
        &format!("localhost:{}", harness.port),
        "localhost:<REDACTED_PORT>",
    );

    // Format the envelope (same shape as go-fixtures/cli.go formatCLIEnvelope).
    let envelope = format_cli_envelope(&args, exit_code, &stdout, &stderr, &redactor);

    // Load and compare the golden fixture.
    let fixture_path = golden_dir(harness).join(format!("{name}.txt"));
    let expected = std::fs::read_to_string(&fixture_path)
        .with_context(|| format!("read golden fixture {}", fixture_path.display()))
        .expect("read golden fixture");

    // The golden fixture was redacted by the Go harness. The runner's redacted
    // output should match exactly (CLI envelopes are compared byte-for-byte
    // per the README).
    //
    // However, the `revoke` case has a machine-specific device ID in the
    // command line (the `$ app revoke <deviceID>` line). The Go harness
    // redacts the device ID in the output, but the command line itself
    // contains the raw ID. The runner needs to redact the command line too.
    // The Go redactor registers the device ID as a secret, so it is replaced
    // with <REDACTED_DEVICE_ID> in the output. But the golden fixture's
    // command line shows the raw (short) device ID prefix, not the full ID.
    //
    // For the revoke case, the golden fixture has "$ app revoke <full-id>" but
    // the redactor replaces the full ID with <REDACTED_DEVICE_ID>. Wait —
    // looking at the golden fixture, the command line shows the full device ID
    // (e.g. "68e778d32b4d52b41bdf6d744ecfe585"), not a redacted version. This
    // is because the Go harness formats the envelope BEFORE redaction for the
    // command line, then redacts the full text.
    //
    // Actually, looking at formatCLIEnvelope in cli.go, the entire envelope
    // (including the command line) is redacted. So the device ID in the
    // command line should be redacted too. But the golden fixture shows the
    // raw ID... Let me re-check.
    //
    // The Go redactor registers the device ID as a secret with placeholder
    // <REDACTED_DEVICE_ID>. But the hex_id_re also replaces ≥20-char hex
    // strings with <REDACTED_ID>. The device ID is 32 hex chars, so it would
    // be replaced by <REDACTED_ID> by the regex, not by the registered secret.
    // The registered secret replacement happens first, so it should be
    // <REDACTED_DEVICE_ID>. But looking at the golden fixture:
    //
    // "$ app revoke 68e778d32b4d52b41bdf6d744ecfe585"
    //
    // The device ID is NOT redacted! This means the Go harness does NOT
    // redact the command line. Let me re-read formatCLIEnvelope...
    //
    // Looking at cli.go formatCLIEnvelope: it writes "$ app {args}" first,
    // then redacts stdout and stderr separately. The command line is NOT
    // redacted. So the golden fixture has the raw device ID in the command
    // line.
    //
    // This is a portability issue — the device ID is machine-specific (it's
    // a random hex string generated during pairing). The golden fixture will
    // never match the runner's output because the device IDs are different.
    //
    // For the revoke case, I need to handle this specially. The simplest
    // approach: skip the command line comparison for the revoke case, or
    // redact the device ID in the command line before comparison.
    //
    // Actually, the best approach is to redact the entire envelope (including
    // the command line) in the runner, and also fix the Go harness to redact
    // the command line. But the user said "only change code in the tests."
    // The go-fixtures ARE test code, so I can fix them.
    //
    // But wait — I already regenerated the golden fixtures. If I fix the Go
    // harness now, I need to regenerate again. Let me do that.
    //
    // Actually, let me take a different approach for now: for the revoke case,
    // I'll redact the device ID in the command line before comparison. This
    // way I don't need to regenerate the golden fixtures again.
    //
    // Hmm, but the golden fixture has the raw device ID. If I redact it in
    // the runner, the runner's output will have <REDACTED_ID> but the golden
    // fixture has the raw ID. They won't match.
    //
    // The cleanest fix: update the Go harness to redact the command line too,
    // then regenerate. Let me do that after creating all the runner modules.

    if name == "revoke" {
        // The revoke case has a machine-specific device ID in the command line
        // that can't be matched exactly. Compare with the device ID redacted
        // in both the actual and expected output.
        let actual_redacted = redact_device_id_in_cmdline(&envelope);
        let expected_redacted = redact_device_id_in_cmdline(&expected);
        if actual_redacted != expected_redacted {
            eprintln!("[contract] FAIL: {name}");
            eprintln!("--- expected ---\n{expected_redacted}");
            eprintln!("--- actual ---\n{actual_redacted}");
            panic!("CLI case {name} mismatch (device ID redacted for comparison)");
        }
    } else {
        if envelope != expected {
            eprintln!("[contract] FAIL: {name}");
            eprintln!("--- expected ---\n{expected}");
            eprintln!("--- actual ---\n{envelope}");
            panic!("CLI case {name} mismatch");
        }
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
