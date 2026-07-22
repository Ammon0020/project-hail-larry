//! Shared helpers for harness probes. Keep this free of harness-specific logic
//! so individual agent modules stay independent.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::config::AgentModel;

pub(super) const MAX_PROBE_OUTPUT: usize = 64 * 1024;
pub(super) const MAX_DIAGNOSTIC: usize = 240;

pub(super) fn find_first_command(commands: &[&str], search_paths: &[&str]) -> Option<PathBuf> {
    let path_entries: Vec<PathBuf> = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect())
        .unwrap_or_default();
    for command in commands {
        for directory in &path_entries {
            if let Some(path) = executable_in(directory, command) {
                return Some(path);
            }
        }
    }
    for directory in search_paths.iter().filter_map(|path| expand_path(path)) {
        for command in commands {
            if let Some(path) = executable_in(&directory, command) {
                return Some(path);
            }
        }
    }
    None
}

fn executable_in(directory: &Path, command: &str) -> Option<PathBuf> {
    let candidates = if cfg!(windows) {
        vec![
            command.into(),
            format!("{command}.exe"),
            format!("{command}.cmd"),
        ]
    } else {
        vec![command.into()]
    };
    candidates
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| {
            path.is_file() && {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    path.metadata()
                        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
        })
}

fn expand_path(path: &str) -> Option<PathBuf> {
    let mut expanded = expand_windows_env(path);
    if expanded == "~" || expanded.starts_with("~/") || expanded.starts_with("~\\") {
        let home = dirs::home_dir()?;
        expanded = home.join(&expanded[1..]).to_string_lossy().into_owned();
    }
    Some(PathBuf::from(expanded))
}

pub(super) fn expand_windows_env(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push('%');
            output.push_str(after_start);
            return output;
        };
        let key = &after_start[..end];
        if !key.is_empty()
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            output.push_str(&env::var(key).unwrap_or_default());
        } else {
            output.push('%');
            output.push_str(key);
            output.push('%');
        }
        rest = &after_start[end + 1..];
    }
    output.push_str(rest);
    output
}

pub(super) fn strip_ansi(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' && matches!(chars.next(), Some('[')) {
            for character in chars.by_ref() {
                if character.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
}

pub(super) async fn run_bounded(
    command: &Path,
    args: &[&str],
    duration: Duration,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut child = Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("start failed: {error}"))?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err("open stdout pipe failed".into());
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err("open stderr pipe failed".into());
    };
    let probe = async {
        let (stdout, stderr, status) = tokio::join!(
            read_capped(&mut stdout, MAX_PROBE_OUTPUT),
            read_capped(&mut stderr, MAX_PROBE_OUTPUT),
            child.wait()
        );
        (stdout, stderr, status)
    };
    let result = timeout(duration, probe).await;
    if result.is_err() {
        terminate(&mut child).await;
        return Err("model probe timed out".into());
    }
    let (stdout, stderr, status) = result.map_err(|_| "model probe timed out")?;
    if status
        .map_err(|error| format!("wait failed: {error}"))?
        .success()
    {
        Ok((stdout, stderr))
    } else {
        Err(format!("model probe failed{}", diagnostic_suffix(&stderr)))
    }
}

pub(super) async fn terminate(child: &mut Child) {
    if child.id().is_some() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
}

pub(super) fn diagnostic_suffix(stderr: &[u8]) -> String {
    let diagnostic = String::from_utf8_lossy(stderr);
    let redacted = diagnostic
        .lines()
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            !line.contains("token") && !line.contains("password") && !line.contains("api_key")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let diagnostic = redacted.trim();
    if diagnostic.is_empty() {
        String::new()
    } else {
        format!(" (agent stderr: {})", truncate(diagnostic, MAX_DIAGNOSTIC))
    }
}

fn truncate(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let output: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{output}…")
    } else {
        output
    }
}

pub(super) async fn read_capped<R: AsyncRead + Unpin>(reader: &mut R, limit: usize) -> Vec<u8> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let Ok(count) = reader.read(&mut chunk).await else {
            return output;
        };
        if count == 0 {
            return output;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&chunk[..count.min(remaining)]);
    }
}

pub(super) async fn read_line_capped<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let count = reader
            .read(&mut byte)
            .await
            .map_err(|error| format!("read response: {error}"))?;
        if count == 0 {
            return Err("ACP peer disconnected".into());
        }
        if byte[0] == b'\n' {
            return Ok(output);
        }
        if output.len() == limit {
            return Err("ACP response exceeded limit".into());
        }
        output.push(byte[0]);
    }
}

pub(super) async fn write_json(
    stdin: &mut tokio::process::ChildStdin,
    value: Value,
) -> Result<(), String> {
    let mut bytes =
        serde_json::to_vec(&value).map_err(|error| format!("encode request: {error}"))?;
    bytes.push(b'\n');
    stdin
        .write_all(&bytes)
        .await
        .map_err(|error| format!("write request: {error}"))?;
    stdin
        .flush()
        .await
        .map_err(|error| format!("flush request: {error}"))
}

