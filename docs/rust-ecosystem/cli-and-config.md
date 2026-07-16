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
// Persist via shared helper (do not re-inline temp+rename):
// crate::fsutil::atomic_write(&path, toml_str.as_bytes(), Some(0o600))?;
```

Go struct tags (`json:"field,omitempty"`) → `#[serde(rename = "field",
skip_serializing_if = "Option::is_none")]`. Optional fields become
`Option<T>` rather than zero-value + `omitempty`.

Home / state path: `crate::fsutil::home_dir()` (`dirs` crate) +
`LOCAL_AGENT_STATE_DIR` override. Stay under `~/.local-agent/` for Go
compatibility — do not switch to XDG config dirs during the port.

## Shared FS helpers — `src/fsutil` (`dirs` + `atomic-write-file`)

- `home_dir()` — `dirs::home_dir()` (passwd / Known Folder fallback).
- `atomic_write(path, data, mode)` — durable replace used by config, MCP
  `mcp.json`, conversation store, TLS cert material. Prefer this over any
  new hand-rolled temp+rename.

## MCP Config — JSON (replaces raw JSON read/write in `internal/mcp/`)

The MCP config is Claude-Desktop-compatible `mcp.json` edited as raw JSON by
the frontend. Preserve raw bytes for read/modify/write; contract fixtures
define existing behavior. Atomic write via `crate::fsutil::atomic_write`.

## QR Code — `qrcode` (replaces `skip2/go-qrcode`)

```rust
use qrcode::QrCode;
let code = QrCode::new(b"https://192.168.1.100:7338/pair?token=...")?;
let png = code.render::<image::Luma<u8>>().build();
// Render to terminal or save to PNG for display
```

The pairing package generates QR + four-word mnemonic passcode. Port the
English word list + sampling only (hyphen-joined 4 words). **Do not** pull a
full BIP-39 crate — Go is not BIP-39 entropy+checksum.

## Fetching Live Docs

```
context7: resolve-library-id "clap rust CLI"
context7: resolve-library-id "qrcode rust"
```
