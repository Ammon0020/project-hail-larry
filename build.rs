//! Build-time checks for the Local Agent Rust binary.
//!
//! The SPA is embedded from `web/dist/` via `rust-embed`. Fail early with a
//! clear message when the Vite build output is missing, rather than shipping a
//! binary that only serves the development fallback page.

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let index = PathBuf::from(&manifest_dir).join("web/dist/index.html");

    // Rebuild when the SPA entry appears, disappears, or changes.
    println!("cargo:rerun-if-changed={}", index.display());
    println!("cargo:rerun-if-changed={manifest_dir}/web/dist");

    if !index.is_file() {
        // Prefer Write over eprintln! — package clippy denies print_stderr.
        let _ = writeln!(
            io::stderr(),
            "\nerror: web/dist/index.html is missing.\n\
             \n\
             The Rust binary embeds the frontend from web/dist/ (rust-embed).\n\
             Go's internal/server/dist/ is not used by the Rust build.\n\
             \n\
             Build the frontend first, then rebuild:\n\
               cd web && npm run build\n\
             Or run the full project build:\n\
               ./build.sh      (Linux/macOS)\n\
               .\\build.ps1    (Windows)\n"
        );
        process::exit(1);
    }
}
