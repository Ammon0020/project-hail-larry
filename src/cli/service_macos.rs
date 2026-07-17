//! macOS launchd LaunchAgent integration.
//!
//! The plist is installed under the invoking user's `~/Library`, never into
//! a system-wide LaunchDaemon location that would require administrator rights.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};

use crate::fsutil;

const PLIST_NAME: &str = "com.local-agent.plist";
const LABEL: &str = "com.local-agent";

/// Return the per-user launchd plist path.
fn plist_path() -> Result<PathBuf> {
    let home = fsutil::home_dir().ok_or_else(|| anyhow!("resolve home directory"))?;
    Ok(home.join("Library").join("LaunchAgents").join(PLIST_NAME))
}

/// Return a user-private daemon log path.
fn log_path(name: &str) -> Result<PathBuf> {
    let home = fsutil::home_dir().ok_or_else(|| anyhow!("resolve home directory"))?;
    Ok(home.join("Library").join("Logs").join(name))
}

/// Escape content embedded in a plist XML text node.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build the launchd plist that starts `binary` at user login.
fn plist_content(binary: &str, stdout_log: &str, stderr_log: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
</dict>
</plist>
"#,
        xml_escape(binary),
        xml_escape(stdout_log),
        xml_escape(stderr_log)
    )
}

/// Register the daemon as a per-user launchd LaunchAgent.
pub(super) fn install() -> Result<()> {
    let binary = std::env::current_exe().context("resolve current executable")?;
    let plist_path = plist_path()?;
    match fs::symlink_metadata(&plist_path) {
        Ok(_) => {
            return Err(anyhow!(
                "launch agent already exists at {} — run 'local_agent uninstall-service' first",
                plist_path.display()
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("stat {}", plist_path.display())),
    }

    let parent = plist_path
        .parent()
        .ok_or_else(|| anyhow!("launch agent plist path has no parent"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .with_context(|| format!("create LaunchAgents directory {}", parent.display()))?;
    }

    let stdout_log = log_path("local-agent.log")?;
    let stderr_log = log_path("local-agent.err")?;
    let content = plist_content(
        &binary.to_string_lossy(),
        &stdout_log.to_string_lossy(),
        &stderr_log.to_string_lossy(),
    );
    fsutil::atomic_write(&plist_path, content.as_bytes(), Some(0o644))
        .with_context(|| format!("write launchd plist {}", plist_path.display()))?;
    run_launchctl(&["load", &plist_path.to_string_lossy()], "launchctl load")?;

    info!(plist = %plist_path.display(), "installed launchd user service");
    writeln!(
        io::stdout(),
        "Installed launchd LaunchAgent: {}",
        plist_path.display()
    )
    .context("write service install status")?;
    writeln!(
        io::stdout(),
        "It will start at next login (or run: launchctl start {LABEL})"
    )
    .context("write service start instruction")
}

/// Unload and remove the daemon's per-user launchd LaunchAgent.
pub(super) fn uninstall() -> Result<()> {
    let plist_path = plist_path()?;
    if let Err(error) = run_launchctl(
        &["unload", &plist_path.to_string_lossy()],
        "launchctl unload",
    ) {
        // Retain Go's idempotent cleanup behavior if the plist was removed
        // before launchctl was asked to unload it.
        warn!(%error, "unload launchd user service before removal");
        writeln!(io::stderr(), "warning: {error}").context("write service unload warning")?;
    }
    match fs::remove_file(&plist_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("remove {}", plist_path.display()));
        }
    }

    info!(plist = %plist_path.display(), "removed launchd user service");
    writeln!(
        io::stdout(),
        "Removed launchd LaunchAgent: {}",
        plist_path.display()
    )
    .context("write service uninstall status")
}

/// Run launchctl and retain its diagnostic output on failure.
fn run_launchctl(args: &[&str], operation: &str) -> Result<()> {
    let output = Command::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("run {operation}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "{operation}: {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_content_matches_launchd_contract() {
        let content = plist_content(
            "/usr/local/bin/local_agent",
            "/Users/test/Library/Logs/local-agent.log",
            "/Users/test/Library/Logs/local-agent.err",
        );
        for expected in [
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            "<plist version=\"1.0\">",
            "<string>com.local-agent</string>",
            "<string>/usr/local/bin/local_agent</string>",
            "<string>start</string>",
            "<key>RunAtLoad</key>",
            "<key>KeepAlive</key>",
            "<key>StandardOutPath</key>",
            "<key>StandardErrorPath</key>",
        ] {
            assert!(content.contains(expected), "plist missing {expected:?}");
        }
    }

    #[test]
    fn plist_content_escapes_xml() {
        let content = plist_content("/tmp/A&B/local_agent", "/tmp/out", "/tmp/err");
        assert!(content.contains("<string>/tmp/A&amp;B/local_agent</string>"));
    }
}
