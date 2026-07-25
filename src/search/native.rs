//! Native fallback search path (used when `rg` is not on `PATH`).
//!
//! Walks the workspace tree with the [`ignore`] crate and matches each file
//! line-by-line with the [`regex`] crate. The walker is configured with the
//! explicit ignore list *only* (no `.gitignore` semantics) so the two paths
//! produce consistent results — matching Go's `filepath.WalkDir` fallback.
//!
//! Binary files (detected via null bytes in the first 512 bytes) are skipped
//! without erroring, matching Go. Hidden files and the same noise directories
//! as the file tree are skipped.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use regex::Regex;
use tokio_util::sync::CancellationToken;

use crate::interfaces::{SearchOptions, SearchResult};
use crate::search::{glob_to_regex, path_to_slash, SearchError, IGNORE_DIRS, MAX_SEARCH_FILES};

/// Walks `root` with the `ignore` crate and scans each file for matches.
///
/// The `cancel` token is checked per-file and per-line so a long search can be
/// aborted promptly (mirrors Go's `ctx.Err()` checks in `searchWithGo`).
pub(crate) async fn search_with_walker(
    root: &Path,
    opts: &SearchOptions,
    re: &Regex,
    cancel: CancellationToken,
) -> Result<Vec<SearchResult>, SearchError> {
    // Build the file-name glob filter (matched against the base name). rg
    // handles full globs natively; the fallback uses our small globToRegex.
    let file_filter = if opts.file_pattern.is_empty() {
        None
    } else {
        let src = glob_to_regex(&opts.file_pattern);
        Some(Regex::new(&src)?)
    };

    // Configure the walker. We want the explicit ignore list only — no
    // .gitignore, no .ignore, no global gitignore — so the fallback matches
    // Go's WalkDir which has no VCS awareness. Hidden files are filtered by
    // the explicit `!.*`-style override below plus the per-entry hidden check
    // (the `ignore` crate's `hidden(false)` toggles hidden inclusion; we keep
    // the default `hidden(true)` so hidden entries are skipped).
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true) // skip hidden files/dirs (leading dot) — matches file tree
        .ignore(false) // no .ignore
        .git_ignore(false) // no .gitignore
        .git_global(false) // no global gitignore
        .git_exclude(false) // no .git/info/exclude
        .parents(false) // do not walk up looking for ignore files
        .follow_links(false); // skip symlinks to avoid cycles (matches Go)

    // Add the explicit ignore dirs as override patterns so they are skipped
    // regardless of where they appear in the tree. `!dir` means "exclude".
    // We anchor each as `!{dir}/` so it matches the directory and everything
    // under it; the `ignore` crate's override matcher handles this.
    let mut override_builder = OverrideBuilder::new(root);
    for dir in IGNORE_DIRS {
        // `!{dir}/` excludes the directory subtree.
        let pat = format!("!{dir}/");
        // add returns Err on a malformed pattern; our literals are safe.
        override_builder
            .add(&pat)
            .map_err(|e| SearchError::RgFailed(e.to_string()))?;
    }
    let overrides = override_builder
        .build()
        .map_err(|e| SearchError::RgFailed(e.to_string()))?;
    builder.overrides(overrides);

    let walker = builder.build();
    let root_buf: PathBuf = root.to_path_buf();
    let max = opts.max_results;

    // The walk + per-file scan is blocking I/O; run on a blocking thread so we
    // do not stall the async runtime. Cancellation is checked inside.
    // Clone `re` so the closure is 'static (spawn_blocking requires it).
    let re_owned = re.clone();
    let cancel_clone = cancel.clone();
    tokio::task::spawn_blocking(move || {
        let mut results: Vec<SearchResult> = Vec::new();
        let mut files_scanned: usize = 0;
        for entry in walker {
            if cancel_clone.is_cancelled() {
                return Err(SearchError::Cancelled);
            }
            // Cap the number of files scanned so a workspace with millions of
            // files cannot exhaust blocking-thread time. Partial results are
            // returned; the caller sees a truncated set.
            if files_scanned >= MAX_SEARCH_FILES {
                break;
            }
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue, // skip unreadable entries
            };
            let ft = entry.file_type();
            // Skip directories (the walker recurses for us) and symlinks.
            let Some(ft) = ft else { continue };
            if ft.is_dir() || ft.is_symlink() {
                continue;
            }
            files_scanned += 1;

            let path = entry.path();
            let base = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip hidden files (defensive — the walker already does this when
            // hidden(true), but a non-hidden parent dir entry could surface a
            // hidden child on some platforms).
            if base.starts_with('.') {
                continue;
            }
            // Apply the file-name glob filter (matched against the base name).
            if let Some(filter) = &file_filter {
                if !filter.is_match(base) {
                    continue;
                }
            }

            // Relativize against root.
            let rel_path = match path.strip_prefix(&root_buf) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let rel_str = path_to_slash(rel_path);

            let remaining = max - results.len() as i32;
            if remaining <= 0 {
                break;
            }
            let file_results =
                match search_file(path, &rel_str, &re_owned, remaining, &cancel_clone) {
                    Ok(rs) => rs,
                    Err(_) => continue, // skip files we can't read (permission, vanished, …)
                };
            results.extend(file_results);
            if results.len() as i32 >= max {
                break;
            }
        }
        Ok(results)
    })
    .await
    .map_err(|e| SearchError::RgFailed(format!("walk task join: {e}")))?
}

/// Scans a single file for matches and returns one [`SearchResult`] per
/// matching line, up to `remaining` slots.
///
/// Binary files (detected via null bytes in the first 512 bytes) are skipped
/// without erroring — matches Go's `searchFile`. The `cancel` token is checked
/// per-line so a long file scan can be aborted.
fn search_file(
    abs_path: &Path,
    rel_path: &str,
    re: &Regex,
    remaining: i32,
    cancel: &CancellationToken,
) -> Result<Vec<SearchResult>, std::io::Error> {
    if remaining <= 0 {
        return Ok(Vec::new());
    }
    let mut f = File::open(abs_path)?;

    // Binary detection: sample the first 512 bytes for null bytes.
    let mut sample = [0u8; 512];
    let n = f.read(&mut sample)?;
    if sample[..n].contains(&0) {
        return Ok(Vec::new());
    }
    // Rewind so the scanner sees the whole file from the start.
    f.seek(SeekFrom::Start(0))?;

    let reader = BufReader::new(f);
    let mut results: Vec<SearchResult> = Vec::new();
    for (line_num, line) in (1_i32..).zip(reader.split(b'\n')) {
        if cancel.is_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "search cancelled",
            ));
        }
        let line = line?;
        // Convert bytes to a string lossy; non-UTF-8 lines are unlikely after
        // the binary check but lossy conversion avoids a hard failure.
        let line_text = String::from_utf8_lossy(&line);
        let Some(m) = re.find(&line_text) else {
            continue;
        };
        let match_start = m.start() as i32;
        let match_end = m.end() as i32;
        results.push(SearchResult {
            path: rel_path.to_string(),
            line_number: line_num,
            line_content: line_text.into_owned(),
            match_start,
            match_end,
        });
        if results.len() as i32 >= remaining {
            break;
        }
    }
    Ok(results)
}
