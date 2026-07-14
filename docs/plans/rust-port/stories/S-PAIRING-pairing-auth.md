# Story S-PAIRING: QR + Mnemonic Pairing & Auth

> **Phase:** 3 | **Depends on:** S-CONFIG | **Go source:** `internal/pairing/` (1,190 lines)

## Summary

Port device pairing: QR code generation, four-word mnemonic passcode,
device credential issuance (hashed at rest), sliding-TTL expiry, grace-
period revocation, auth validation (Bearer token / WS query params).

## Go Source

`internal/pairing/` — `Manager`, QR generation (`skip2/go-qrcode`),
mnemonic generation (BIP-39-style word list), device credential hashing
(`crypto/sha256`), TTL expiry, grace-period revocation, auth check.

## Rust Implementation

- QR: `qrcode` crate (see `docs/rust-ecosystem/cli-and-config.md`)
- Mnemonic: port the word list + generation logic (pure string ops)
- Credential hashing: `sha2::Sha256`, `hex` crate
- Random bytes: `rand` crate (`OsRng` for cryptographic randomness)
- TTL: `tokio::time::Instant` + background expiry sweep task
- Grace-period revocation: `CancellationToken` per pending revocation
- **Security: never log raw tokens/passcodes** — port the same care
- Port all tests

## Security Notes

This is a security-critical package. Ensure:
- Device credentials hashed at rest (never raw)
- Raw tokens/passcodes never logged
- Rate limiting on pairing endpoints (enforced in S-SERVER)
- Constant-time comparison for credential checks (`subtle` crate)

## Acceptance Criteria

- [ ] QR code encodes HTTPS pairing URL
- [ ] Four-word mnemonic passcode generated correctly
- [ ] Device credentials hashed at rest
- [ ] Pairing flow: initiate → verify-passcode → verify-token
- [ ] Sliding-TTL expiry works
- [ ] Grace-period revocation (any device can cancel)
- [ ] Auth validation (Bearer header + WS query params)
- [ ] No raw secrets in logs
- [ ] `cargo test pairing` passes
