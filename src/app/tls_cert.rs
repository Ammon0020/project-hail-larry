//! Self-signed certificate generation for the HTTPS listener.
//!
//! Certificates are generated once and then reused. Replacing a locally trusted
//! certificate without an explicit operator action would break paired devices
//! and weakens trust-on-first-use behavior.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rcgen::{CertificateParams, KeyPair, SanType, PKCS_ECDSA_P256_SHA256};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::fsutil;

/// Filename for the public certificate, stored under `Config::tls_cert_dir`.
pub const CERT_FILE_NAME: &str = "cert.pem";
/// Filename for the private key, stored under `Config::tls_cert_dir`.
pub const KEY_FILE_NAME: &str = "key.pem";

/// Generated TLS certificate and key paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificatePaths {
    /// PEM-encoded X.509 certificate path.
    pub cert: PathBuf,
    /// PEM-encoded private-key path.
    pub key: PathBuf,
}

/// Generate or reuse a self-signed ECDSA P-256 certificate.
///
/// The certificate is valid for localhost, `127.0.0.1`, and a configured
/// non-wildcard bind host. Both PEM files are atomically written with
/// owner-only permissions so a crash never leaves a truncated private key.
///
/// # Errors
///
/// Returns an error if the certificate directory cannot be created, SANs
/// cannot be built, the certificate cannot be generated, or the PEM files
/// cannot be written.
pub fn ensure_self_signed(cert_dir: &Path, host: &str) -> Result<CertificatePaths> {
    let paths = CertificatePaths {
        cert: cert_dir.join(CERT_FILE_NAME),
        key: cert_dir.join(KEY_FILE_NAME),
    };
    if paths.cert.is_file() && paths.key.is_file() {
        return Ok(paths);
    }

    let mut sans = vec![
        SanType::DnsName("localhost".try_into().context("build localhost SAN")?),
        SanType::IpAddress("127.0.0.1".parse().context("parse loopback SAN")?),
    ];
    if !host.is_empty() && host != "0.0.0.0" && host != "::" {
        let san = match host.parse() {
            Ok(address) => SanType::IpAddress(address),
            Err(_) => SanType::DnsName(host.try_into().context("validate configured host SAN")?),
        };
        if !sans.contains(&san) {
            sans.push(san);
        }
    }

    let mut params = CertificateParams::new(Vec::<String>::new())
        .context("create self-signed certificate parameters")?;
    params.subject_alt_names = sans;
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
        .context("generate self-signed certificate private key")?;
    let cert = params
        .self_signed(&key)
        .context("self-sign TLS certificate")?;

    // Log the SHA-256 fingerprint of the new certificate so operators can
    // verify it on paired devices (trust-on-first-use). An attacker with write
    // access to the cert directory could delete these files to force a silent
    // re-mint; this warning makes that visible on every fresh generation.
    let fingerprint = hex::encode(Sha256::digest(cert.der().as_ref()));
    warn!(
        %fingerprint,
        cert = %paths.cert.display(),
        "generated a new self-signed TLS certificate; verify this fingerprint on paired devices"
    );

    // Ensure the directory exists even before the first atomic write, then
    // persist both files with restrictive permissions.
    fsutil::create_dir_all(cert_dir)
        .with_context(|| format!("create TLS certificate directory {}", cert_dir.display()))?;
    fsutil::atomic_write(&paths.cert, cert.pem().as_bytes(), Some(0o600))
        .with_context(|| format!("write TLS certificate {}", paths.cert.display()))?;
    fsutil::atomic_write(&paths.key, key.serialize_pem().as_bytes(), Some(0o600))
        .with_context(|| format!("write TLS private key {}", paths.key.display()))?;

    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn generates_persistent_pem_certificate_and_key() {
        let temp = tempfile::tempdir().expect("temporary certificate directory");
        let first = ensure_self_signed(temp.path(), "localhost").expect("generate certificate");
        let first_cert = fs::read(&first.cert).expect("read certificate");
        let first_key = fs::read(&first.key).expect("read key");
        assert!(String::from_utf8_lossy(&first_cert).contains("BEGIN CERTIFICATE"));
        assert!(String::from_utf8_lossy(&first_key).contains("BEGIN PRIVATE KEY"));

        let second = ensure_self_signed(temp.path(), "localhost").expect("reuse certificate");
        assert_eq!(first, second);
        assert_eq!(
            fs::read(second.cert).expect("read reused certificate"),
            first_cert
        );
        assert_eq!(fs::read(second.key).expect("read reused key"), first_key);
    }
}
