//! Binary entry point for the Local Agent daemon.
//!
//! S-ARCH scope: install the rustls crypto provider, initialize file logging,
//! and exit cleanly. Real daemon wiring (clap subcommands, axum server, ACP
//! sessions) lands in S-DAEMON / S-SERVER / S-CLI.

// Lift the no-panic lint policy for test code (see src/lib.rs for rationale).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use anyhow::Result;
use local_agent::app;

#[tokio::main]
async fn main() -> Result<()> {
    // Install exactly one rustls crypto provider before any TLS code runs.
    // Mixed provider features cause runtime selection failures
    // (web-framework.md). S-ARCH acceptance criterion.
    app::tls::install_crypto_provider()?;

    // Initialize file logging to ~/.local-agent/logs/ (S-ARCH).
    let _log_guard = app::logging::init_file_logging()?;

    tracing::info!("local-agent daemon starting (S-ARCH stub)");
    // Real daemon composition root arrives in S-DAEMON.
    Ok(())
}
