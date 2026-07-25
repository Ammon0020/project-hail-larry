use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalRequest, KillTerminalResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, TerminalExitStatus, TerminalOutputRequest,
    TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::HandlerDeps;
use crate::interfaces::AppError;
use crate::shell::{
    filter_agent_env, filter_daemon_env, merge_env, Executor, DEFAULT_MAX_OUTPUT_BYTES,
};

/// Maximum terminal records retained per ACP session.
const MAX_TERMINALS_PER_SESSION: usize = 16;
/// Maximum output retained for an ACP terminal when the agent gives no lower limit.
const MAX_TERMINAL_OUTPUT_BYTES: usize = DEFAULT_MAX_OUTPUT_BYTES;

pub(in crate::acp::core) type TerminalRegistry = Arc<Mutex<HashMap<String, Arc<TerminalState>>>>;

/// State shared by callback requests for one ACP terminal.
///
/// The standard mutex is intentionally used only for short, synchronous state
/// updates. Terminal waits observe the watch channel outside the lock.
pub(in crate::acp::core) struct TerminalState {
    cancel: CancellationToken,
    output: Mutex<RetainedOutput>,
    exit: watch::Sender<Option<TerminalExitStatus>>,
}

/// Bounded terminal output that discards the oldest complete UTF-8 prefix.
struct RetainedOutput {
    text: String,
    limit: usize,
    truncated: bool,
}

impl RetainedOutput {
    fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
            truncated: false,
        }
    }

    fn push_line(&mut self, line: &str) {
        if self.limit == 0 {
            // Each callback invocation represents at least a newline, even
            // when the emitted line is empty.
            self.truncated = true;
            return;
        }
        self.text.push_str(line);
        self.text.push('\n');
        if self.text.len() > self.limit {
            let excess = self.text.len() - self.limit;
            let start = self.text.ceil_char_boundary(excess);
            self.text.drain(..start);
            self.truncated = true;
        }
    }
}

