//! Tests for the workspace subprocess runner (port of `shell_test.go`).
//!
//! These mirror the Go test cases and add the S-SHELL acceptance criteria:
//! CWD enforcement, line-by-line streaming, exit codes, cancellation (process
//! group kill), path-traversal rejection, timeout handling, and bounded
//! output. All tests use `tempfile` for isolation so they never touch the
//! developer's real filesystem.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use super::*;

/// Collect streamed lines into a shared vec via a callback closure.
fn line_collector() -> (
    Arc<StdMutex<Vec<String>>>,
    impl FnMut(&str) + Send + 'static,
) {
    let lines: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let l = Arc::clone(&lines);
    let cb = move |s: &str| l.lock().unwrap().push(s.to_string());
    (lines, cb)
}

/// `echo hello` runs and returns stdout "hello" with exit code 0.
#[tokio::test]
async fn run_echo() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path().canonicalize().unwrap());
    let (result, err) = exec.run(CancellationToken::new(), "echo hello", None).await;
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello");
}

/// An empty command returns `ShellError::EmptyCommand` and never spawns.
#[tokio::test]
async fn run_empty_command() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let (_, err) = exec.run(CancellationToken::new(), "", None).await;
    assert!(matches!(err, Some(ShellError::EmptyCommand)));
}

/// Non-zero exit codes are captured into `CommandResult::exit_code`.
#[tokio::test]
async fn run_exit_code() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let command = if cfg!(windows) { "exit /b 1" } else { "exit 1" };
    let (result, _) = exec.run(CancellationToken::new(), command, None).await;
    assert_ne!(result.exit_code, 0, "expected non-zero exit code");
    assert_eq!(result.exit_code, 1);
}

/// The command runs in the workspace directory (pwd/cd output contains it).
#[tokio::test]
async fn run_working_directory() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let exec = Executor::new(root.clone());
    let command = if cfg!(windows) { "cd" } else { "pwd" };
    let (result, err) = exec.run(CancellationToken::new(), command, None).await;
    assert!(err.is_none(), "run: {err:?}");
    let out = result.stdout.trim().to_lowercase();
    assert!(
        out.contains(&root.to_string_lossy().to_lowercase()),
        "expected output to contain {}, got {}",
        root.display(),
        out
    );
}

/// A caller-supplied relative `cwd` is honoured and contained.
#[tokio::test]
async fn run_with_relative_cwd() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("subdir")).unwrap();
    let exec = Executor::new(root.clone());
    let command = if cfg!(windows) { "cd" } else { "pwd" };
    let (result, err) = exec
        .run(CancellationToken::new(), command, Some("subdir"))
        .await;
    assert!(err.is_none(), "run: {err:?}");
    let out = result.stdout.trim().to_lowercase();
    assert!(
        out.contains(&root.join("subdir").to_string_lossy().to_lowercase()),
        "expected output to contain subdir, got {out}"
    );
}

/// Path traversal in `cwd` is rejected before the process is spawned.
#[tokio::test]
async fn run_rejects_path_traversal_cwd() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let exec = Executor::new(root);
    let (_, err) = exec
        .run(CancellationToken::new(), "pwd", Some("../../etc/passwd"))
        .await;
    assert!(
        matches!(err, Some(ShellError::InvalidCwd(_))),
        "expected InvalidCwd, got {err:?}"
    );
}

/// An absolute `cwd` outside the workspace is rejected.
#[tokio::test]
async fn run_rejects_absolute_cwd() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let exec = Executor::new(root);
    let target = if cfg!(windows) { "C:\\Windows" } else { "/etc" };
    let (_, err) = exec
        .run(CancellationToken::new(), "pwd", Some(target))
        .await;
    assert!(
        matches!(err, Some(ShellError::InvalidCwd(_))),
        "expected InvalidCwd, got {err:?}"
    );
}

/// Async execution streams stdout line-by-line via the callback.
#[tokio::test]
async fn run_async_streams_stdout() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let (lines, cb) = line_collector();
    let (result, err) = exec
        .run_async(CancellationToken::new(), "echo streaming", None, cb, |_| {})
        .await;
    assert!(err.is_none(), "run async: {err:?}");
    assert_eq!(result.exit_code, 0);
    let got = lines.lock().unwrap();
    assert!(!got.is_empty(), "expected at least one stdout line");
    assert_eq!(got[0], "streaming");
}

