//! Workspace-scoped subprocess runner (Go `internal/shell/`).
//!
//! Blueprint references: Sec 15 (Shell Execution). The daemon executes
//! approved shell commands on behalf of agents via ACP. Commands run within
//! workspace boundaries; output is streamed line-by-line.
//!
//! # Design
//!
//! - [`tokio::process::Command`] drives async subprocesses.
//! - stdout/stderr are read through `tokio::io::BufReader` line-by-line and
//!   forwarded to caller-supplied callbacks (mirroring Go's `RunAsync` /
//!   `RunAsyncArgs` `onStdout`/`onStderr` callbacks).
//! - The current working directory is enforced against the workspace root via
//!   [`pathutil::clean_path`]; path traversal in a caller-supplied `cwd` is
//!   rejected before the process is spawned.
//! - Each command owns a [`tokio_util::sync::CancellationToken`]. On cancel
//!   (or timeout) the entire process *group* is signalled, not just the
//!   immediate child — so orphaned grandchildren of a shell pipeline cannot
//!   survive daemon shutdown. Isolation and kill live in [`crate::procutil`]
//!   (Unix `setpgid` + `killpg` (+ `/proc` tree walk for `setsid` escapes);
//!   Windows Job Object with `KILL_ON_JOB_CLOSE`).
//! - Each command has a configurable timeout ([`Executor::with_command_timeout`],
//!   default 30 min). On expiry the process group is killed just as on cancel.
//! - Captured/streamed output is bounded by [`Executor::max_output_bytes`]
//!   per stream so a noisy command cannot exhaust daemon memory.
//!
//! All public functions return `std::result::Result`; none panic.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::pathutil::{clean_path, resolve_symlink, PathError};
#[cfg(unix)]
use crate::procutil::kill_process_group;
#[cfg(target_os = "linux")]
use crate::procutil::kill_process_tree;
use crate::procutil::{configure_process_group, ProcessGroupCleanup};

/// Default per-stream output cap (1 MiB). Matches the Go daemon's practical
/// bound for captured command output; callers may override via
/// [`Executor::with_max_output_bytes`]. Without a bound a single
/// `cat /dev/urandom` or runaway `find /` could OOM the daemon.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Default per-command execution timeout (30 minutes). Without a deadline an
/// agent can spawn long-running commands (e.g. `sleep 1000000`) that pin a
/// process slot and consume a PIPED stdout reader task until the session is
/// closed. Callers may override via [`Executor::with_command_timeout`].
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Errors returned by the shell executor.
///
/// `ShellError` distinguishes "could not start" / "invalid invocation" failures
/// (surfaced as `Err`) from a successfully-run command that exited non-zero
/// (reported via [`CommandResult::exit_code`] with `Ok`), matching Go's
/// `buildResult`.
#[derive(Debug, Error)]
pub enum ShellError {
    /// Caller passed an empty command string.
    #[error("empty command")]
    EmptyCommand,

