//! Process-group helpers for subprocess trees (shell + ACP agents).
//!
//! `tokio::process::Command::kill_on_drop` / `async_process` kill-on-drop only
//! terminate the direct child. On Unix we put the child in its own process
//! group via `setpgid(0,0)` in a `pre_exec` hook, then kill the whole group
//! with `kill(-pgid, SIGKILL)`. That ensures grandchildren of a shell pipeline
//! or an ACP agent cannot survive daemon shutdown.
//!
//! On Windows we create a new process group (`CREATE_NEW_PROCESS_GROUP`) and
//! terminate via the child handle. Full tree kill needs a Job Object and is
//! deferred (matches prior Go / shell behaviour: immediate child only).
//!
//! Callers configure a `std::process::Command` (via tokio's `as_std_mut()` or
//! before converting into `async_process::Command`), then keep a
//! [`ProcessGroupCleanup`] guard until the child has been reaped.

use std::process::Command;

/// Kills the Unix process group when a running command future / actor is dropped.
///
/// Tokio / async-process `kill_on_drop` only handles the direct child. This
/// guard extends that behavior to the dedicated process group configured by
/// [`configure_process_group`]. On Windows, `kill_on_drop` remains deliberately
/// bounded to the child handle: terminating descendants requires a Job Object.
#[cfg(unix)]
pub struct ProcessGroupCleanup {
    pgid: Option<i32>,
}

#[cfg(unix)]
impl ProcessGroupCleanup {
    /// Build a cleanup guard from the child's PID (process group id after setup).
    #[must_use]
    pub fn new(pid: Option<u32>) -> Self {
        Self {
            pgid: pid.and_then(|pid| i32::try_from(pid).ok()),
        }
    }

    /// Disarm after a successful reap so drop does not re-signal an exited group.
    pub fn disarm(&mut self) {
        self.pgid = None;
    }
}

#[cfg(unix)]
impl Drop for ProcessGroupCleanup {
    fn drop(&mut self) {
        if let Some(pgid) = self.pgid {
            kill_process_group(pgid);
        }
    }
}

/// No additional drop behavior is needed on Windows because Tokio /
/// async-process `kill_on_drop` safely terminates the immediate child handle.
#[cfg(not(unix))]
pub struct ProcessGroupCleanup;

#[cfg(not(unix))]
impl ProcessGroupCleanup {
    /// Windows stub — process-group tree kill is not available without Job Objects.
    #[must_use]
    pub fn new(_pid: Option<u32>) -> Self {
        Self
    }

    /// No-op on Windows.
    pub fn disarm(&mut self) {}
}

/// Send `SIGKILL` to every member of the process group identified by `pgid`.
///
/// # Safety contract
///
/// `pgid` must be the dedicated group created by [`configure_process_group`]
/// for a child we own. Errors (already exited) are ignored.
#[cfg(unix)]
pub fn kill_process_group(pgid: i32) {
    // SAFETY: negative PID addresses only that process group. SIGKILL is
    // non-catchable; ESRCH / other errors mean the group is already gone.
    unsafe {
        let _ = libc::kill(-pgid, libc::SIGKILL);
    }
}

/// Put `cmd`'s child into an isolated process group (Unix) or new Windows group.
///
/// Must be called before `spawn`. On Unix, failure of `setpgid` aborts spawn so
/// a later group kill cannot miss descendants.
pub fn configure_process_group(cmd: &mut Command) {
    configure_process_group_inner(cmd);
}

#[cfg(unix)]
fn configure_process_group_inner(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: `setpgid` only touches the child's own state. The closure is
    // called post-fork, pre-exec in the child. Return an error rather than
    // spawning without group isolation: otherwise a later kill could fail
    // to contain descendants.
    unsafe {
        cmd.pre_exec(|| {
            // setpgid(0, 0) puts the child into a new process group with pgid
            // == child pid, making it safe to target with kill(-pid, signal).
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn configure_process_group_inner(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NEW_PROCESS_GROUP = 0x00000200. Children of this process form a
    // new group; child.kill() terminates the immediate child. Full group
    // kill requires a Job Object (deferred — Go also only kills the child).
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Dropping the cleanup guard kills a Unix process group, including a
    /// backgrounded grandchild that kill-on-drop alone would leave running.
    #[cfg(unix)]
    #[test]
    fn drop_guard_kills_process_group_grandchild() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let pid_file = dir.path().join("grandchild.pid");
        let pid_file_path = pid_file.to_str().expect("utf-8 path");

        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "sleep 30 & echo $! > \"$1\"; wait",
            "_",
            pid_file_path,
        ]);
        configure_process_group(&mut cmd);

        let child = cmd.spawn().expect("spawn shell with process group");
        let pid = child.id();
        let cleanup = ProcessGroupCleanup::new(Some(pid));

        // Wait until the grandchild PID is written, then drop the guard
        // (SIGKILL the group) without waiting on the shell first.
        let grandchild = wait_for_pid_file(&pid_file, Duration::from_secs(2));
        drop(cleanup);
        // Also drop the Child so we do not leave a reaper holding the shell;
        // the group kill already signalled both processes.
        drop(child);

        let mut exited = false;
        for _ in 0..40 {
            if process_is_gone_or_zombie(grandchild) {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if !exited {
            // SAFETY: best-effort cleanup so a failed assertion does not leak.
            unsafe {
                libc::kill(grandchild, libc::SIGKILL);
            }
        }
        assert!(
            exited,
            "grandchild process {grandchild} survived ProcessGroupCleanup drop"
        );
    }

    #[cfg(unix)]
    fn wait_for_pid_file(path: &std::path::Path, timeout: Duration) -> i32 {
        let start = std::time::Instant::now();
        loop {
            if let Ok(contents) = std::fs::read_to_string(path) {
                let trimmed = contents.trim();
                if !trimmed.is_empty() {
                    return trimmed.parse().expect("numeric grandchild PID");
                }
            }
            assert!(
                start.elapsed() < timeout,
                "timed out waiting for grandchild PID file at {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[cfg(unix)]
    fn process_is_gone_or_zombie(pid: i32) -> bool {
        // SAFETY: kill(pid, 0) is a existence probe; ESRCH means gone.
        if unsafe { libc::kill(pid, 0) } == -1 {
            return std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        }
        // Linux reports process state immediately after the final `)` in
        // `/proc/<pid>/stat`. Treat Z as exited even if not yet reaped.
        let stat_path = format!("/proc/{pid}/stat");
        std::fs::read_to_string(stat_path)
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .map(|(_, rest)| rest.starts_with('Z'))
            })
            .unwrap_or(false)
    }
}
