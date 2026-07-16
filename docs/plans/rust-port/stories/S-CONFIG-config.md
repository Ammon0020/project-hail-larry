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
- Atomic writes: **`crate::fsutil::atomic_write`** (`atomic-write-file` crate:
  temp + fsync + mode `0600` + rename + parent fsync). Do not re-inline the
  helper; config, MCP, and conversation store share it.
- Home / state dir: **`crate::fsutil::home_dir`** (`dirs` crate) +
  `LOCAL_AGENT_STATE_DIR` override. Preserve the established `~/.local-agent/`
  location and Go camelCase field names — do not move to XDG config dirs during
  the compatibility port.
- Port `config_test.go`

## Acceptance Criteria

- [x] Config round-trips through TOML without data loss
- [x] Atomic write (temp + rename) — no corruption on crash
- [x] `cargo test config` passes
- [ ] Config file format is compatible, or S-MIGRATE provides a tested atomic migration
- [x] Existing file permissions and unknown compatible fields are not silently weakened or lost
