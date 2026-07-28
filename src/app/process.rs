//! Platform-specific daemon process inspection and termination.

use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// Return whether `pid` still identifies a live process.
#[must_use]
pub fn is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    platform::is_running(pid)
}

/// Ask the daemon process to stop, then wait for it to exit.
///
/// Unix sends SIGTERM so the daemon's signal handler can drain its listeners.
/// Windows uses `taskkill` without `/F` first for the equivalent graceful
/// console-control path.
///
/// # Errors
///
/// Returns an error if the stop signal cannot be sent or the process does not
/// exit within the grace period.
pub fn stop(pid: u32) -> Result<()> {
    if pid == 0 {
        bail!("daemon PID cannot be 0");
    }
    if !is_running(pid) {
        return Ok(());
    }
    platform::request_stop(pid)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_running(pid) {
        if Instant::now() >= deadline {
            bail!("daemon PID {pid} did not exit within 5 seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(unix)]
mod platform {
    use super::{Context, Result};

    pub(super) fn is_running(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // kill(pid, 0) performs no signal delivery. EPERM still proves a
        // process exists, while ESRCH means the PID is stale.
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    pub(super) fn request_stop(pid: u32) -> Result<()> {
        let pid = i32::try_from(pid).context("daemon PID exceeds platform range")?;
        let result = unsafe { libc::kill(pid, libc::SIGTERM) };
        if result != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("send SIGTERM to daemon PID {pid}"));
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::{bail, Context, Result};
    use std::process::Command;

    pub(super) fn is_running(pid: u32) -> bool {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|output| {
                let text = String::from_utf8_lossy(&output.stdout);
                text.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }

    pub(super) fn request_stop(pid: u32) -> Result<()> {
        // Try graceful close first (no /F). A background daemon started with
        // `--background` has no window/console to receive the WM_CLOSE signal,
        // so the graceful taskkill can fail with "process can only be
        // terminated forcefully". Fall back to `/F` in that case so the daemon
        // is still stopped rather than orphaned.
        let graceful = Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .status()
            .context("invoke taskkill")?;
        if graceful.success() {
            return Ok(());
        }
        let forced = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
            .context("invoke taskkill /F")?;
        if !forced.success() {
            bail!("taskkill failed for daemon PID {pid} with status {forced}");
        }
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::{bail, Result};

    pub(super) fn is_running(_pid: u32) -> bool {
        false
    }

    pub(super) fn request_stop(_pid: u32) -> Result<()> {
        bail!("daemon stop is unsupported on this platform")
    }
}
