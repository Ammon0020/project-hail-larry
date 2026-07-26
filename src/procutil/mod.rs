//! Process-group helpers for subprocess trees (shell + ACP agents).
//!
//! `tokio::process::Command::kill_on_drop` / `async_process` kill-on-drop only
//! terminate the direct child. On Unix we put the child in its own process
//! group via `setpgid(0,0)` in a `pre_exec` hook, then kill the whole group
//! with `kill(-pgid, SIGKILL)`. That ensures grandchildren of a shell pipeline
//! or an ACP agent cannot survive daemon shutdown.
//!
//! On Windows the child is assigned to a Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; closing the job handle on cancel or
//! drop kills the entire process tree (the standard Windows equivalent of Unix
//! `killpg`). `CREATE_NEW_PROCESS_GROUP` is also set for console signal
//! isolation. On Linux, [`kill_process_tree`] supplements the group kill by
//! walking `/proc` for descendants that escaped via `setsid()`.
//!
//! Callers configure a `std::process::Command` (via tokio's `as_std_mut()` or
//! before converting into `async_process::Command`), then keep a
//! [`ProcessGroupCleanup`] guard until the child has been reaped.

use std::process::Command;

/// Kills the Unix process group when a running command future / actor is dropped.
///
/// Tokio / async-process `kill_on_drop` only handles the direct child. This
/// guard extends that behavior to the dedicated process group configured by
/// [`configure_process_group`].
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

/// Windows Job Object cleanup guard. The child (and all its descendants) are
/// assigned to a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; closing
/// the job handle on drop or cancel kills the entire tree — the standard
/// Windows equivalent of Unix `kill(-pgid, SIGKILL)`.
#[cfg(windows)]
pub struct ProcessGroupCleanup {
    job: Option<*mut std::ffi::c_void>,
}

