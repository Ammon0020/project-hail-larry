//! Shared filesystem helpers used by config, logging, MCP, and other state
//! writers.
//!
//! Consolidates two concerns that were previously duplicated or hand-rolled:
//! - [`home_dir`] — platform home directory via the `dirs` crate (falls back
//!   beyond bare `$HOME`/`%USERPROFILE%`, matching Go `os.UserHomeDir` more
//!   closely than env-only lookups).
//! - [`atomic_write`] — durable atomic replace (temp + fsync + rename) via
//!   `atomic-write-file`, with an optional Unix file mode and a best-effort
//!   parent-directory fsync matching Go `mcp.WriteFileAtomic`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;

/// Resolve the user's home directory.
///
/// Uses the `dirs` crate so resolution is correct when `$HOME` is unset
/// (passwd DB on Unix, Known Folder API on Windows). Returns `None` only when
/// the platform cannot determine a home directory at all.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Write `data` to `path` atomically and durably.
///
/// Steps:
/// 1. Ensure the parent directory exists (`create_dir_all`, mode `0700` on Unix).
/// 2. Open an [`AtomicWriteFile`] temp in the same directory (so rename is
///    same-filesystem).
/// 3. On Unix, apply `mode` to the temp file when `mode` is `Some`.
/// 4. Write all bytes, `sync_all` the file, then `commit` (rename into place).
/// 5. Best-effort `fsync` of the parent directory so the new dirent is durable.
///
/// A crash leaves either the previous file intact or a temp the helper cleans
/// up; never a half-written target. Mirrors Go `mcp.WriteFileAtomic`.
///
/// # Errors
///
/// Returns any I/O error from directory creation, open, write, fsync, rename,
/// or permission changes.
pub fn atomic_write(path: &Path, data: &[u8], mode: Option<u32>) -> io::Result<()> {
    #[cfg(not(unix))]
    let _ = mode;

    ensure_parent_dir(path)?;

    // `mut` is only needed on Unix, where the mode/owner options below mutate
    // `options`. On non-Unix the binding is never mutated, so a single `mut`
    // would trip `-D unused-mut` on Windows.
    #[cfg(unix)]
    let mut options = AtomicWriteFile::options();
    #[cfg(not(unix))]
    let options = AtomicWriteFile::options();
    // Do not preserve a pre-existing mode from an older file; callers pass the
    // intended mode (e.g. 0600 for config). Preserve owner when possible so a
    // non-root daemon does not fail when the original file is ours.
    #[cfg(unix)]
    {
        use atomic_write_file::unix::OpenOptionsExt as AtomicOpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;
        options.preserve_mode(false);
        options.try_preserve_owner(true);
        if let Some(m) = mode {
            options.mode(m);
        }
    }

    let mut file = options.open(path)?;
    file.write_all(data)?;
    // Flush contents + metadata before rename so a power loss cannot leave a
    // renamed-but-empty/truncated file.
    file.sync_all()?;
    file.commit()?;

    // Best-effort parent directory sync. Some platforms reject Sync on
    // directories; ignore those errors (matches Go WriteFileAtomic).
    #[cfg(unix)]
    {
        use std::fs::File;
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                if let Ok(d) = File::open(dir) {
                    let _ = d.sync_all();
                }
            }
        }
    }

    Ok(())
}

/// Create `dir` and any missing parents with mode `0700` on Unix (default
/// `create_dir_all` semantics on non-Unix). Use for security-sensitive state
/// directories that may hold secrets (config, MCP, TLS keys, logs, uploads).
///
/// # Errors
///
/// Returns the underlying `io::Error` if directory creation fails (e.g. the path
/// is invalid, permissions are insufficient, or a non-directory file exists).
pub fn create_dir_all(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Create the parent of `path` if missing. On Unix, state directories get mode
/// `0700` because they may hold secrets (config, MCP, TLS keys).
fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn home_dir_resolves() {
        // CI and developer shells set a home; dirs also falls back to the
        // passwd/Known Folder APIs when env is empty.
        assert!(
            home_dir().is_some(),
            "home directory must resolve in test environments"
        );
    }

    #[test]
    fn atomic_write_creates_file_and_no_temps() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("state.toml");
        atomic_write(&path, b"hello = 1\n", Some(0o600)).expect("write");
        assert_eq!(fs::read_to_string(&path).expect("read"), "hello = 1\n");

        let temps: Vec<_> = fs::read_dir(tmp.path())
            .expect("read dir")
            .filter_map(std::result::Result::ok)
            .filter(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                s.ends_with(".tmp") || s.contains("atomicwrite")
            })
            .collect();
        assert!(temps.is_empty(), "leftover temp files: {temps:?}");
    }

    #[test]
    fn atomic_write_overwrites_without_corruption() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("nested").join("cfg.toml");
        atomic_write(&path, b"v = 1\n", Some(0o600)).expect("first");
        atomic_write(&path, b"v = 2\n", Some(0o600)).expect("second");
        assert_eq!(fs::read_to_string(&path).expect("read"), "v = 2\n");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_applies_mode() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("secret.toml");
        atomic_write(&path, b"secret = true\n", Some(0o600)).expect("write");
        let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    }
}
