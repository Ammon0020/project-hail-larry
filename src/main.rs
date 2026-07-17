//! Binary entry point for the Local Agent daemon.
//!
//! `--serve` starts the first HTTP UI-smoke surface. Full daemon lifecycle,
//! TLS dual listening, and CLI subcommands remain later work streams.

// Lift the no-panic lint policy for test code (see src/lib.rs for rationale).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use anyhow::{bail, Result};
use clap::Parser;
use local_agent::app;

/// Minimal bootstrap CLI while the full daemon command surface is ported.
#[derive(Debug, Parser)]
#[command(name = "local_agent", about = "Local Agent HTTP smoke server")]
struct Args {
    /// Bind the HTTP UI smoke server using configured host and port (default 7337).
    #[arg(long)]
    serve: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install exactly one rustls crypto provider before any TLS code runs.
    // Mixed provider features cause runtime selection failures
    // (web-framework.md). S-ARCH acceptance criterion.
    app::tls::install_crypto_provider()?;

    // Initialize file logging to ~/.local-agent/logs/ (S-ARCH).
    let _log_guard = app::logging::init_file_logging()?;

    let args = Args::parse();
    if args.serve {
        return app::listen::serve_http().await;
    }

    bail!("no command selected; run `local_agent --serve` to start HTTP on configured port")
}