/// Both stdout and stderr are streamed to their respective callbacks.
#[tokio::test]
async fn run_async_streams_stdout_and_stderr() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let (out_lines, out_cb) = line_collector();
    let (err_lines, err_cb) = line_collector();
    let command = "echo to-stdout; echo to-stderr 1>&2";
    let (result, err) = exec
        .run_async(CancellationToken::new(), command, None, out_cb, err_cb)
        .await;
    assert!(err.is_none(), "run async: {err:?}");
    assert_eq!(result.exit_code, 0);

    let out = out_lines.lock().unwrap();
    let errl = err_lines.lock().unwrap();
    assert!(out.iter().any(|s| s == "to-stdout"), "stdout: {out:?}");
    assert!(errl.iter().any(|s| s == "to-stderr"), "stderr: {errl:?}");
}

/// `run_async_args` passes the structured arg list verbatim (no shell
/// re-parsing), so args with spaces/metachars survive intact.
#[tokio::test]
async fn run_async_args_preserves_arg_quoting() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    // `printf` with a single arg containing spaces and a shell metachar —
    // under `sh -c` this would be re-parsed; via args it is verbatim.
    let arg = "hello; world | $HOME";
    let (result, err) = exec
        .run_async_args(
            CancellationToken::new(),
            "printf",
            &["%s\\n", arg],
            None,
            |_| {},
            |_| {},
        )
        .await;
    assert!(err.is_none(), "run: {err:?}");
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), arg);
}

/// Env vars supplied via `with_env` are visible to the spawned process and
/// replace (not augment) the inherited environment.
#[tokio::test]
async fn run_with_env_replaces_environment() {
    let dir = TempDir::new().unwrap();
    // Build env = inherited + ACP_TEST_VAR so PATH etc. are present.
    let env = merge_env(std::env::vars(), [("ACP_TEST_VAR", "from-agent")]);
    let exec = Executor::new(dir.path()).with_env(env);
    let command = if cfg!(windows) { "cmd" } else { "sh" };
    let args: &[&str] = if cfg!(windows) {
        &["/C", "echo %ACP_TEST_VAR%"]
    } else {
        &["-c", "printf %s \"$ACP_TEST_VAR\""]
    };
    let (result, err) = exec
        .run_async_args(
            CancellationToken::new(),
            command,
            args,
            None,
            |_| {},
            |_| {},
        )
        .await;
    assert!(err.is_none(), "run: {err:?}");
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "from-agent");
}

/// `merge_env` overlays extra on base with extra winning for duplicates and
/// preserves insertion order for new keys. Port of Go `TestMergeEnv`.
#[test]
fn merge_env_overlays_and_preserves_order() {
    let base = vec![
        ("PATH", "/usr/bin"),
        ("HOME", "/home/user"),
        ("EDITOR", "vi"),
    ];
    let extra = vec![("EDITOR", "nano"), ("FOO", "bar")];
    let got = merge_env(base, extra);
    let want: Vec<(String, String)> = vec![
        ("PATH".into(), "/usr/bin".into()),
        ("HOME".into(), "/home/user".into()),
        ("EDITOR".into(), "nano".into()),
        ("FOO".into(), "bar".into()),
    ];
    assert_eq!(got, want);
}

/// Merging onto an empty base just yields extra, in order.
#[test]
fn merge_env_empty_base() {
    let got = merge_env(Vec::<(&str, &str)>::new(), [("A", "1"), ("B", "2")]);
    let want: Vec<(String, String)> = vec![("A".into(), "1".into()), ("B".into(), "2".into())];
    assert_eq!(got, want);
}

