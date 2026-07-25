# Plaintext pairing token written to disk as QR PNG; survives crash before cleanup

- **Difficulty:** easy
- **Urgency:** low
- **File:** `src/pairing/mod.rs`
- **Lines:** 161-185 (create_session), 647-660 (write_qr), 440 (issue_credential cleanup), 563-572 (cleanup_sessions)

## Description

`create_session` writes the full pairing URL (`http://host:port?token=<256-bit token>`) as a PNG to `data_dir/pairing-{id}.png` with mode 0600 (good), but the token is stored in plaintext in the image. The file is deleted on successful pairing (`issue_credential`, line 440) and on session expiry/cleanup (`cleanup_sessions`, lines 567-569). If the daemon crashes or is killed between `create_session` and cleanup, the PNG — and thus the still-valid (up to `session_ttl`, default 5 min) pairing token — remains on disk. Any process or backup that reads `~/.local-agent/pairing-*.png` within the TTL can complete pairing without the mnemonic. The token is single-use and short-lived, limiting impact, but it is a plaintext secret at rest outside the hashed `devices.json`.

## Recommendation

Either (a) regenerate the QR on demand from an in-memory token rather than persisting the PNG, or (b) on `Manager::new`, scan `data_dir` for stale `pairing-*.png` files and remove them (since their in-memory sessions no longer exist), and ensure `close`/`Drop` removes them too.

## Verification

`write_qr(&url, &qr_path)` (pairing/mod.rs:169) persists the URL containing `token`. `issue_credential` removes the file only on success (line 440). `cleanup_sessions` removes it only for expired/used sessions while the manager is running (lines 567-569). `Manager::new` (lines 124-145) and `Drop`/`close` (lines 392-407) do not scan for or remove orphaned `pairing-*.png` files.