/// Create an ACP terminal and start its command without delaying the response.
///
/// The command is gated on an explicit permission prompt so a malicious agent
/// cannot bypass `request_permission` and spawn arbitrary commands directly.
/// The approved action is bound to the exact argv/cwd/env that is executed.
pub(in crate::acp::core) async fn create_terminal(
    deps: HandlerDeps,
    request: CreateTerminalRequest,
) -> Result<CreateTerminalResponse, AppError> {
    if deps.cancellation.is_cancelled() {
        return Err(AppError::internal("ACP session is closing"));
    }
    let cwd = terminal_cwd(&deps.workspace_path, request.cwd.as_deref())?;
    let limit = request
        .output_byte_limit
        .map_or(MAX_TERMINAL_OUTPUT_BYTES, |limit| {
            usize::try_from(limit)
                .unwrap_or(MAX_TERMINAL_OUTPUT_BYTES)
                .min(MAX_TERMINAL_OUTPUT_BYTES)
        });

    // Build a display string carrying the full argv, cwd, and env so the user
    // can make an informed approval decision and the policy key discriminates
    // on the exact executed command (target stays empty for execute tools).
    let command_display = {
        let mut parts = vec![request.command.clone()];
        parts.extend(request.args.iter().cloned());
        let mut display = parts.join(" ");
        if let Some(cwd) = cwd.as_deref() {
            display.push_str(&format!(" (cwd: {cwd})"));
        }
        if !request.env.is_empty() {
            let env_pairs: Vec<(String, String)> = request
                .env
                .iter()
                .map(|variable| (variable.name.clone(), variable.value.clone()))
                .collect();
            display.push_str(&format!(" (env: {env_pairs:?})"));
        }
        display
    };
    let permission = crate::interfaces::PermissionRequest {
        id: Uuid::new_v4().to_string(),
        // Agent session IDs are protocol transport identifiers. Permissions
        // belong to the local lifecycle entry so close clears its exact
        // pending prompts and durable policies.
        session_id: deps.local_session_id.clone(),
        tool: "create_terminal".to_string(),
        tool_kind: "execute".to_string(),
        command: command_display,
        target: String::new(),
        options: Vec::new(),
        option_details: Vec::new(),
    };
    // Gate the spawn on a real permission decision before executing anything.
    let decision = deps.permissions.request(permission).await?;
    if matches!(
        decision,
        crate::interfaces::PermissionDecision::Deny
            | crate::interfaces::PermissionDecision::RejectAlways
    ) {
        return Err(AppError::Forbidden(
            "terminal creation denied by permission".to_string(),
        ));
    }

    let terminal_id = format!("term-{}", Uuid::new_v4().simple());
    let (exit, _) = watch::channel(None);
    let state = Arc::new(TerminalState {
        cancel: deps.cancellation.child_token(),
        output: Mutex::new(RetainedOutput::new(limit)),
        exit,
    });
    {
        let mut terminals = deps
            .terminals
            .lock()
            .map_err(|_| AppError::internal("ACP terminal registry lock poisoned"))?;
        if terminals.len() >= MAX_TERMINALS_PER_SESSION {
            return Err(AppError::internal("ACP terminal capacity exceeded"));
        }
        terminals.insert(terminal_id.clone(), Arc::clone(&state));
    }

    // Only inherit a minimal allowlist of safe vars from the daemon so secrets
    // (provider keys, DEVIN_*, LOCAL_AGENT_*, etc.) don't leak to agent-spawned
    // commands, and strip dangerous hijack vars (LD_PRELOAD, DYLD_*, etc.) from
    // the agent-supplied env before merging.
    let env = merge_env(
        filter_daemon_env(std::env::vars()),
        filter_agent_env(
            request
                .env
                .iter()
                .map(|variable| (variable.name.clone(), variable.value.clone())),
        ),
    );
    let command = request.command;
    let args = request.args;
    let executor = Executor::new(&deps.workspace_path)
        .with_env(env)
        .with_max_output_bytes(limit);
    tokio::spawn(async move {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let stdout_state = Arc::clone(&state);
        let stderr_state = Arc::clone(&state);
        let (result, error) = executor
            .run_async_args(
                state.cancel.clone(),
                &command,
                &args,
                cwd.as_deref(),
                move |line| append_terminal_output(&stdout_state, line),
                move |line| append_terminal_output(&stderr_state, line),
            )
            .await;
        if let Some(error) = error {
            // Commands, argv, and environment values may contain credentials;
            // keep the diagnostic category without recording their contents.
            tracing::warn!(error = %error, "ACP terminal command ended abnormally");
        }
        let status = TerminalExitStatus::new()
            .exit_code((result.exit_code >= 0).then_some(result.exit_code as u32))
            .signal(result.signal);
        state.exit.send_replace(Some(status));
    });
    Ok(CreateTerminalResponse::new(terminal_id))
}

/// Return a snapshot of terminal output without waiting for the command.
pub(in crate::acp::core) fn terminal_output(
    deps: HandlerDeps,
    request: TerminalOutputRequest,
) -> Result<TerminalOutputResponse, AppError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    let output = terminal
        .output
        .lock()
        .map_err(|_| AppError::internal("ACP terminal output lock poisoned"))?;
    let exit_status = terminal.exit.borrow().clone();
    Ok(TerminalOutputResponse::new(output.text.clone(), output.truncated).exit_status(exit_status))
}

/// Wait asynchronously for an owned terminal to exit.
pub(in crate::acp::core) async fn wait_for_terminal_exit(
    deps: HandlerDeps,
    request: WaitForTerminalExitRequest,
) -> Result<WaitForTerminalExitResponse, AppError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    let mut exit = terminal.exit.subscribe();
    loop {
        if let Some(status) = exit.borrow().clone() {
            return Ok(WaitForTerminalExitResponse::new(status));
        }
        exit.changed()
            .await
            .map_err(|_| AppError::internal("ACP terminal exited without a status"))?;
    }
}

