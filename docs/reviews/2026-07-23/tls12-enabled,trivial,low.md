# TLS 1.2 enabled (rustls tls12 feature)

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `Cargo.toml`
- **Lines:** 55

## Description

`rustls = { version = "0.23", default-features = false, features = ["aws-lc-rs", "logging", "std", "tls12"] }` enables TLS 1.2. rustls's TLS 1.2 cipher suites are still secure (ECDHE + AEAD only; no RC4/3DES/CBC-with-weak-MAC), so this is not a vulnerability, but TLS 1.3-only would be stricter and removes the older handshake/renegotiation surface. Acceptable for client compatibility, noted for completeness.

## Recommendation

If LAN clients are all modern browsers, drop `tls12` and rely on TLS 1.3. Otherwise document the choice.

## Verification

`Cargo.toml:55` lists `"tls12"` in the rustls feature list. `app/tls.rs:16` installs `aws_lc_rs::default_provider()`.