    /// Caller-supplied `cwd` failed workspace containment (traversal or
    /// symlink escape). The process was never spawned.
    #[error("invalid cwd: {0}")]
    InvalidCwd(#[from] PathError),

    /// The subprocess could not be spawned (`Command::spawn` failed).
    #[error("spawn command: {0}")]
    Spawn(std::io::Error),

    /// The command was cancelled (token fired or timeout elapsed) and the
    /// process group was signalled. The partial [`CommandResult`] is still
    /// returned to the caller alongside this error so streamed output is not
    /// lost.
    #[error("command cancelled")]
    Cancelled,

    /// Internal I/O failure while reading a stream or awaiting the child.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Result of a completed command.
///
/// Mirrors Go `shell.Result`. `signal` is `Some` on Unix when the process was
/// terminated by a signal (e.g. cancelled → `SIGKILL`); `None` on Windows or
/// for normal exits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandResult {
    /// Captured stdout (truncated to [`Executor::max_output_bytes`]).
    pub stdout: String,
    /// Captured stderr (truncated to [`Executor::max_output_bytes`]).
    pub stderr: String,
    /// Exit code. `0` on success; non-zero on failure; `-1` when the command
    /// could not start or was cancelled before producing an exit status.
    pub exit_code: i32,
    /// Terminating signal name on Unix (e.g. `"KILLED"`), if any.
    pub signal: Option<String>,
}

/// Executor runs shell commands within a workspace directory.
///
/// Construct with [`Executor::new`]. The spawned process inherits the daemon
/// environment by default; replace it with [`Executor::with_env`] (typically
/// `merge_env(std::env::vars(), agent_vars)`).
#[derive(Debug, Clone)]
pub struct Executor {
    workspace_path: PathBuf,
    env: Option<Vec<(String, String)>>,
    max_output_bytes: usize,
    command_timeout: Option<Duration>,
}

impl Executor {
    /// Create a new executor scoped to `workspace_path`.
    ///
    /// `workspace_path` should be an absolute, canonicalised directory; if it
    /// is not, [`Executor::run`] will still attempt to spawn but per-command
    /// CWD containment uses the canonicalised root (see [`pathutil::clean_path`]).
    pub fn new<P: Into<PathBuf>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.into(),
            env: None,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            command_timeout: Some(DEFAULT_COMMAND_TIMEOUT),
        }
    }

    /// Return a copy of the executor with the environment set to `env`.
    ///
    /// `env` fully replaces the inherited environment (matching Go's
    /// `WithEnv`). Callers should normally build it as
    /// `merge_env(std::env::vars(), agent_vars)` so PATH etc. are preserved.
    /// Pass an empty `Vec` to clear the inherited environment.
    pub fn with_env<I, K, V>(self, env: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            env: Some(env.into_iter().map(|(k, v)| (k.into(), v.into())).collect()),
            ..self
        }
    }

    /// Return a copy of the executor with a custom per-stream output cap.
    /// `bytes == 0` drops all captured output (matching `RetainedOutput`'s
    /// `limit == 0` "discard" convention); use a non-zero value to retain
    /// output up to the cap.
    pub fn with_max_output_bytes(self, bytes: usize) -> Self {
        Self {
            max_output_bytes: bytes,
            ..self
        }
    }

    /// Return a copy of the executor with a custom per-command timeout.
    /// Pass `None` to disable the timeout entirely (commands run until
    /// cancelled or exited). The default is [`DEFAULT_COMMAND_TIMEOUT`].
    pub fn with_command_timeout(self, timeout: Option<Duration>) -> Self {
        Self {
            command_timeout: timeout,
            ..self
        }
    }

    /// Run `command` (via `sh -c` / `cmd /C`) in the workspace root, capturing
    /// all output. No streaming callbacks. Port of Go `Executor.Run`.
    ///
    /// `cwd` is validated against the workspace root via [`clean_path`] before
    /// the process is spawned; pass `None` (or `""`) to use the workspace root.
    /// `token` cancels the command — on cancel the process group is killed and
    /// [`ShellError::Cancelled`] is returned alongside the partial [`CommandResult`].
    pub async fn run(
        &self,
        token: CancellationToken,
        command: &str,
        cwd: Option<&str>,
    ) -> (CommandResult, Option<ShellError>) {
        self.run_inner(token, command, &[], cwd, None::<fn(&str)>, None::<fn(&str)>)
            .await
    }

    /// Run `command` (via `sh -c` / `cmd /C`) with line-by-line streaming
    /// callbacks. Port of Go `Executor.RunAsync`.
    ///
    /// `on_stdout` / `on_stderr` are invoked once per line (without the
    /// trailing newline) as output is produced. The captured [`CommandResult`]
    /// still accumulates the full (bounded) output for callers that want both.
    pub async fn run_async<Fout, Ferr>(
        &self,
        token: CancellationToken,
        command: &str,
        cwd: Option<&str>,
        on_stdout: Fout,
        on_stderr: Ferr,
    ) -> (CommandResult, Option<ShellError>)
    where
        Fout: FnMut(&str) + Send + 'static,
        Ferr: FnMut(&str) + Send + 'static,
    {
        self.run_inner(token, command, &[], cwd, Some(on_stdout), Some(on_stderr))
            .await
    }

    /// Run `command` with an explicit argument list (no shell wrapping),
    /// streaming output. Port of Go `Executor.RunAsyncArgs`.
    ///
    /// Unlike [`Executor::run_async`], the structured arg list is passed
    /// verbatim to the child — an arg containing spaces, quotes, or shell
    /// metacharacters is not re-interpreted by a shell.
    pub async fn run_async_args<Fout, Ferr>(
        &self,
        token: CancellationToken,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        on_stdout: Fout,
        on_stderr: Ferr,
    ) -> (CommandResult, Option<ShellError>)
    where
        Fout: FnMut(&str) + Send + 'static,
        Ferr: FnMut(&str) + Send + 'static,
    {
        self.run_inner(token, command, args, cwd, Some(on_stdout), Some(on_stderr))
            .await
    }

    /// Shared core for `run` / `run_async` / `run_async_args`.
    ///
    /// `args` non-empty ⇒ direct exec (no shell). `args` empty ⇒ `sh -c` /
    /// `cmd /C`. `on_stdout`/`on_stderr` `None` ⇒ capture-only (no per-line
    /// callback). Returns `(CommandResult, Option<ShellError>)` so callers
    /// always receive the partial output even on cancellation.
    async fn run_inner<Fout, Ferr>(
        &self,
        token: CancellationToken,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        on_stdout: Option<Fout>,
        on_stderr: Option<Ferr>,
    ) -> (CommandResult, Option<ShellError>)
    where
        Fout: FnMut(&str) + Send + 'static,
        Ferr: FnMut(&str) + Send + 'static,
    {
        if command.is_empty() {
            return (CommandResult::default(), Some(ShellError::EmptyCommand));
        }

        // Validate / resolve the CWD against the workspace root. An empty cwd
        // means "use the workspace root itself".
        let dir = match resolve_cwd(&self.workspace_path, cwd) {
            Ok(d) => d,
            Err(e) => return (CommandResult::default(), Some(ShellError::InvalidCwd(e))),
        };

        let mut cmd = build_command(command, args);
        // Dropping the run future (for example, when its caller times out)
        // must also stop the immediate child rather than detaching it.
        cmd.kill_on_drop(true);
        cmd.current_dir(&dir);
        if let Some(env) = &self.env {
            cmd.env_clear();
            for (k, v) in env {
                cmd.env(k, v);
            }
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // Process-group setup is platform-specific (see `crate::procutil`).
        configure_process_group(cmd.as_std_mut());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return (CommandResult::default(), Some(ShellError::Spawn(e))),
        };

        // Save the PID before any mutable borrow (child.wait()) so we can
        // kill the process group by PID during cancellation without needing
        // &mut child (which is held by the wait future).
        let pid = child.id();
        // `kill_on_drop` only terminates `child`. Keep a Unix-specific cleanup
        // guard for the full run so dropping this future also kills its process
        // group and cannot leave grandchildren behind.
        let mut process_group_cleanup = ProcessGroupCleanup::new(pid);

        // Take the pipes before awaiting so we own the readers.
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Shared, thread-safe accumulation so the spawned reader tasks and
        // the final return can both touch the buffers.
        let out_buf = Arc::new(Mutex::new(String::new()));
        let err_buf = Arc::new(Mutex::new(String::new()));
        let cap = self.max_output_bytes;

        // Spawn the readers as independent tasks so they make progress while
        // we await the child's exit. This requires the callbacks to be
        // `Send + 'static` — standard for an async daemon whose callbacks
        // typically capture `Arc`-based channels or event senders.
        let stdout_task = {
            let token = token.clone();
            let out_buf = Arc::clone(&out_buf);
            tokio::spawn(async move {
                if let Some(pipe) = stdout {
                    read_stream(pipe, cap, &out_buf, on_stdout, token).await;
                }
            })
        };
        let stderr_task = {
            let token = token.clone();
            let err_buf = Arc::clone(&err_buf);
            tokio::spawn(async move {
                if let Some(pipe) = stderr {
                    read_stream(pipe, cap, &err_buf, on_stderr, token).await;
                }
            })
        };

        // Race the child's exit against cancellation and the per-command
        // timeout. On either firing we kill the process group by PID (saved
        // above) — this avoids borrowing `child` while the wait future holds
        // `&mut child`. By putting `child.wait()` directly in the select (not
        // pre-pinned), the macro owns the future and drops it when the stop
        // branch wins, releasing the borrow.
        let (wait_status_opt, cancelled) = tokio::select! {
            biased; // poll the stop signal first so a concurrent cancel/timeout wins.
            // Combine cancellation and the per-command deadline into a single
            // "stop" future so the select stays two-way.
            () = async {
                tokio::select! {
                    () = token.cancelled() => {},
                    () = command_deadline(self.command_timeout) => {},
                }
            } => {
                // Kill the process group by PID. On Unix this sends SIGKILL
                // to the entire group (setpgid was called in pre_exec); on
                // Windows the Job Object handle (held by
                // `process_group_cleanup`) kills the tree when dropped. We
                // also kill the immediate child after the select on Windows
                // when the borrow is released.
                #[cfg(unix)]
                if let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) {
                    kill_process_group(pid);
                    // Best-effort: also kill descendants that may have escaped
                    // the process group via setsid() (new session). Linux-only
                    // via /proc walk; macOS relies on the group kill alone.
                    #[cfg(target_os = "linux")]
                    kill_process_tree(pid);
                }
                // Return a sentinel; the real wait happens after the select
                // drops the wait future and frees child for a fresh borrow.
                (None::<std::io::Result<std::process::ExitStatus>>, true)
            }
            status = child.wait() => (Some(status), false),
        };

        // After the select, the wait future is dropped and child is free.
        let wait_status = if cancelled {
            #[cfg(windows)]
            {
                // On Windows we couldn't kill inside the select (no &mut
                // child). Kill the immediate child now, then reap.
                let _ = child.kill().await;
            }
            child.wait().await
        } else {
            // Non-cancelled: wait_status_opt is Some(result).
            wait_status_opt.unwrap_or_else(|| {
                // Defensive: should be unreachable (only cancel arm returns None).
                Err(std::io::Error::other("no wait status"))
            })
        };

        // Drain the readers so the buffers are complete before we read them.
        // After the child exits (or is killed) the pipes hit EOF and the
        // reader tasks complete; awaiting them guarantees full buffers.
        let _ = stdout_task.await;
        let _ = stderr_task.await;

        let stdout = out_buf
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let stderr = err_buf
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();

        let mut result = CommandResult {
            stdout,
            stderr,
            exit_code: 0,
            signal: None,
        };

        match wait_status {
            Ok(status) => {
                // The child has exited normally (or was reaped after
                // cancellation), so a future drop can no longer leak this
                // process group.
                process_group_cleanup.disarm();
                result.exit_code = status.code().unwrap_or(-1);
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    if let Some(sig) = status.signal() {
                        result.signal = Some(signal_name(sig));
                    }
                }
                if cancelled {
                    // The token fired; even if the wait eventually returned a
                    // status, treat it as a cancellation for caller semantics.
                    return (result, Some(ShellError::Cancelled));
                }
                (result, None)
            }
            Err(e) => {
                // wait() failure is rare (e.g. ECHILD after kill). Surface it.
                result.exit_code = -1;
                if !result.stderr.is_empty() {
                    result.stderr.push('\n');
                }
                result.stderr.push_str(&e.to_string());
                (result, Some(ShellError::Io(e)))
            }
        }
    }
}

