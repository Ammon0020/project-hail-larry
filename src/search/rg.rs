//! Ripgrep (`rg`) primary search path.
//!
//! When `rg` is on `PATH`, search shells out to `rg --json` and parses the
//! resulting NDJSON stream. rg is fast and respects `.gitignore` by default;
//! we additionally pass `--hidden` and our own `-g` negations so the skipped
//! directory set matches the native fallback (which has no `.gitignore`
//! semantics). The `--` separator marks the end of rg's options so a
//! user-supplied pattern starting with `-` cannot be interpreted as a flag
//! (argument-injection guard, matching Go).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::interfaces::{SearchOptions, SearchResult};
use crate::search::{path_to_slash, SearchError, IGNORE_DIRS};

/// Cached result of probing `PATH` for `rg` — resolved once and reused for
/// every search so we do not re-scan `PATH` on each call. Mirrors Go's
/// `sync.Once`-guarded `rgAvailable` / `exec.LookPath`.
static RG_ON_PATH: OnceLock<bool> = OnceLock::new();

/// Reports whether the ripgrep binary is available on `PATH`.
///
/// Scans `$PATH` for an executable file named `rg` (with the platform
/// extension on Windows). Avoids spawning a process just to probe, and avoids
/// pulling in the `which` crate as a direct dependency.
pub(crate) fn on_path() -> bool {
    *RG_ON_PATH.get_or_init(look_up_rg)
}

/// Scans `PATH` for `rg`. Returns `true` if found.
fn look_up_rg() -> bool {
    use std::env;

    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    let exe_name = if cfg!(windows) { "rg.exe" } else { "rg" };
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(exe_name);
        // is_file follows symlinks; sufficient for "does an rg binary exist".
        if candidate.is_file() {
            return true;
        }
    }
    false
}

/// Spawns `rg --json` and parses its output into [`SearchResult`]s.
///
/// rg exits with code 1 when there are no matches — that is mapped to an empty
/// result list, not an error. A cancellation triggered through `cancel`
/// surfaces as [`SearchError::Cancelled`].
pub(crate) async fn search_with_rg(
    root: &Path,
    opts: &SearchOptions,
    re: &Regex,
    cancel: CancellationToken,
) -> Result<Vec<SearchResult>, SearchError> {
    let mut args: Vec<String> = Vec::with_capacity(16);
    args.push("--json".into());
    args.push("--no-config".into());
    args.push("--hidden".into());
    args.push("-n".into());
    args.push("--max-count".into());
    args.push(opts.max_results.to_string());
    // ripgrep's --max-count is per file. `parse_rg_output` additionally caps
    // the returned result set globally to the requested API limit.
    if opts.ignore_case {
        args.push("--ignore-case".into());
    }
    if opts.context_lines > 0 {
        args.push("-C".into());
        args.push(opts.context_lines.to_string());
    }
    // Negate the same directories skipped by the native fallback so the two
    // strategies produce consistent results.
    for dir in IGNORE_DIRS {
        args.push("-g".into());
        args.push(format!("!{dir}"));
    }
    // Also skip hidden files/dirs (leading dot) to match the file tree.
    args.push("-g".into());
    args.push("!.*".into());
    if !opts.file_pattern.is_empty() {
        args.push("-g".into());
        args.push(opts.file_pattern.clone());
    }
    // The "--" separator prevents a user pattern starting with "-" from being
    // interpreted as a ripgrep flag.
    args.push("--".into());
    args.push(opts.pattern.clone());
    args.push(root.to_string_lossy().into_owned());

    let mut cmd = Command::new("rg");
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // rg binary name is hardcoded; the user pattern is sandboxed behind "--".
    let mut child = cmd
        .spawn()
        .map_err(|e| SearchError::RgFailed(e.to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SearchError::RgFailed("rg stdout pipe was not captured".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| SearchError::RgFailed("rg stderr pipe was not captured".to_string()))?;

    // Parse stdout concurrently with stderr draining so a chatty rg (e.g. many
    // permission warnings on stderr) cannot deadlock the pipes.
    let root_buf: PathBuf = root.to_path_buf();
    let parse_handle = tokio::spawn(parse_rg_output(
        stdout,
        root_buf,
        re.clone(),
        opts.max_results,
    ));

    let mut stderr_text = String::new();
    let mut stderr_lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = stderr_lines.next_line().await {
        if !stderr_text.is_empty() {
            stderr_text.push('\n');
        }
        stderr_text.push_str(&line);
    }

    // Wire cancellation: if the token fired while we were draining, kill the
    // child so the wait returns promptly.
    if cancel.is_cancelled() {
        let _ = child.kill().await;
    }

    let exit_status = child
        .wait()
        .await
        .map_err(|e| SearchError::RgFailed(e.to_string()))?;
    let code = exit_status.code().unwrap_or(-1);

    // rg exit 1 == "no matches", not an error.
    if code == 1 {
        let _ = parse_handle.await;
        return Ok(Vec::new());
    }
    if !exit_status.success() {
        if cancel.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        let trimmed = stderr_text.trim();
        return Err(SearchError::RgFailed(format!(
            "exit code {code}: {trimmed}"
        )));
    }

    parse_handle
        .await
        .map_err(|e| SearchError::RgFailed(format!("parse task join: {e}")))?
}

/// Parses the `rg --json` NDJSON stream on `stdout` into results.
///
/// Only `match` record types are emitted; context/summary records are ignored
/// so each result is a real hit. Match offsets are recomputed via the compiled
/// regex (rg provides `submatches`, but recomputing keeps offsets consistent
/// with the native fallback — matches Go).
// Match offsets fit in one line; `results.len()` ≤ `max` (i32), so usize→i32 cannot truncate/wrap.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
async fn parse_rg_output(
    stdout: tokio::process::ChildStdout,
    root: PathBuf,
    re: Regex,
    max: i32,
) -> Result<Vec<SearchResult>, SearchError> {
    let mut results: Vec<SearchResult> = Vec::new();
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() {
            continue;
        }
        // serde_json parses from a string slice; rg --json emits one JSON
        // object per line (NDJSON).
        let rec: RgRecord = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue, // skip malformed lines rather than aborting
        };
        if rec.record_type != "match" {
            continue;
        }

        // ripgrep nests the payload under "data"; the top-level fields are a
        // fallback for older/alternate emitters that flatten the record.
        let path_str = rec.data.path.text.as_deref().or(rec.path.text.as_deref());
        let Some(path_str) = path_str else { continue };
        // Relativize against root. rg returns paths under root, so strip_prefix
        // suffices (matches Go's filepath.Rel for the under-root case; we never
        // produce `../` components).
        let Ok(rel_path) = Path::new(path_str).strip_prefix(&root) else {
            continue;
        };
        // Never surface absolute paths; use forward slashes for cross-platform.
        let rel_str = path_to_slash(rel_path);

        let line_num = if rec.data.line_number != 0 {
            rec.data.line_number
        } else {
            rec.line_number
        };
        let line_text = rec
            .data
            .lines
            .text
            .clone()
            .or(rec.lines.text.clone())
            .unwrap_or_default();

        // Compute match offsets from the first submatch on the line text.
        let (start, end) = match re.find(&line_text) {
            Some(m) => (m.start() as i32, m.end() as i32),
            None => (0, 0),
        };

        results.push(SearchResult {
            path: rel_str,
            line_number: line_num,
            line_content: line_text,
            match_start: start,
            match_end: end,
        });
        if results.len() as i32 >= max {
            break;
        }
    }
    Ok(results)
}