/// Cancellation kills a long-running command and returns `ShellError::Cancelled`.
#[tokio::test]
async fn cancellation_kills_long_running_command() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let token = CancellationToken::new();
    let tok = token.clone();

    // `sleep 30` would outlive the test; cancel it almost immediately.
    let handle = tokio::spawn(async move {
        let command = if cfg!(windows) {
            // ping -n 31 127.0.0.1 > NUL is the Windows long-sleep idiom.
            "ping -n 31 127.0.0.1 > NUL"
        } else {
            "sleep 30"
        };
        exec.run(token, command, None).await
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    tok.cancel();

    let (result, err) = handle.await.unwrap();
    assert!(
        matches!(err, Some(ShellError::Cancelled)),
        "expected Cancelled, got {err:?}"
    );
    // On Unix the killed process reports a signal; on Windows exit code is
    // typically 1. Either way it must not be a clean 0.
    assert!(
        result.signal.is_some() || result.exit_code != 0,
        "expected signal or non-zero exit, got code={} signal={:?}",
        result.exit_code,
        result.signal
    );
}

/// Dropping the run future kills an entire Unix process group, including a
/// backgrounded grandchild that `kill_on_drop` alone would leave running.
#[cfg(unix)]
#[tokio::test]
async fn dropping_run_future_kills_process_group_grandchild() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let pid_file = dir.path().join("grandchild.pid");
    let pid_file_path = pid_file.to_str().expect("temporary path should be UTF-8");
    let args = [
        "-c",
        "sleep 30 & echo $! > \"$1\"; wait",
        "_",
        pid_file_path,
    ];

    let timeout = tokio::time::timeout(
        Duration::from_millis(250),
        exec.run_async_args(CancellationToken::new(), "sh", &args, None, |_| {}, |_| {}),
    )
    .await;
    assert!(timeout.is_err(), "long-running command should time out");

    let pid: i32 = std::fs::read_to_string(&pid_file)
        .expect("child should write its PID before timing out")
        .trim()
        .parse()
        .expect("grandchild PID should be numeric");

    // Allow the drop guard to signal the process group. A zombie has exited
    // and is harmless; it may remain briefly if the test environment's PID 1
    // has not yet reaped the orphaned grandchild.
    let mut exited = false;
    for _ in 0..20 {
        if process_is_gone_or_zombie(pid) {
            exited = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if !exited {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert!(
        exited,
        "grandchild process {pid} survived a dropped run future"
    );

    fn process_is_gone_or_zombie(pid: i32) -> bool {
        if unsafe { libc::kill(pid, 0) } == -1 {
            return std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        }

        // Linux reports process state immediately after the final `)` in
        // `/proc/<pid>/stat`. Treat Z as exited even if it is not reaped yet.
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

/// Cancellation kills the *process group*, not just the immediate child —
/// a grandchild of a shell pipeline cannot survive daemon shutdown.
///
/// We spawn `sh -c 'sleep 30 & sleep 30'` (a backgrounded grandchild) and
/// verify that after cancellation no `sleep` process is left running. This
/// is the orphan-prevention race the story calls out.
#[cfg(unix)]
#[tokio::test]
async fn cancellation_kills_process_group_not_just_child() {
    use std::process::Command as StdCommand;

    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let token = CancellationToken::new();
    let tok = token.clone();

    // Spawn a shell that backgrounds a long-running grandchild. If we only
    // killed the shell, the grandchild `sleep` would survive.
    let handle = tokio::spawn(async move { exec.run(token, "sleep 30 & wait", None).await });

    // Give the shell time to start the backgrounded grandchild.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Snapshot the count of `sleep` processes before cancel.
    let before = count_processes("sleep");
    assert!(
        before >= 1,
        "expected at least one sleep before cancel, got {before}"
    );

    tok.cancel();
    let (_result, err) = handle.await.unwrap();
    assert!(matches!(err, Some(ShellError::Cancelled)));

    // Poll briefly: the SIGKILL to the process group is async; give the
    // kernel a moment to reap the grandchildren.
    let mut after = count_processes("sleep");
    for _ in 0..20 {
        if after == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        after = count_processes("sleep");
    }
    assert_eq!(
        after, 0,
        "orphaned `sleep` process survived cancellation (process group not killed)"
    );

    fn count_processes(name: &str) -> usize {
        // `pgrep -x` matches exact process names. Fall back to 0 if pgrep
        // isn't available (CI without procps).
        let out = StdCommand::new("pgrep").args(["-c", "-x", name]).output();
        match out {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout);
                s.trim().parse::<usize>().unwrap_or(0)
            }
            _ => 0,
        }
    }
}

/// A timeout (token fired after a deadline) cancels the command.
#[tokio::test]
async fn timeout_cancels_command() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let token = CancellationToken::new();

    // Schedule a cancel after 100ms.
    let tok = token.clone();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        tok.cancel();
    });

    let command = if cfg!(windows) {
        "ping -n 31 127.0.0.1 > NUL"
    } else {
        "sleep 30"
    };
    let (result, err) = exec.run(token, command, None).await;
    let _ = timer.await;
    assert!(matches!(err, Some(ShellError::Cancelled)));
    assert!(
        result.signal.is_some() || result.exit_code != 0,
        "expected signal or non-zero exit"
    );
}