#[cfg(windows)]
impl ProcessGroupCleanup {
    /// Create a Job Object, assign the child process (identified by `pid`) to
    /// it, and return a guard whose drop closes the job handle. If the job or
    /// process cannot be opened, returns a guard with no job — callers fall
    /// back to `child.kill()` (immediate child only).
    #[must_use]
    pub fn new(pid: Option<u32>) -> Self {
        // Open the child process to assign it to the job. Only
        // PROCESS_SET_QUOTA (required by AssignProcessToJobObject) and
        // PROCESS_TERMINATE are requested.
        const PROCESS_SET_QUOTA: u32 = 0x0100;
        const PROCESS_TERMINATE: u32 = 0x0001;
        let Some(pid) = pid else {
            return Self { job: None };
        };
        let job = unsafe { create_kill_on_close_job() };
        if job.is_null() {
            return Self { job: None };
        }
        unsafe {
            let process = winapi::OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                winapi::AssignProcessToJobObject(job, process);
                winapi::CloseHandle(process);
            }
        }
        Self { job: Some(job) }
    }

    /// Close the job handle after a successful reap. Because
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` only signals remaining members,
    /// closing after all have exited is a no-op.
    pub fn disarm(&mut self) {
        if let Some(job) = self.job.take() {
            unsafe { winapi::CloseHandle(job) };
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessGroupCleanup {
    fn drop(&mut self) {
        // Closing the job handle kills all assigned processes and their
        // descendants (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE).
        if let Some(job) = self.job.take() {
            unsafe { winapi::CloseHandle(job) };
        }
    }
}

// SAFETY: The wrapped Job Object handle is an opaque kernel object reference,
// not thread-affine. `CloseHandle` (the only operation performed on the stored
// handle after construction) is thread-safe per the Windows API. All
// mutation of the handle occurs in `new` (before the guard is shared) or under
// `&mut self` in `disarm`/`drop`, so there is no concurrent access to the
// stored pointer itself. Implementing `Send` lets the guard be held across
// `.await` points inside a `tokio::spawn` future; `Sync` is implemented for
// symmetry and because shared `&self` access never touches the handle.
#[cfg(windows)]
unsafe impl Send for ProcessGroupCleanup {}
#[cfg(windows)]
unsafe impl Sync for ProcessGroupCleanup {}

/// Fallback for non-Unix non-Windows targets (no process-group tree kill).
#[cfg(not(any(unix, windows)))]
pub struct ProcessGroupCleanup;

#[cfg(not(any(unix, windows)))]
impl ProcessGroupCleanup {
    #[must_use]
    pub fn new(_pid: Option<u32>) -> Self {
        Self
    }

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

/// Best-effort kill of all descendants of `root_pid` by walking `/proc`.
///
/// This supplements [`kill_process_group`]: a grandchild that called
/// `setsid()` escapes the process group (it starts a new session), but is
/// still a descendant of `root_pid` and will be caught by this tree walk.
/// Race conditions are inherent (processes can spawn between scan and kill),
/// so this is a best-effort supplementary measure, not a guarantee. The
/// process group kill is the primary mechanism; this catches escapees.
#[cfg(target_os = "linux")]
pub fn kill_process_tree(root_pid: i32) {
    for pid in collect_descendants(root_pid) {
        // SAFETY: SIGKILL is non-catchable; ESRCH means already gone.
        unsafe {
            let _ = libc::kill(pid, libc::SIGKILL);
        }
    }
}

/// Walk `/proc/*/stat` to collect every descendant of `root_pid` (BFS by
/// PPID relationship). Returns only descendants, not `root_pid` itself.
#[cfg(target_os = "linux")]
// pid/ppid pair is intentional — parent vs child process ids read from /proc.
#[allow(clippy::similar_names)]
fn collect_descendants(root_pid: i32) -> Vec<i32> {
    use std::collections::{HashMap, VecDeque};

    // Build parent → children map from /proc.
    let mut children_by_parent: HashMap<i32, Vec<i32>> = HashMap::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let pid: i32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
                Some(p) => p,
                None => continue,
            };
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            // Format: "pid (comm) state ppid ..." — comm may contain spaces
            // and parens, so split at the LAST ")".
            let Some((_, rest)) = stat.rsplit_once(") ") else {
                continue;
            };
            let ppid: i32 = rest
                .split_whitespace()
                .nth(1) // state is field 0, ppid is field 1
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if ppid > 0 {
                children_by_parent.entry(ppid).or_default().push(pid);
            }
        }
    }

    // BFS from root_pid to collect all descendants.
    let mut descendants = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(root_pid);
    while let Some(pid) = queue.pop_front() {
        if let Some(children) = children_by_parent.get(&pid) {
            for &child in children {
                descendants.push(child);
                queue.push_back(child);
            }
        }
    }
    descendants
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
    // new group for console signal isolation. Full tree kill is handled by
    // the Job Object assigned in `ProcessGroupCleanup::new`.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

// ---- Windows Job Object FFI (raw declarations; no windows-sys dependency) ----
#[cfg(windows)]
mod winapi {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct JobObjectBasicLimitInformation {
        pub per_process_user_time_limit: i64,
        pub per_job_user_time_limit: i64,
        pub limit_flags: u32,
        pub minimum_working_set_size: usize,
        pub maximum_working_set_size: usize,
        pub active_process_limit: u32,
        pub affinity: usize,
        pub priority_class: u32,
        pub scheduling_class: u32,
    }

    #[repr(C)]
    // Field names mirror the Windows API `IO_COUNTERS` struct
    // (ReadOperationCount, etc.) — keep the faithful snake_case translation.
    #[allow(clippy::struct_field_names)]
    pub struct IoCounters {
        pub read_operation_count: u64,
        pub write_operation_count: u64,
        pub other_operation_count: u64,
        pub read_transfer_count: u64,
        pub write_transfer_count: u64,
        pub other_transfer_count: u64,
    }

    #[repr(C)]
    pub struct JobObjectExtendedLimitInformation {
        pub basic_limit_information: JobObjectBasicLimitInformation,
        pub io_info: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory_used: usize,
        pub peak_job_memory_used: usize,
    }

    pub const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    pub const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

    extern "system" {
        pub fn CreateJobObjectW(lp_job_attributes: *mut c_void, lp_name: *const u16)
            -> *mut c_void;
        pub fn SetInformationJobObject(
            h_job: *mut c_void,
            info_class: u32,
            info: *mut c_void,
            len: u32,
        ) -> i32;
        pub fn AssignProcessToJobObject(h_job: *mut c_void, h_process: *mut c_void) -> i32;
        pub fn CloseHandle(handle: *mut c_void) -> i32;
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    }
}

/// Create a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so that
/// closing the handle kills all assigned processes. Returns a null handle on
/// failure.
#[cfg(windows)]
unsafe fn create_kill_on_close_job() -> *mut std::ffi::c_void {
    use winapi::{
        CloseHandle, CreateJobObjectW, IoCounters, JobObjectBasicLimitInformation,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
    if job.is_null() {
        return std::ptr::null_mut();
    }
    let mut info = JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation {
            per_process_user_time_limit: 0,
            per_job_user_time_limit: 0,
            limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            minimum_working_set_size: 0,
            maximum_working_set_size: 0,
            active_process_limit: 0,
            affinity: 0,
            priority_class: 0,
            scheduling_class: 0,
        },
        io_info: IoCounters {
            read_operation_count: 0,
            write_operation_count: 0,
            other_operation_count: 0,
            read_transfer_count: 0,
            write_transfer_count: 0,
            other_transfer_count: 0,
        },
        process_memory_limit: 0,
        job_memory_limit: 0,
        peak_process_memory_used: 0,
        peak_job_memory_used: 0,
    };
    // `size_of` on a `repr(C)` Windows struct is a compile-time constant that
    // always fits in `u32` on the platforms we target.
    #[allow(clippy::cast_possible_truncation)]
    let info_len = std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32;
    if SetInformationJobObject(
        job,
        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
        std::ptr::from_mut(&mut info).cast::<std::ffi::c_void>(),
        info_len,
    ) == 0
    {
        CloseHandle(job);
        return std::ptr::null_mut();
    }
    job
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
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
