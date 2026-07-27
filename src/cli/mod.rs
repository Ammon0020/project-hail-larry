//! Host-side command-line interface.
//!
//! Commands that mutate host configuration operate directly on the local
//! state directory; commands that inspect a live daemon use its loopback HTTP
//! listener. This keeps LAN device credentials out of host CLI invocation.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::app::{daemon, logging, port};
use crate::config::Config;
use crate::interfaces::WorkspaceManager;
use crate::workspace::Manager as WorkspaceManagerImpl;

#[cfg(target_os = "linux")]
mod service_linux;
#[cfg(target_os = "macos")]
mod service_macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod service_unsupported;
#[cfg(target_os = "windows")]
mod service_windows;

const LOG_TAIL_BYTES: u64 = 64 * 1024;

/// Local Agent Interface host CLI.
#[derive(Debug, Parser)]
#[command(name = "local_agent", about = "Self-hosted AI code editor daemon")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Supported host CLI commands.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Start the daemon in the foreground or as a detached child process.
    Start {
        /// Start a detached daemon process and return immediately.
        #[arg(long)]
        background: bool,
    },
    /// Gracefully stop the daemon tracked by its PID file.
    Stop,
    /// Show daemon process state and configured listener addresses.
    Status,
    /// Register an absolute workspace directory.
    AddFolder { path: String },
    /// Unregister a workspace by its stable ID.
    RemoveFolder { id: String },
    /// List configured workspace directories.
    ListFolders,
    /// Generate a one-time QR code and mnemonic pairing passcode.
    Pair,
    /// List paired devices through the local daemon.
    Devices,
    /// Revoke a paired device through the local daemon.
    Revoke { id: String },
    /// Print the recent tail of the rolling daemon log.
    Logs,
    /// Register a per-user startup service.
    InstallService,
    /// Remove a per-user startup service.
    UninstallService,
}

/// Parse and execute a CLI command.
///
/// # Errors
///
/// Returns an error if the selected subcommand fails (e.g. daemon start fails,
/// workspace path is invalid, or a service install/uninstall step errors).
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Start { background } => start(background).await,
        Commands::Stop => stop(),
        Commands::Status => status().await,
        Commands::AddFolder { path } => add_folder(&path).await,
        Commands::RemoveFolder { id } => remove_folder(&id).await,
        Commands::ListFolders => list_folders().await,
        Commands::Pair => pair().await,
        Commands::Devices => devices().await,
        Commands::Revoke { id } => revoke(&id).await,
        Commands::Logs => logs(),
        Commands::InstallService => install_service(),
        Commands::UninstallService => uninstall_service(),
    }
}

/// Register the daemon as a per-user service on the current platform.
fn install_service() -> Result<()> {
    #[cfg(target_os = "linux")]
    return service_linux::install();
    #[cfg(target_os = "macos")]
    return service_macos::install();
    #[cfg(target_os = "windows")]
    return service_windows::install();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return service_unsupported::install();
}

/// Remove the daemon's per-user service from the current platform.
fn uninstall_service() -> Result<()> {
    #[cfg(target_os = "linux")]
    return service_linux::uninstall();
    #[cfg(target_os = "macos")]
    return service_macos::uninstall();
    #[cfg(target_os = "windows")]
    return service_windows::uninstall();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return service_unsupported::uninstall();
}

async fn start(background: bool) -> Result<()> {
    let config = Config::load().context("load local-agent configuration")?;
    if daemon::status(&config)?.running {
        bail!("daemon is already running; stop it with `local_agent stop` first");
    }
    // The PID file may be missing while a process still holds the configured
    // port (orphaned daemon, different binary, interrupted cleanup). Probe
    // before constructing the daemon so we fail fast with an actionable
    // message instead of a confusing `Address already in use` at bind time.
    // Skip for port 0 (OS-assigned, used in tests).
    if config.port != 0 {
        let port = u16::try_from(config.port).context("validate HTTP port")?;
        let host = config.host.clone();
        let listening = tokio::task::spawn_blocking(move || port::is_port_listening(&host, port))
            .await
            .context("join port probe")?;
        if listening {
            let holder = port::find_pid_listening_on(port)
                .ok()
                .flatten()
                .map(|pid| format!(" (PID {pid})"))
                .unwrap_or_default();
            bail!(
                "port {port} is already in use{holder}; stop the other process or run \
                 `local_agent stop` first"
            );
        }
    }
    if background {
        let executable = std::env::current_exe().context("resolve current executable")?;
        ProcessCommand::new(executable)
            .arg("start")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("start daemon in background")?;
        writeln!(
            io::stdout(),
            "Daemon starting in background. Run `local_agent status` to inspect it."
        )
        .context("write start status")?;
        return Ok(());
    }

    let daemon = daemon::Daemon::new(config).await?;
    daemon.run(wait_for_shutdown_signal()).await
}

