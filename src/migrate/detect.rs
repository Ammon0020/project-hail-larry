//! Detect whether a state directory is Go-format, Rust-format, or empty.
//!
//! Detection is based on which config file exists:
//! - Go: `config.json` present (and optionally other JSON/SQLite artifacts)
//! - Rust: `config.toml` present
//! - Both: special-case (migration may have completed without deleting JSON, or
//!   interrupted after writing TOML)

use std::path::Path;

/// On-disk config file written by the Go daemon.
pub const GO_CONFIG_FILE: &str = "config.json";

/// On-disk config file written by the Rust port.
pub const RUST_CONFIG_FILE: &str = "config.toml";

/// Migration format version. Bumped when the Rust state layout changes in a
/// way that requires a new transform. Backup files use this version:
/// `config.json.bak.v{N}`.
pub const MIGRATE_FORMAT_VERSION: u32 = 1;

/// Which config format the state directory currently uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFormat {
    /// Fresh / empty state dir — no config file present.
    Empty,
    /// Go daemon state (`config.json`).
    Go,
    /// Rust daemon state (`config.toml`).
    Rust,
    /// Both files present. Usually means migration completed but the JSON
    /// backup/legacy file was left for Go rollback, or migration was interrupted
    /// after writing TOML.
    Both,
}

/// Inspect `state_dir` and return the detected config format.
///
/// Presence of the file is what matters; contents are validated later by the
/// migration / load path so detection stays cheap and side-effect free.
#[must_use]
pub fn detect_format(state_dir: &Path) -> StateFormat {
    let has_json = state_dir.join(GO_CONFIG_FILE).is_file();
    let has_toml = state_dir.join(RUST_CONFIG_FILE).is_file();
    match (has_json, has_toml) {
        (false, false) => StateFormat::Empty,
        (true, false) => StateFormat::Go,
        (false, true) => StateFormat::Rust,
        (true, true) => StateFormat::Both,
    }
}

/// Backup path for the Go config at the current migration format version.
///
/// Example: `<state_dir>/config.json.bak.v1`
#[must_use]
pub fn config_json_backup_path(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join(format!("{GO_CONFIG_FILE}.bak.v{MIGRATE_FORMAT_VERSION}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detect_empty_when_no_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert_eq!(detect_format(tmp.path()), StateFormat::Empty);
    }

    #[test]
    fn detect_go_when_only_json() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join(GO_CONFIG_FILE), b"{}").expect("write");
        assert_eq!(detect_format(tmp.path()), StateFormat::Go);
    }

    #[test]
    fn detect_rust_when_only_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join(RUST_CONFIG_FILE), b"port = 1\n").expect("write");
        assert_eq!(detect_format(tmp.path()), StateFormat::Rust);
    }

    #[test]
    fn detect_both() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join(GO_CONFIG_FILE), b"{}").expect("write");
        fs::write(tmp.path().join(RUST_CONFIG_FILE), b"port = 1\n").expect("write");
        assert_eq!(detect_format(tmp.path()), StateFormat::Both);
    }

    #[test]
    fn backup_path_is_versioned() {
        let p = config_json_backup_path(Path::new("/tmp/state"));
        assert_eq!(
            p,
            Path::new("/tmp/state").join("config.json.bak.v1"),
            "backup name must include format version"
        );
    }
}
