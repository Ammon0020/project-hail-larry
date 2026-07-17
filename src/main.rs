//! Binary entry point for the Local Agent daemon.
//!
//! The `start` command owns daemon lifecycle and dual HTTP/HTTPS listening.

// Lift the no-panic lint policy for test code (see src/lib.rs for rationale).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use anyhow::Result;
use clap::Parser;
use local_agent::app;
use local_agent::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    // Install exactly one rustls crypto provider before any TLS code runs.
    // Mixed provider features cause runtime selection failures
    // (web-framework.md). S-ARCH acceptance criterion.
    app::tls::install_crypto_provider()?;

    // Initialize file logging to ~/.local-agent/logs/ (S-ARCH).
    let _log_guard = app::logging::init_file_logging()?;

    local_agent::cli::run(Cli::parse()).await
}
