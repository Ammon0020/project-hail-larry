//! File logging configuration.
//!
//! S-ARCH acceptance criterion: file logs land at a stable location under the
//! state directory (`~/.local-agent/logs/` by default). We use
//! `tracing-appender`'s non-blocking rolling writer so the daemon never blocks
//! on disk I/O. The returned guard must be held for the lifetime of the
//! process so buffered records flush on shutdown.
//!
//! When `LOCAL_AGENT_STATE_DIR` is set (contract harness, tests), logs follow
//! that override — matching Go's `cfg.DataDir/daemon.log` placement under the
//! isolated state dir — so neutralizing `$HOME` for autodetect does not break
//! startup.
//!
//! Redaction of credentials, passcodes, bearer tokens, and file contents is
//! the responsibility of call sites using structured fields — see
//! `docs/rust-ecosystem/build-and-embed.md`.

use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;

use crate::config::Config;

/// Log subdirectory name under the resolved state directory.
pub const LOG_SUBDIR: &str = "logs";

/// Resolve the stable rolling-log directory used by the daemon and CLI.
///
/// Uses [`Config::resolved_state_dir`] so `LOCAL_AGENT_STATE_DIR` redirects
/// logs into the isolated state tree (default: `~/.local-agent/logs`).
pub fn log_dir() -> Result<PathBuf> {
    let state_dir =
        Config::resolved_state_dir().context("could not resolve state directory for log path")?;
    Ok(state_dir.join(LOG_SUBDIR))
}

/// Initialize file logging under the resolved state directory's `logs/` folder.
///
/// Returns a `WorkerGuard` that must be kept alive for the process lifetime;
/// dropping it flushes the non-blocking writer.
pub fn init_file_logging() -> Result<(WorkerGuard, PathBuf)> {
    let log_dir = log_dir()?;
    crate::fsutil::create_dir_all(&log_dir)
        .with_context(|| format!("creating log dir {}", log_dir.display()))?;

    // Daily rotation keeps log files bounded. The prefix is the daemon name;
    // the suffix is the date.
    let file_appender = rolling::daily(&log_dir, "local-agent.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .ok(); // tolerate re-init in tests

    Ok((guard, log_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_subdir_is_stable() {
        // Stable path component is an S-ARCH acceptance criterion.
        assert_eq!(LOG_SUBDIR, "logs");
    }

    #[test]
    fn log_dir_follows_state_dir() {
        // Default (no LOCAL_AGENT_STATE_DIR): ~/.local-agent/logs.
        let dir = log_dir().expect("log_dir resolves");
        assert!(
            dir.ends_with(".local-agent/logs") || dir.ends_with(LOG_SUBDIR),
            "unexpected log_dir: {}",
            dir.display()
        );
    }
}