/// Resolve `cwd` (relative to the workspace root) to an absolute directory,
/// enforcing containment via [`clean_path`] and on-disk symlink resolution via
/// [`resolve_symlink`].
///
/// `None` or `""` returns the workspace root itself. A traversal attempt
/// (`../../etc`) or absolute path (`/etc`) is rejected. A symlink that points
/// outside the workspace (e.g. `ln -s /etc ./etc` then `cwd="etc"`) is also
/// rejected — `clean_path` is lexical only, so [`resolve_symlink`] is layered
/// on top to defend against symlink-based escapes.
fn resolve_cwd(
    workspace_root: &Path,
    cwd: Option<&str>,
) -> std::result::Result<PathBuf, PathError> {
    let cwd = cwd.unwrap_or("").trim();
    if cwd.is_empty() {
        // Use the workspace root directly. Canonicalise so the spawned
        // process sees the real path (handles symlinked roots).
        return Ok(
            std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf())
        );
    }
    let path = clean_path(workspace_root, cwd)?;
    // Layer on-disk symlink resolution so a CWD like "etc" (where ./etc →
    // /etc) cannot bypass workspace containment. clean_path is lexical only.
    resolve_symlink(workspace_root, &path)
}

/// Resolve the per-command deadline into a future. `None` ⇒ no timeout (the
/// future never resolves, so only cancellation can stop the command).
async fn command_deadline(timeout: Option<Duration>) {
    match timeout {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending::<()>().await,
    }
}

