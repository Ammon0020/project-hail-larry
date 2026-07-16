# Story S-CONFIG: Config Storage

> **Phase:** 1 | **Depends on:** — | **Go source:** `internal/config/` (311 lines)

## Summary

Port config persistence: TOML config file at `~/.local-agent/config.toml`,
atomic writes, workspace list, TLS settings, port config.

## Go Source

`internal/config/config.go` — `Config` struct, `Load`, `Save` (atomic
write: write temp → rename), workspace add/remove, device credential
storage.

## Rust Implementation

- Use `toml` + `serde` for serialization (replaces `pelletier/go-toml/v2`)
- Atomic writes: temp file in the same directory → fsync file → rename → fsync
  parent directory where supported; preserve existing permissions.
- Preserve the established `~/.local-agent/` location and Go TOML field names.
  Do not substitute platform config directories during a compatibility port.
- Port `config_test.go`

## Acceptance Criteria

- [x] Config round-trips through TOML without data loss
- [x] Atomic write (temp + rename) — no corruption on crash
- [x] `cargo test config` passes
- [ ] Config file format is compatible, or S-MIGRATE provides a tested atomic migration
- [x] Existing file permissions and unknown compatible fields are not silently weakened or lost
