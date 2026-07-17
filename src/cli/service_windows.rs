//! Windows per-user Run-key integration.
//!
//! A true Windows Service needs elevated installation and a service manager
//! lifecycle. The current-user Run key is the intentional, non-admin
//! equivalent of the Linux/macOS per-user service implementations.

use std::io::{self, Write};

use anyhow::{anyhow, Context, Result};
use tracing::info;
use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
use winreg::RegKey;

const RUN_KEY_NAME: &str = "LocalAgent";
const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Quote the executable and append the foreground daemon subcommand.
fn run_key_value(binary: &str) -> String {
    format!(r#""{binary}" start"#)
}

/// Open the user's Run key with the minimal access required for this command.
fn run_key() -> Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY_PATH, KEY_QUERY_VALUE | KEY_SET_VALUE)
        .with_context(|| format!(r"open HKCU\{RUN_KEY_PATH}"))
}

/// Register the daemon to start after the current user logs in.
pub(super) fn install() -> Result<()> {
    let binary = std::env::current_exe().context("resolve current executable")?;
    let key = run_key()?;
    match key.get_value::<String, _>(RUN_KEY_NAME) {
        Ok(existing) if !existing.is_empty() => {
            return Err(anyhow!(
                r"autostart entry already exists (HKCU\{RUN_KEY_PATH}\{RUN_KEY_NAME} = {existing:?}) \
                 — run 'local_agent uninstall-service' first"
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("read existing Run-key value"),
    }

    let value = run_key_value(&binary.to_string_lossy());
    key.set_value(RUN_KEY_NAME, &value)
        .context("set Run-key value")?;

    info!(key = %RUN_KEY_PATH, value, "installed Windows Run-key autostart");
    writeln!(
        io::stdout(),
        r"Installed autostart entry: HKCU\{RUN_KEY_PATH}\{RUN_KEY_NAME} = {value}"
    )
    .context("write service install status")
}

/// Remove the daemon's per-user autostart entry.
pub(super) fn uninstall() -> Result<()> {
    let key = run_key()?;
    match key.get_value::<String, _>(RUN_KEY_NAME) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            writeln!(
                io::stdout(),
                "Autostart entry was not present; nothing to remove."
            )
            .context("write service uninstall status")?;
            return Ok(());
        }
        Err(error) => return Err(error).context("read existing Run-key value"),
    }
    key.delete_value(RUN_KEY_NAME)
        .context("delete Run-key value")?;

    info!(key = %RUN_KEY_PATH, "removed Windows Run-key autostart");
    writeln!(
        io::stdout(),
        r"Removed autostart entry: HKCU\{RUN_KEY_PATH}\{RUN_KEY_NAME}"
    )
    .context("write service uninstall status")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_key_value_quotes_executable_paths() {
        assert_eq!(run_key_value(r"C:\app.exe"), r#""C:\app.exe" start"#);
        assert_eq!(
            run_key_value(r"C:\Program Files\local_agent.exe"),
            r#""C:\Program Files\local_agent.exe" start"#
        );
    }
}