/// Build the OS-specific shell invocation.
///
/// `args` non-empty ⇒ direct exec of `command args...` (no shell wrapping).
/// `args` empty ⇒ `sh -c command` (Unix) / `cmd /C command` (Windows).
fn build_command(command: &str, args: &[&str]) -> Command {
    if !args.is_empty() {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd
    } else {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(command);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(command);
            cmd
        }
    }
}

/// Read a child stdout/stderr pipe to completion, line-by-line.
///
/// - Appends each line (with its newline) to `buf`, capped at `cap` bytes.
///   Once the cap is reached further reads are discarded (the stream is still
///   drained to EOF so the child doesn't block on a full pipe).
/// - Invokes `on_line` for each line (without the trailing newline) if the
///   caller supplied a callback.
/// - Stops early if `token` fires (the caller is already killing the group).
async fn read_stream<R, F>(
    pipe: R,
    cap: usize,
    buf: &Arc<Mutex<String>>,
    mut on_line: Option<F>,
    token: CancellationToken,
) where
    R: tokio::io::AsyncRead + Unpin,
    F: FnMut(&str),
{
    let mut reader = BufReader::new(pipe);
    let mut line = String::new();
    loop {
        // tokio::select! on the line read vs cancellation so we don't block
        // forever on a quiet pipe when the token fires.
        let read = tokio::select! {
            biased;
            () = token.cancelled() => break,
            r = reader.read_line(&mut line) => r,
        };

        match read {
            Ok(0) => break, // EOF
            Ok(_n) => {
                // Strip the trailing newline for the callback; keep it in buf.
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if let Some(cb) = on_line.as_mut() {
                    cb(trimmed);
                }
                append_capped(buf, cap, &line);
                line.clear();
            }
            Err(_) => break, // pipe closed / EIO — stop reading
        }
    }

    // Flush any trailing bytes without a newline (rare; e.g. `printf` w/o \n).
    if !line.is_empty() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(cb) = on_line.as_mut() {
            cb(trimmed);
        }
        append_capped(buf, cap, &line);
    }
}