pub(super) async fn wait_for_response(
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    expected_id: i64,
) -> Result<Value, String> {
    loop {
        let line = read_line_capped(stdout, MAX_PROBE_OUTPUT).await?;
        let value: Value = serde_json::from_slice(&line)
            .map_err(|error| format!("invalid ACP response: {error}"))?;
        if value.get("id").and_then(Value::as_i64) != Some(expected_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            if error.get("code").and_then(Value::as_i64) == Some(-32601) {
                return Err("method not supported".into());
            }
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("ACP request failed");
            return Err(message.into());
        }
        return Ok(value);
    }
}

/// Spawn an ACP stdio child, run `exchange`, then always terminate the process.
pub(super) async fn with_acp_child<F, Fut, T>(
    command: &Path,
    args: &[&str],
    duration: Duration,
    exchange: F,
) -> Result<T, String>
where
    F: FnOnce(tokio::process::ChildStdin, tokio::process::ChildStdout) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut child = Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("start failed: {error}"))?;
    let Some(stdin) = child.stdin.take() else {
        return Err("open stdin pipe failed".into());
    };
    let Some(stdout) = child.stdout.take() else {
        return Err("open stdout pipe failed".into());
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err("open stderr pipe failed".into());
    };
    let stderr_task = tokio::spawn(async move { read_capped(&mut stderr, MAX_PROBE_OUTPUT).await });
    let result = timeout(duration, exchange(stdin, stdout)).await;
    terminate(&mut child).await;
    let stderr = stderr_task.await.unwrap_or_default();
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(format!("{}{}", error, diagnostic_suffix(&stderr))),
        Err(_) => Err("ACP probe timed out".into()),
    }
}

pub(super) async fn probe_providers(
    command: &Path,
    args: &[&str],
    duration: Duration,
) -> Result<Vec<AgentModel>, String> {
    with_acp_child(command, args, duration, |mut stdin, stdout| async move {
        write_json(
            &mut stdin,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": 1,
                    "clientInfo": {"name": "local-agent-autodetect", "version": "1.0"},
                    "clientCapabilities": {}
                }
            }),
        )
        .await?;
        let mut stdout = BufReader::new(stdout);
        wait_for_response(&mut stdout, 1).await?;
        write_json(
            &mut stdin,
            json!({"jsonrpc": "2.0", "id": 2, "method": "providers/list", "params": {}}),
        )
        .await?;
        let response = wait_for_response(&mut stdout, 2).await?;
        Ok(response
            .get("result")
            .and_then(|result| result.get("providers"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|provider| provider.get("id").and_then(Value::as_str))
            .map(|id| AgentModel {
                id: id.into(),
                name: id.into(),
            })
            .collect())
    })
    .await
}

/// Extract model select options from an ACP `session/new` result value.
pub(super) fn models_from_session_config(result: &Value) -> Vec<AgentModel> {
    let Some(options) = result
        .get("configOptions")
        .or_else(|| result.get("sessionConfig").and_then(|c| c.get("options")))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    for option in options {
        let category = option.get("category").and_then(Value::as_str).unwrap_or("");
        let id = option.get("id").and_then(Value::as_str).unwrap_or("");
        let is_model = category.eq_ignore_ascii_case("model") || id.eq_ignore_ascii_case("model");
        if !is_model {
            continue;
        }
        let Some(choices) = option.get("options").and_then(Value::as_array) else {
            continue;
        };
        let mut models = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for choice in choices {
            let Some(value) = choice.get("value").and_then(Value::as_str) else {
                continue;
            };
            if value.is_empty() || !seen.insert(value.to_string()) {
                continue;
            }
            let name = choice
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(value);
            models.push(AgentModel {
                id: value.into(),
                name: name.into(),
            });
        }
        if !models.is_empty() {
            return models;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{expand_windows_env, models_from_session_config};
    use serde_json::json;

    #[test]
    fn windows_environment_expansion_is_deterministic() {
        assert_eq!(
            expand_windows_env("before-%ACP_UNKNOWN_TEST%-after"),
            "before--after"
        );
        assert_eq!(expand_windows_env("100% literal"), "100% literal");
    }

    #[test]
    fn session_config_parser_extracts_multiple_models() {
        let result = json!({
            "sessionId": "s1",
            "configOptions": [
                {
                    "id": "mode",
                    "category": "mode",
                    "options": [{"value": "ask", "name": "Ask"}]
                },
                {
                    "id": "model",
                    "category": "model",
                    "options": [
                        {"value": "m1", "name": "Model One"},
                        {"value": "m2", "name": "Model Two"},
                        {"value": "m1", "name": "dup"}
                    ]
                }
            ]
        });
        let models = models_from_session_config(&result);
        assert!(models.len() >= 2);
        assert_eq!(models[0].id, "m1");
        assert_eq!(models[0].name, "Model One");
        assert_eq!(models[1].id, "m2");
        let ids: std::collections::HashSet<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), models.len());
    }

    #[test]
    fn session_config_parser_handles_missing() {
        assert!(models_from_session_config(&json!({"sessionId": "s"})).is_empty());
    }
}
