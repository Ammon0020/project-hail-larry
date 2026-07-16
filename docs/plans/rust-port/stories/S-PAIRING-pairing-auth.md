# Story S-PAIRING: QR + Mnemonic Pairing & Auth

> **Phase:** 3 | **Depends on:** S-CONFIG, S-MIGRATE | **Go source:** `internal/pairing/` (1,190 lines)

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
- **Mnemonic: port the English word list + `generatePasscode` only** (4
  hyphen-joined words sampled with `OsRng`). **Do not** pull a full `bip39`
  crate — Go is not BIP-39 entropy+checksum; only the word list is shared.
- Credential hashing: `sha2::Sha256`, `hex` crate
- Random bytes: `rand` crate (`OsRng` for cryptographic randomness)
- Constant-time compare: `subtle` crate (port Go `crypto/subtle` usage)
- Device credential / session state persistence: `crate::fsutil::atomic_write`
  when writing secret-bearing files under `~/.local-agent/`.
- Optional hygiene: `zeroize` / `secrecy` for in-memory passcodes if easy;
  never required for parity if logging discipline matches Go.
- Persisted expiry uses a wall-clock timestamp compatible with existing state;
  use monotonic time only for in-process delays.
- Grace-period revocation: `CancellationToken` per pending revocation.
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
- [ ] Constant-time credential comparisons are used
- [ ] No raw secrets in logs
- [ ] Existing Go-created device credential state remains valid through S-MIGRATE
- [ ] `cargo test pairing` passes