/// Append `chunk` to the shared buffer, truncating at `cap` bytes total.
/// `cap == 0` drops everything — matching `RetainedOutput`'s `limit == 0`
/// "discard" convention so an agent-supplied 0 can never disable the cap and
/// exhaust daemon memory.
fn append_capped(buf: &Arc<Mutex<String>>, cap: usize, chunk: &str) {
    if cap == 0 || chunk.is_empty() {
        return;
    }
    let mut guard = buf.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.len() < cap {
        let remaining = cap.saturating_sub(guard.len());
        if remaining >= chunk.len() {
            guard.push_str(chunk);
        } else {
            // Push as much as fits (char boundary safe via floor_char_boundary).
            let take = chunk.floor_char_boundary(remaining);
            guard.push_str(&chunk[..take]);
        }
    }
}

/// Environment keys that are safe to inherit from the daemon process into
/// agent-spawned commands. Everything else is dropped so daemon secrets
/// (provider API keys, `DEVIN_API_KEY`, `LOCAL_AGENT_*`, etc.) never leak to
/// the agent's child processes.
const SAFE_INHERIT_ENV_KEYS: &[&str] = &["PATH", "HOME", "USER", "SHELL", "LANG", "TERM"];

/// Environment keys (or prefixes) that an agent must not set on commands it
/// spawns, because they can hijack execution of the child process
/// (`LD_PRELOAD`, `DYLD_*`, interpreter startup files, `PATH` shenanigans,
/// etc.). These are stripped from the agent-supplied env before merging.
const BLOCKED_AGENT_ENV_KEYS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_FORCE_FLAT_NAMESPACE",
    "IFS",
    "BASH_ENV",
    "ENV",
    "PERL5OPT",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "RUBYOPT",
];