// ============================================================================
// ripgrep --json record model (subset we consume)
// ============================================================================

/// Top-level rg JSON record. `record_type` corresponds to the `type` field.
/// The nested payload lives in `data`; the top-level `path`/`lines`/
/// `line_number` fields are a fallback for emitters that flatten the record.
///
/// Visibility is `pub(crate)` so the test module (`super::tests`) can
/// deserialize synthetic records and assert on their fields without having to
/// re-derive the model.
#[derive(Debug, Deserialize)]
pub(crate) struct RgRecord {
    #[serde(rename = "type")]
    pub(crate) record_type: String,
    #[serde(default)]
    pub(crate) data: RgData,
    #[serde(default)]
    pub(crate) path: RgText,
    #[serde(default)]
    pub(crate) lines: RgText,
    #[serde(default)]
    pub(crate) line_number: i32,
}

/// Nested payload of a ripgrep match record.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct RgData {
    #[serde(default)]
    pub(crate) path: RgText,
    #[serde(default)]
    pub(crate) lines: RgText,
    #[serde(default)]
    pub(crate) line_number: i32,
}

/// Text payload. ripgrep also emits a `bytes` field (base64) for binary
/// content, but we only consume the text form.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct RgText {
    #[serde(default)]
    pub(crate) text: Option<String>,
}

/// Test-only re-exports. The integration test in `super::super::tests`
/// references `rg::tests::RgRecordForTest` to deserialize a synthetic rg
/// `--json` line and assert the parser model puts the matched line text (not
/// the file path) into `data.lines.text`. Re-exporting under a stable alias
/// keeps the production structs private to this module while letting the test
/// exercise the same deserialization path the parser uses.
#[cfg(test)]
pub(crate) mod tests {
    // Only the top-level record type is named explicitly in the test; the
    // nested `RgData`/`RgText` structs are reached via `rec.data.lines.text`
    // etc. and need no name binding here (their `pub(crate)` fields are
    // visible crate-wide).
    pub(crate) use super::RgRecord as RgRecordForTest;
}
