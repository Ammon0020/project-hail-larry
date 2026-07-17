//! Explicit failure for platforms without a supported user-service mechanism.

use anyhow::{bail, Result};

/// Report that startup-service installation is not available on this platform.
pub(super) fn install() -> Result<()> {
    bail!("install-service is not supported on this platform")
}

/// Report that startup-service removal is not available on this platform.
pub(super) fn uninstall() -> Result<()> {
    bail!("uninstall-service is not supported on this platform")
}