/// Whether a key matches a blocked agent env entry. Blocked entries are matched
/// exactly or as a prefix (e.g. `DYLD_*`), so an agent cannot smuggle a
/// hijack var past the filter by suffixing it.
fn is_blocked_agent_env_key(key: &str) -> bool {
    BLOCKED_AGENT_ENV_KEYS
        .iter()
        .any(|blocked| key == *blocked || key.starts_with(&format!("{}_", blocked)))
}

/// Filter the daemon's own environment down to a minimal allowlist before it is
/// passed to agent-spawned commands. This prevents secrets and daemon-specific
/// vars (provider API keys, `DEVIN_*`, `LOCAL_AGENT_*`, etc.) from leaking into
/// the agent's child processes. `LC_*` locale vars are allowed through because
/// they are benign and expected by locale-aware tools.
pub fn filter_daemon_env<I, K, V>(vars: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: Into<String>,
{
    vars.into_iter()
        .filter(|(k, _)| {
            let k = k.as_ref();
            SAFE_INHERIT_ENV_KEYS.contains(&k) || k.starts_with("LC_")
        })
        .map(|(k, v)| (k.as_ref().to_string(), v.into()))
        .collect()
}

/// Strip dangerous env keys from the agent-supplied env before it is merged
/// onto the (already filtered) daemon env. This stops an agent from hijacking
/// its child processes via `LD_PRELOAD`, `DYLD_*`, interpreter startup hooks,
/// or similar execution-redirection variables.
pub fn filter_agent_env<I, K, V>(vars: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: Into<String>,
{
    vars.into_iter()
        .filter(|(k, _)| !is_blocked_agent_env_key(k.as_ref()))
        .map(|(k, v)| (k.as_ref().to_string(), v.into()))
        .collect()
}

/// Overlay `extra` on top of `base`, with `extra` winning for duplicate keys.
///
/// Port of Go `MergeEnv`. The returned vec has no duplicate keys and preserves
/// insertion order (base order first, then new extra keys in their order).
/// `base` and `extra` may be different iterator types (e.g. `std::env::vars`
/// for base and a slice for extra).
pub fn merge_env<I1, I2, K1, V1, K2, V2>(base: I1, extra: I2) -> Vec<(String, String)>
where
    I1: IntoIterator<Item = (K1, V1)>,
    I2: IntoIterator<Item = (K2, V2)>,
    K1: Into<String>,
    V1: Into<String>,
    K2: Into<String>,
    V2: Into<String>,
{
    let mut merged: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    let insert = |merged: &mut Vec<(String, String)>,
                  seen: &mut std::collections::HashMap<String, usize>,
                  k: String,
                  v: String| {
        if let Some(&idx) = seen.get(&k) {
            merged[idx].1 = v;
        } else {
            seen.insert(k.clone(), merged.len());
            merged.push((k, v));
        }
    };

    for (k, v) in base {
        insert(&mut merged, &mut seen, k.into(), v.into());
    }
    for (k, v) in extra {
        insert(&mut merged, &mut seen, k.into(), v.into());
    }
    merged
}

/// Map a Unix signal number to its uppercase name (e.g. `9` → `"KILLED"`).
/// Falls back to `"SIG<n>"` for unknown signals.
#[cfg(unix)]
fn signal_name(sig: i32) -> String {
    match sig {
        libc::SIGHUP => "HUP".to_string(),
        libc::SIGINT => "INT".to_string(),
        libc::SIGQUIT => "QUIT".to_string(),
        libc::SIGKILL => "KILLED".to_string(),
        libc::SIGTERM => "TERMINATED".to_string(),
        libc::SIGSEGV => "SEGV".to_string(),
        libc::SIGABRT => "ABRT".to_string(),
        _ => format!("SIG{sig}"),
    }
}

#[cfg(not(unix))]
#[allow(dead_code)] // unused on non-unix, non-windows targets (e.g. tests)
fn signal_name(_sig: i32) -> String {
    String::from("UNKNOWN")
}

#[cfg(test)]
mod tests;