/// Bounded output: a command that emits more than `max_output_bytes` is
/// truncated; the cap prevents memory exhaustion.
#[tokio::test]
async fn bounded_output_truncates_at_cap() {
    let dir = TempDir::new().unwrap();
    // 256-byte cap so the test is fast.
    let exec = Executor::new(dir.path()).with_max_output_bytes(256);
    // Emit ~4 KiB of `y` lines. Without the cap this would be 4 KiB in memory.
    let command = if cfg!(windows) {
        // PowerShell-style isn't available via cmd easily; use a small repeat.
        "for /L %i in (1,1,400) do @echo yyyyyyyyyyyyyyyyyyyyyy"
    } else {
        "yes yyyyyyyyyyyyyyyyyyyyyy | head -200"
    };
    let (result, err) = exec.run(CancellationToken::new(), command, None).await;
    assert!(
        err.is_none() || matches!(err, Some(ShellError::Cancelled)),
        "{err:?}"
    );
    assert!(
        result.stdout.len() <= 256,
        "expected stdout <= 256 bytes, got {}",
        result.stdout.len()
    );
    assert!(
        !result.stdout.is_empty(),
        "expected some captured output before the cap"
    );
}

/// A cap of 0 drops all captured output (matching `RetainedOutput`'s
/// `limit == 0` "discard" convention) so an agent-supplied 0 cannot disable
/// the cap and exhaust daemon memory.
#[tokio::test]
async fn zero_cap_drops_all_output() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path()).with_max_output_bytes(0);
    let (result, err) = exec
        .run(CancellationToken::new(), "echo dropped-output", None)
        .await;
    assert!(err.is_none());
    assert!(
        result.stdout.is_empty(),
        "expected no captured output with cap == 0, got {:?}",
        result.stdout
    );
}

/// A non-existent command (direct exec, no shell) returns `ShellError::Spawn`.
#[tokio::test]
async fn spawn_failure_returns_error() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    // Using run_async_args with a non-empty args list bypasses `sh -c` and
    // tries to exec the binary directly, so a non-existent binary fails at
    // spawn time (not at exit). An empty args list would fall through to
    // `sh -c <command>`, which spawns sh successfully.
    let (_, err) = exec
        .run_async_args(
            CancellationToken::new(),
            "this-binary-does-not-exist-xyz-12345",
            &["--dummy-arg"],
            None,
            |_| {},
            |_| {},
        )
        .await;
    assert!(
        matches!(err, Some(ShellError::Spawn(_))),
        "expected Spawn error, got {err:?}"
    );
}

/// A command writing a final line without a trailing newline is still
/// captured and streamed.
#[tokio::test]
async fn trailing_line_without_newline_is_captured() {
    let dir = TempDir::new().unwrap();
    let exec = Executor::new(dir.path());
    let (lines, cb) = line_collector();
    let (result, err) = exec
        .run_async(
            CancellationToken::new(),
            "printf %s no-newline",
            None,
            cb,
            |_| {},
        )
        .await;
    assert!(err.is_none(), "{err:?}");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "no-newline");
    let got = lines.lock().unwrap();
    assert!(got.iter().any(|s| s == "no-newline"), "lines: {got:?}");
}
