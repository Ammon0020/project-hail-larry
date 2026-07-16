//! File logging configuration.
//!
//! S-ARCH acceptance criterion: file logs land at a stable
//! `~/.local-agent/logs/` location. We use `tracing-appender`'s non-blocking
//! rolling writer so the daemon never blocks on disk I/O. The returned guard
//! must be held for the lifetime of the process so buffered records flush on
//! shutdown.
//!
//! Redaction of credentials, passcodes, bearer tokens, and file contents is
//! the responsibility of call sites using structured fields — see
//! `docs/rust-ecosystem/build-and-embed.md`.

use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;

use crate::fsutil;

/// Default log directory relative to the user's home: `~/.local-agent/logs/`.
pub const LOG_DIR_NAME: &str = ".local-agent/logs";

/// Initialize file logging to `~/.local-agent/logs/`.
///
/// Returns a `WorkerGuard` that must be kept alive for the process lifetime;
/// dropping it flushes the non-blocking writer. The current stub wires only
/// the file appender; console output and `env-filter` tuning land in S-DAEMON.
pub fn init_file_logging() -> Result<(WorkerGuard, PathBuf)> {
    let home = fsutil::home_dir().context("could not resolve user home directory for log path")?;
    let log_dir = home.join(LOG_DIR_NAME);
    std::fs::create_dir_all(&log_dir)
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
    fn log_dir_name_is_stable() {
        // Stable path is an S-ARCH acceptance criterion.
        assert_eq!(LOG_DIR_NAME, ".local-agent/logs");
    }

    #[test]
    fn home_dir_resolves_in_test_env() {
        // CI and dev shells resolve a home via dirs (env or platform APIs).
        assert!(
            fsutil::home_dir().is_some(),
            "home directory must resolve for logging"
        );
    }
}
