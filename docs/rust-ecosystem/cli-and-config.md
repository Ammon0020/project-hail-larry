# CLI & Config Reference

> `clap` (CLI), `toml` + `serde` (config), `qrcode` (QR pairing).

## CLI — `clap` (replaces `spf13/cobra`)

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "app", about = "Local Agent Interface daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon
    Start { #[arg(long)] background: bool },
    /// Stop the running daemon
    Stop,
    /// Show daemon status
    Status,
    /// Register a workspace folder
    AddFolder { path: Option<PathBuf> },
    /// Remove a workspace
    RemoveFolder { id: String },
    /// List registered workspaces
    ListFolders,
    /// Generate QR code for pairing
    Pair,
    /// List paired devices
    Devices,
    /// Revoke a paired device
    Revoke { id: String },
    /// Install as system service
    InstallService,
    /// Uninstall system service
    UninstallService,
    /// Show recent logs
    Logs,
}
```

The `cmd/app/` Go code (cobra commands) maps to clap subcommands. Platform-
specific service install (`service_linux.go`, `service_darwin.go`,
`service_windows.go`) uses `#[cfg(target_os = "...")]` conditional
compilation instead of Go's build tags.

## Config — `toml` + `serde` (replaces `pelletier/go-toml/v2`)

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    tls_enabled: bool,
    #[serde(default = "default_port")]
    port: u16,
    workspaces: Vec<WorkspaceEntry>,
    // ...
}

let config: Config = toml::from_str(&fs::read_to_string(path)?)?;
let toml_str = toml::to_string_pretty(&config)?;
```

Go struct tags (`json:"field,omitempty"`) → `#[serde(rename = "field",
skip_serializing_if = "Option::is_none")]`. Optional fields become
`Option<T>` rather than zero-value + `omitempty`.

## MCP Config — JSON (replaces raw JSON read/write in `internal/mcp/`)

The MCP config is Claude-Desktop-compatible `mcp.json` edited as raw JSON by
the frontend (formatting/comments preserved). Keep using `serde_json::Value`
for round-trip — don't deserialize into typed structs or formatting is lost.

## QR Code — `qrcode` (replaces `skip2/go-qrcode`)

```rust
use qrcode::QrCode;
let code = QrCode::new(b"https://192.168.1.100:7338/pair?token=...")?;
let png = code.render::<image::Luma<u8>>().build();
// Render to terminal or save to PNG for display
```

The pairing package generates QR + four-word mnemonic passcode. The mnemonic
generation (BIP-39-style word list) is pure string logic — port directly.

## Fetching Live Docs

```
context7: resolve-library-id "clap rust CLI"
context7: resolve-library-id "qrcode rust"
```