fn stop() -> Result<()> {
    let config = Config::load().context("load local-agent configuration")?;
    daemon::stop(&config)?;
    writeln!(io::stdout(), "Daemon stopped.").context("write stop status")
}

async fn status() -> Result<()> {
    let config = Config::load().context("load local-agent configuration")?;
    let status = daemon::status(&config)?;
    let workspaces = configured_workspaces(&config).await?;
    let mut out = io::stdout();
    match status.pid {
        Some(pid) => writeln!(out, "Status:   Running (PID {pid})")?,
        None => writeln!(out, "Status:   Stopped")?,
    }
    writeln!(out, "HTTP:     http://{}", status.http)?;
    if let Some(https) = status.https {
        writeln!(out, "HTTPS:    https://{https} (self-signed)")?;
    }
    writeln!(out, "Data:     {}", config.data_dir)?;
    writeln!(out, "Workspaces: {}", workspaces.len())?;
    for workspace in &workspaces {
        write_workspace_line(&mut out, workspace)?;
    }
    Ok(())
}

async fn add_folder(path: &str) -> Result<()> {
    let absolute = Path::new(path)
        .canonicalize()
        .with_context(|| format!("resolve workspace path {path}"))?;
    if !absolute.is_dir() {
        bail!("workspace path is not a directory: {}", absolute.display());
    }
    let mut config = Config::load().context("load local-agent configuration")?;
    let absolute = absolute.to_string_lossy().into_owned();
    let existed = config.workspaces.iter().any(|entry| entry == &absolute);
    config
        .add_workspace(&absolute)
        .context("save workspace configuration")?;

    // When the daemon is already running, also register into its live manager
    // so the UI sees the folder without a restart.
    if daemon::status(&config)?.running {
        local_request(
            &config,
            reqwest::Method::POST,
            "/api/workspaces",
            Some(json!({ "path": absolute })),
        )
        .await
        .context("sync workspace into running daemon")?;
    }

    let message = if existed {
        "Workspace already registered"
    } else {
        "Workspace registered"
    };
    writeln!(io::stdout(), "{message}: {absolute}").context("write workspace status")
}

async fn remove_folder(id: &str) -> Result<()> {
    let mut config = Config::load().context("load local-agent configuration")?;
    let workspaces = configured_workspaces(&config).await?;
    let workspace = workspaces
        .into_iter()
        .find(|workspace| workspace.id == id)
        .ok_or_else(|| anyhow!("workspace not found: {id}"))?;
    config
        .remove_workspace(&workspace.path)
        .context("save workspace configuration")?;

    if daemon::status(&config)?.running {
        let path = format!("/api/workspaces/{}", workspace.id);
        match local_request(&config, reqwest::Method::DELETE, &path, None).await {
            Ok(_) => {}
            // Config was already updated; a missing live entry just means the
            // daemon never loaded this folder (e.g. added while stopped).
            Err(error) if error.to_string().contains("HTTP 404") => {}
            Err(error) => return Err(error).context("sync workspace removal into running daemon"),
        }
    }

    writeln!(
        io::stdout(),
        "Workspace removed: {} ({})",
        workspace.id,
        workspace.path
    )
    .context("write workspace status")
}

async fn list_folders() -> Result<()> {
    let config = Config::load().context("load local-agent configuration")?;
    let workspaces = configured_workspaces(&config).await?;
    let mut out = io::stdout();
    if workspaces.is_empty() {
        writeln!(
            out,
            "No workspaces registered. Use `local_agent add-folder <path>` to add one."
        )?;
        return Ok(());
    }
    for workspace in &workspaces {
        write_workspace_line(&mut out, workspace)?;
    }
    Ok(())
}

fn write_workspace_line(
    out: &mut impl Write,
    workspace: &crate::interfaces::WorkspaceInfo,
) -> Result<()> {
    if workspace.available {
        writeln!(
            out,
            "{}\t{}\t{}",
            workspace.id, workspace.name, workspace.path
        )?;
    } else {
        // Clear marker so operators can reconnect the drive or remove-folder.
        writeln!(
            out,
            "{}\t{}\t{}\tUNAVAILABLE: {}",
            workspace.id, workspace.name, workspace.path, workspace.error
        )?;
    }
    Ok(())
}