/// Cancel a terminal while retaining its output for subsequent inspection.
pub(in crate::acp::core) fn kill_terminal(
    deps: HandlerDeps,
    request: KillTerminalRequest,
) -> Result<KillTerminalResponse, AppError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    terminal.cancel.cancel();
    Ok(KillTerminalResponse::new())
}

/// Cancel and remove a terminal, releasing its registry-owned resources.
pub(in crate::acp::core) fn release_terminal(
    deps: HandlerDeps,
    request: ReleaseTerminalRequest,
) -> Result<ReleaseTerminalResponse, AppError> {
    let terminal = deps
        .terminals
        .lock()
        .map_err(|_| AppError::internal("ACP terminal registry lock poisoned"))?
        .remove(&request.terminal_id.to_string())
        .ok_or_else(|| AppError::not_found_id("terminal", &request.terminal_id.to_string()))?;
    terminal.cancel.cancel();
    Ok(ReleaseTerminalResponse::new())
}

fn terminal_state(
    registry: &TerminalRegistry,
    terminal_id: &str,
) -> Result<Arc<TerminalState>, AppError> {
    registry
        .lock()
        .map_err(|_| AppError::internal("ACP terminal registry lock poisoned"))?
        .get(terminal_id)
        .cloned()
        .ok_or_else(|| AppError::not_found_id("terminal", terminal_id))
}

fn append_terminal_output(state: &TerminalState, line: &str) {
    if let Ok(mut output) = state.output.lock() {
        output.push_line(line);
    }
}

/// Cancel every terminal when its owning ACP session disconnects.
pub(in crate::acp::core) fn cancel_terminals(registry: &TerminalRegistry) {
    if let Ok(mut terminals) = registry.lock() {
        for terminal in terminals.values() {
            terminal.cancel.cancel();
        }
        terminals.clear();
    }
}

/// Validate the ACP-required absolute CWD and translate it for Executor.
fn terminal_cwd(root: &Path, cwd: Option<&Path>) -> Result<Option<String>, AppError> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    if !cwd.is_absolute() {
        return Err(AppError::validation(
            "terminal cwd must be an absolute path within the workspace",
        ));
    }
    let root = std::fs::canonicalize(root)
        .map_err(|error| AppError::internal(format!("canonicalize workspace: {error}")))?;
    let cwd = std::fs::canonicalize(cwd)
        .map_err(|error| AppError::validation(format!("invalid terminal cwd: {error}")))?;
    if !cwd.is_dir() {
        return Err(AppError::validation("terminal cwd is not a directory"));
    }
    let relative = cwd
        .strip_prefix(&root)
        .map_err(|_| AppError::validation("terminal cwd is outside the workspace"))?;
    if relative.as_os_str().is_empty() {
        Ok(None)
    } else {
        relative
            .to_str()
            .map(|path| Some(path.to_string()))
            .ok_or_else(|| AppError::validation("terminal cwd is not valid Unicode"))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::terminal_cwd;
    use super::RetainedOutput;

    #[test]
    fn retained_terminal_output_truncates_at_utf8_boundary() {
        let mut output = RetainedOutput::new(5);
        output.push_line("éé");
        output.push_line("x");

        assert!(output.truncated);
        assert!(output.text.len() <= 5);
        assert!(std::str::from_utf8(output.text.as_bytes()).is_ok());
    }

    #[test]
    fn terminal_cwd_rejects_workspace_path_escapes() {
        let workspace = TempDir::new().expect("workspace");
        let outside = TempDir::new().expect("outside path");

        let error = terminal_cwd(workspace.path(), Some(outside.path()))
            .expect_err("outside cwd must be rejected");
        assert!(error.to_string().contains("outside the workspace"));
    }
}
