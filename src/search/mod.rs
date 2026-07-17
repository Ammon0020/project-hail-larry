//! Workspace content search (Go `internal/search/`).
//!
//! Blueprint references: Sec 14 (Search). The daemon exposes a workspace-wide
//! content-search endpoint that powers the editor's find-in-files UI. Search
//! runs over registered workspace roots, applies the same ignore rules as the
//! file tree, and returns matching lines with byte offsets and (when `rg` is
//! used) optional context.
//!
//! # Design (mirrors the Go implementation)
//!
//! - **Primary path:** spawn `rg --json` via [`tokio::process::Command`] when
//!   ripgrep is on `PATH`. JSON events are parsed for path/line/offsets.
//!   `IgnoreCase`, `MaxResults`, `FilePattern`, and `ContextLines` are honored
//!   via rg flags exactly as in Go.
//! - **Fallback:** when `rg` is missing, walk the tree with the [`ignore`]
//!   crate and match each file line-by-line with the [`regex`] crate. The
//!   walker is configured with the explicit ignore list *only* (no
//!   `.gitignore` semantics) so the two paths produce consistent results.
//! - Both paths skip the same set of noise directories (see [`IGNORE_DIRS`])
//!   and hidden files, never return absolute paths, and cap results at
//!   [`DEFAULT_MAX_RESULTS`] when the caller does not specify a cap.
//!
//! Search DTOs ([`SearchOptions`], [`SearchResult`]) live in
//! [`crate::interfaces`] so the trait layer never depends on this module.
//!
//! All public functions return `std::result::Result`; none panic.

use std::path::Path;

use regex::Regex;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::interfaces::{SearchOptions, SearchResult};

pub use crate::interfaces::{
    SearchOptions as SearchOptionsReExport, SearchResult as SearchResultReExport,
};

mod native;
mod rg;

#[cfg(test)]
mod tests;

/// Default cap on returned matches when `SearchOptions::max_results <= 0`.
/// Matches Go's `defaultMaxResults`. A giant workspace cannot exhaust memory.
pub const DEFAULT_MAX_RESULTS: i32 = 200;

/// Directory names always skipped during the walk. Matches the file-tree
/// behavior in `internal/workspace` plus common build/dep caches that would
/// otherwise produce noisy matches. Kept as a slice (rather than a `HashSet`)
/// because the set is small and iterated for rg `-g` negations and the
/// `ignore` builder's `add_ignore` calls.
pub const IGNORE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "vendor",
    "dist",
    "build",
    ".next",
    "target",
    ".cache",
    "coverage",
    "out",
];

/// Errors returned by the search module.
///
/// `EmptyPattern` and `InvalidPattern` are caller errors (the API layer maps
/// them to HTTP 400). `RgFailed` wraps a non-zero rg exit (other than the
/// "no matches" exit code 1, which is handled internally and returns `Ok`).
#[derive(Debug, Error)]
pub enum SearchError {
    /// Caller passed an empty/whitespace pattern.
    #[error("search pattern is required")]
    EmptyPattern,

    /// Pattern failed regex compilation.
    #[error("invalid pattern: {0}")]
    InvalidPattern(#[from] regex::Error),

    /// `rg` exited with a non-zero, non-1 status (1 means "no matches" and is
    /// not an error). Includes trimmed stderr for diagnostics.
    #[error("rg failed: {0}")]
    RgFailed(String),

    /// The native fallback walker returned an underlying I/O error.
    #[error("walk error: {0}")]
    Walk(#[from] std::io::Error),

    /// The cancellation token was triggered mid-search.
    #[error("search cancelled")]
    Cancelled,
}

/// Runs a content search rooted at `root` and returns up to
/// `opts.max_results` matches.
///
/// All returned paths are relative to `root`; absolute paths are never
/// returned. The `cancel` token is honored for cancellation/timeout in both
/// the rg and native-fallback strategies (mirrors Go's `context.Context`).
///
/// # Errors
///
/// Returns [`SearchError::EmptyPattern`] if `opts.pattern` is empty,
/// [`SearchError::InvalidPattern`] if the pattern fails regex compilation,
/// and [`SearchError::RgFailed`] / [`SearchError::Walk`] for backend failures.
pub async fn search(
    root: &Path,
    opts: &SearchOptions,
    cancel: CancellationToken,
) -> Result<Vec<SearchResult>, SearchError> {
    if opts.pattern.trim().is_empty() {
        return Err(SearchError::EmptyPattern);
    }

    // Normalize the cap; <= 0 means the default. Clone opts so we can mutate
    // the local cap without touching the caller's struct.
    let mut opts = opts.clone();
    if opts.max_results <= 0 {
        opts.max_results = DEFAULT_MAX_RESULTS;
    }

    // Compile the pattern up front so both strategies share the same
    // validation. Case-insensitivity is applied via the (?i) inline flag
    // prefix (matches Go, which cannot pass flags to regexp.Compile).
    let mut pattern = opts.pattern.clone();
    if opts.ignore_case {
        pattern.insert_str(0, "(?i)");
    }
    let re = Regex::new(&pattern)?;

    if rg::on_path() {
        rg::search_with_rg(root, &opts, &re, cancel).await
    } else {
        native::search_with_walker(root, &opts, &re, cancel).await
    }
}

/// Converts a simple glob (with `*` and `?`) into an anchored regex pattern
/// string. Matches Go's `globToRegex`: does not support character classes or
/// braces. rg handles full globs natively, so this is only used by the native
/// fallback.
///
/// Returns the regex source string (anchored with `^...$`); the caller
/// compiles it.
pub(crate) fn glob_to_regex(glob: &str) -> String {
    let mut out = String::with_capacity(glob.len() + 2);
    out.push('^');
    for ch in glob.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            // Escape regex metacharacters so a literal `.` in the glob is a
            // literal dot in the pattern.
            '.' | '+' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('$');
    out
}
