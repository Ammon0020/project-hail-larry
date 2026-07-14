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
- Atomic writes: write to `config.toml.tmp` → `fs::rename` (atomic on same
  filesystem). Use `std::fs` or `tokio::fs`
- Config dir: `dirs::config_dir()` or `std::env::var("HOME")` + `.local-agent/`
- Port `config_test.go`

## Acceptance Criteria

- [ ] Config round-trips through TOML without data loss
- [ ] Atomic write (temp + rename) — no corruption on crash
- [ ] `cargo test config` passes
- [ ] Config file format compatible (or documented migration path)