async fn configured_workspaces(config: &Config) -> Result<Vec<crate::interfaces::WorkspaceInfo>> {
    let manager = WorkspaceManagerImpl::new();
    for path in &config.workspaces {
        if let Err(error) = manager.register(path).await {
            // Keep missing workspace registrations in config; surface them to
            // the operator rather than silently pruning state.
            tracing::warn!(workspace = %path, %error, "configured workspace unavailable");
            manager
                .retain_unavailable(path, error.to_string())
                .map_err(anyhow::Error::from)?;
        }
    }
    manager.list().await.map_err(Into::into)
}

async fn pair() -> Result<()> {
    let config = running_config()?;
    let body = json!({
        "host": pairing_host(&config.host),
        "port": u16::try_from(config.port).context("validate pairing port")?,
    });
    let session = local_request(
        &config,
        reqwest::Method::POST,
        "/api/pair/initiate",
        Some(body),
    )
    .await?;
    let mut out = io::stdout();
    writeln!(out, "Passcode: {}", required_string(&session, "passcode")?)?;
    writeln!(out, "URL:      {}", required_string(&session, "url")?)?;
    writeln!(out, "QR Code:  {}", required_string(&session, "qrPath")?)?;
    writeln!(out, "Expires:  {}", required_string(&session, "expiresAt")?)?;
    Ok(())
}

async fn devices() -> Result<()> {
    let config = running_config()?;
    let devices = local_request(&config, reqwest::Method::GET, "/api/devices", None).await?;
    let items = devices
        .as_array()
        .ok_or_else(|| anyhow!("invalid devices response from daemon"))?;
    let mut out = io::stdout();
    if items.is_empty() {
        writeln!(
            out,
            "No paired devices. Use `local_agent pair` to pair a device."
        )?;
        return Ok(());
    }
    writeln!(out, "DEVICE ID\tNAME\tPAIRED AT")?;
    for device in items {
        writeln!(
            out,
            "{}\t{}\t{}",
            required_string(device, "id")?,
            required_string(device, "name")?,
            required_string(device, "pairedAt")?
        )?;
    }
    Ok(())
}

async fn revoke(id: &str) -> Result<()> {
    let config = running_config()?;
    let path = format!("/api/devices/{id}");
    let _ = local_request(&config, reqwest::Method::DELETE, &path, None).await?;
    writeln!(io::stdout(), "Device {id} revoked.").context("write revoke status")
}

fn logs() -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let log_dir = logging::log_dir().context("resolve daemon log directory")?;
    let Some(log_file) = newest_log(&log_dir)? else {
        writeln!(io::stdout(), "No log file found. Is the daemon running?")?;
        return Ok(());
    };
    let metadata = fs::metadata(&log_file)
        .with_context(|| format!("stat daemon log {}", log_file.display()))?;
    let offset = metadata.len().saturating_sub(LOG_TAIL_BYTES);
    let mut file = fs::File::open(&log_file)
        .with_context(|| format!("open daemon log {}", log_file.display()))?;
    file.seek(SeekFrom::Start(offset))
        .context("seek daemon log")?;
    if offset > 0 {
        let mut byte = [0_u8; 1];
        while file.read(&mut byte).context("read daemon log")? == 1 {
            if byte[0] == b'\n' {
                break;
            }
        }
    }
    io::copy(&mut file, &mut io::stdout()).context("write daemon logs")?;
    Ok(())
}

fn newest_log(log_dir: &Path) -> Result<Option<std::path::PathBuf>> {
    let entries = match fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", log_dir.display())),
    };
    let mut files = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|type_| type_.is_file())
                .unwrap_or(false)
        })
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("local-agent.log")
        })
        .collect::<Vec<_>>();
    files.sort_by_key(std::fs::DirEntry::file_name);
    Ok(files.pop().map(|entry| entry.path()))
}

fn running_config() -> Result<Config> {
    let config = Config::load().context("load local-agent configuration")?;
    if !daemon::status(&config)?.running {
        bail!("daemon is not running; start it with `local_agent start` first");
    }
    Ok(config)
}

async fn local_request(
    config: &Config,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let (url, client) = local_daemon_client(config)?;
    let url = format!("{url}{path}");
    let request = client.request(method, url);
    let response = match body {
        Some(body) => request.json(&body).send().await,
        None => request.send().await,
    }
    .context("call local daemon API")?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .context("read local daemon API response")?;
    if !status.is_success() {
        bail!(
            "local daemon API returned HTTP {}: {}",
            status,
            String::from_utf8_lossy(&bytes)
        );
    }
    serde_json::from_slice(&bytes).context("decode local daemon API response")
}

