//! Linux systemd user-service integration.
//!
//! The daemon is registered only in the invoking user's systemd scope. This
//! intentionally avoids system-wide installation and its root requirement.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};

use crate::fsutil;

const UNIT_NAME: &str = "local-agent.service";

/// Return the per-user systemd unit path.
fn unit_path() -> Result<PathBuf> {
    let home = fsutil::home_dir().ok_or_else(|| anyhow!("resolve home directory"))?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join(UNIT_NAME))
}

/// Escape an executable path for a double-quoted systemd `ExecStart` argument.
fn systemd_escape_argument(value: &str) -> String {
    value.replace('\\', r"\\").replace('"', r#"\""#)
}

/// Build the systemd user unit content for `binary`.
fn unit_content(binary: &str) -> String {
    format!(
        "[Unit]\n\
Description=Local Agent Interface\n\
After=network.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart=\"{}\" start\n\
Restart=on-failure\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        systemd_escape_argument(binary)
    )
}

/// Register the daemon as a systemd user service.
pub(super) fn install() -> Result<()> {
    let binary = std::env::current_exe().context("resolve current executable")?;
    let unit_path = unit_path()?;
    match fs::symlink_metadata(&unit_path) {
        Ok(_) => {
            return Err(anyhow!(
                "service unit already exists at {} — run 'local_agent uninstall-service' first",
                unit_path.display()
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("stat {}", unit_path.display())),
    }

    let parent = unit_path
        .parent()
        .ok_or_else(|| anyhow!("systemd unit path has no parent"))?;
    // Match Go's 0750 parent directory while respecting the user's umask.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o750)
            .create(parent)
            .with_context(|| format!("create systemd unit directory {}", parent.display()))?;
    }

    let content = unit_content(&binary.to_string_lossy());
    fsutil::atomic_write(&unit_path, content.as_bytes(), Some(0o644))
        .with_context(|| format!("write systemd unit {}", unit_path.display()))?;

    run_systemctl(
        &["--user", "daemon-reload"],
        "systemctl --user daemon-reload",
    )?;
    run_systemctl(
        &["--user", "enable", UNIT_NAME],
        "systemctl --user enable local-agent.service",
    )?;

    info!(unit = %unit_path.display(), "installed systemd user service");
    writeln!(
        io::stdout(),
        "Installed systemd user unit: {}",
        unit_path.display()
    )
    .context("write service install status")?;
    writeln!(
        io::stdout(),
        "Start it now with: systemctl --user start {UNIT_NAME}"
    )
    .context("write service start instruction")
}

/// Disable and remove the daemon's systemd user service.
pub(super) fn uninstall() -> Result<()> {
    let unit_path = unit_path()?;

    if let Err(error) = run_systemctl(
        &["--user", "disable", UNIT_NAME],
        "systemctl --user disable local-agent.service",
    ) {
        // Preserve Go's best-effort disable behavior so an already-removed
        // unit can still be cleaned up locally.
        warn!(%error, "disable systemd user service before removal");
        writeln!(io::stderr(), "warning: {error}").context("write service disable warning")?;
    }

    match fs::remove_file(&unit_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("remove {}", unit_path.display())),
    }
    run_systemctl(
        &["--user", "daemon-reload"],
        "systemctl --user daemon-reload",
    )?;

    info!(unit = %unit_path.display(), "removed systemd user service");
    writeln!(
        io::stdout(),
        "Removed systemd user unit: {}",
        unit_path.display()
    )
    .context("write service uninstall status")
}

/// Run a systemctl command and retain its diagnostic output on failure.
fn run_systemctl(args: &[&str], operation: &str) -> Result<()> {
    let output = Command::new("systemctl")
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
    fn unit_content_matches_systemd_contract() {
        let content = unit_content("/usr/local/bin/local_agent");
        for expected in [
            "[Unit]",
            "Description=Local Agent Interface",
            "After=network.target",
            "[Service]",
            "Type=simple",
            "ExecStart=\"/usr/local/bin/local_agent\" start",
            "Restart=on-failure",
            "RestartSec=5",
            "[Install]",
            "WantedBy=default.target",
        ] {
            assert!(content.contains(expected), "unit missing {expected:?}");
        }
    }

    #[test]
    fn unit_content_escapes_quoted_binary_path() {
        let content = unit_content("/tmp/a\"b/local_agent");
        assert!(content.contains(r#"ExecStart="/tmp/a\"b/local_agent" start"#));
    }
}
