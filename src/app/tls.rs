//! TLS crypto provider installation.
//!
//! S-ARCH acceptance criterion: exactly one rustls `CryptoProvider` is
//! installed at startup before any TLS code runs. We select `aws-lc-rs`
//! (the rustls default provider) over `ring` because:
//!   1. It is the rustls-maintained default for new deployments.
//!   2. It has a FIPS validation path (relevant if a user requires it).
//!   3. `aws-lc-rs` is actively maintained and audited by AWS.
//!
//! Installing the provider process-wide via `CryptoProvider::install_default`
//! avoids per-connection provider selection, which can fail at runtime when
//! mixed provider features are pulled in transitively
//! (see docs/rust-ecosystem/web-framework.md).

use anyhow::{anyhow, Result};
use rustls::crypto::aws_lc_rs::default_provider;

/// Install the process-wide rustls crypto provider.
///
/// Must be called once at startup, before constructing any `ServerConfig` or
/// `ClientConfig`. `install_default` returns `Err(existing_provider)` if a
/// provider is already installed — we treat that as success (the provider is
/// present) rather than a startup failure, since the test harness and the
/// binary both call this path. We do verify *some* default is installed after
/// the call so a silent misconfiguration is surfaced.
pub fn install_crypto_provider() -> Result<()> {
    match default_provider().install_default() {
        Ok(()) => Ok(()),
        Err(_already_installed) => {
            // A provider is already installed (e.g. a prior call in the same
            // process, common in tests). Confirm one is present; if not, the
            // install raced and lost to nothing, which is a real failure.
            if rustls::crypto::CryptoProvider::get_default().is_some() {
                Ok(())
            } else {
                Err(anyhow!(
                    "rustls crypto provider install reported a conflict but no default is set"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S-ARCH: verify the crypto provider installs at startup. The provider
    /// is process-global, so this also covers the `main()` call path.
    #[test]
    fn crypto_provider_installs_at_startup() {
        // First call installs; a second call returns `false` (already
        // installed) which we treat as success since the provider is present.
        let _ = install_crypto_provider();
        // Confirm the process default is now set.
        assert!(
            rustls::crypto::CryptoProvider::get_default().is_some(),
            "rustls default crypto provider must be installed after startup"
        );
    }
}