/// Build the base URL and HTTP client for loopback daemon calls.
///
/// The daemon defaults to TLS-on (`Config::default_or_error` sets
/// `tls_enabled: true`), but the CLI's serde default for `tls_enabled` is
/// `false` (the field is `#[serde(default)]`). When the on-disk config omits
/// the field, the CLI reads `false` while the daemon serves TLS — the CLI
/// then builds a plain HTTP client, the daemon's cleartext listener redirects
/// to HTTPS, reqwest follows the redirect to the self-signed endpoint, and
/// raises `invalid peer certificate: UnknownIssuer`.
///
/// To be robust against this config drift, HTTPS + CA trust is driven by the
/// **cert file's presence on disk** rather than the `tls_enabled` flag. If
/// the daemon's `cert.pem` exists, connect directly to the HTTPS port with
/// the cert loaded as a root CA. Otherwise, fall back to plain HTTP with
/// redirects disabled (so a stale redirect doesn't produce a confusing TLS
/// error on a client that has no CA loaded).
fn local_daemon_client(config: &Config) -> Result<(String, reqwest::Client)> {
    // Resolve the cert directory using the same logic as the listener
    // (listen.rs): explicit `tls_cert_dir` wins, otherwise `{data_dir}/tls`.
    let cert_dir = if config.tls_cert_dir.is_empty() {
        std::path::Path::new(&config.data_dir).join("tls")
    } else {
        std::path::PathBuf::from(&config.tls_cert_dir)
    };
    let cert_path = cert_dir.join(crate::app::tls_cert::CERT_FILE_NAME);

    // If the cert file exists, the daemon is serving TLS (regardless of what
    // the local config's tls_enabled flag says). Connect via HTTPS with the
    // cert trusted as a root CA.
    if cert_path.is_file() {
        let https_port =
            u16::try_from(daemon::resolved_https_port(config)?).context("validate HTTPS port")?;
        let url = format!("https://127.0.0.1:{https_port}");
        let cert_pem = std::fs::read(&cert_path).with_context(|| {
            format!(
                "read daemon TLS certificate at {} (is the daemon running with TLS?)",
                cert_path.display()
            )
        })?;
        let cert =
            reqwest::Certificate::from_pem(&cert_pem).context("parse daemon TLS certificate")?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .add_root_certificate(cert)
            .build()
            .context("create local daemon HTTPS client")?;
        return Ok((url, client));
    }

    // No cert file on disk — the daemon is either running without TLS or not
    // running at all. Use plain HTTP. Disable redirect-following so that if
    // the daemon IS serving TLS (and redirecting HTTP→HTTPS) but the cert
    // file is unreadable/missing, the CLI surfaces a clear connection error
    // instead of following the redirect to an untrusted HTTPS endpoint and
    // raising a confusing "invalid peer certificate" message.
    let port = u16::try_from(config.port).context("validate HTTP port")?;
    let url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("create local daemon HTTP client")?;
    Ok((url, client))
}

fn pairing_host(host: &str) -> &str {
    match host {
        "" | "0.0.0.0" | "::" => "localhost",
        host => host,
    }
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("daemon response missing string field '{key}'"))
}

/// Wait for Ctrl-C and, on Unix, SIGTERM.
fn wait_for_shutdown_signal() -> CancellationToken {
    let token = CancellationToken::new();
    let signal_token = token.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            match signal(SignalKind::terminate()) {
                Ok(mut terminate) => {
                    tokio::select! {
                        result = tokio::signal::ctrl_c() => {
                            if let Err(error) = result {
                                tracing::error!(%error, "wait for Ctrl-C");
                            }
                        }
                        _ = terminate.recv() => {}
                    }
                }
                Err(error) => tracing::error!(%error, "install SIGTERM handler"),
            }
        }
        #[cfg(not(unix))]
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "wait for Ctrl-C");
        }
        signal_token.cancel();
    });
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_cli_surface() {
        for command in [
            ["local_agent", "start"].as_slice(),
            ["local_agent", "start", "--background"].as_slice(),
            ["local_agent", "stop"].as_slice(),
            ["local_agent", "status"].as_slice(),
            ["local_agent", "add-folder", "/tmp/work"].as_slice(),
            ["local_agent", "remove-folder", "workspace-id"].as_slice(),
            ["local_agent", "list-folders"].as_slice(),
            ["local_agent", "pair"].as_slice(),
            ["local_agent", "devices"].as_slice(),
            ["local_agent", "revoke", "device-id"].as_slice(),
            ["local_agent", "logs"].as_slice(),
            ["local_agent", "install-service"].as_slice(),
            ["local_agent", "uninstall-service"].as_slice(),
        ] {
            Cli::try_parse_from(command).expect("parse CLI command");
        }
    }
}
