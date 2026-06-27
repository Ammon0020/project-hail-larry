# OpenItems

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 2. High Priority

_All items resolved. TLS on LAN (Work Stream 4a) and Event persistence (Phase 1) both shipped._

- [x] **TLS on LAN** — Self-signed ECDSA P-256 cert generated on first daemon start (trust-on-first-use: reused, never overwritten). SANs include localhost, 127.0.0.1, and all LAN IPv4 addresses. Configurable via `tlsEnabled` / `tlsCertDir` in config.json. (Work Stream 4a)
- [x] **Event persistence** — SQLite event store (`internal/events`) in WAL mode; append-only log with query/replay. Daemon recovers state after restart. (Phase 1)

---

## 3. Medium Priority

- [x] **Pairing TTL** — Configurable via `pairingTtlSeconds` in config.json (default 300s / 5 min). `pairing.Manager.SetTTL` setter wired from daemon. (Work Stream 4b)
- [x] **ACP transport end-to-end verification** — Verified 2026-06-27 with `mistral-vibe` / `devstral-small`: full flow daemon → handshake → session → prompt → workspace-context injection → `fs/read_text_file` tool call → 245 streaming chunks → clean completion. No permission prompts needed (file reads auto-approved). Not yet verified: `LoadSession` across restart, shell-command permission prompts, UI-side event rendering.
- [x] **Events endpoint default limit** — Raised from 100 to 1000 in `internal/server/api.go` and `internal/events/events.go`. `?limit` still works for callers wanting fewer. Test added (`TestGetSessionEventsDefaultLimit`). Fixed 2026-06-27.
- [ ] **`GET /api/sessions/{id}` returns 404** — Only `GET /api/sessions` (list) is implemented; there is no single-session fetch endpoint. Minor API gap. Discovered during E2E test 2026-06-27.
- [ ] **Device credential expiry** — Permanent until revoked, or time-limited?
- [ ] **Reconnection behavior** — Phone drops Wi‑Fi mid-session; WebSocket reconnect; in-flight permission prompts
- [ ] **Image upload flow** — How whiteboard photos / images reach the agent via ACP
- [ ] **Multi-user vs multi-device** — One user's devices only, or can multiple people pair to the same daemon?

---

## 4. Lower Priority / Future

- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [ ] **Editor on mobile** — CodeMirror 6 is lighter than Monaco but still needs touch-optimized configuration (larger line heights, disable drag-and-drop, simplified gutter) for small edits on phones
- [ ] **Session replay** — Implementation details
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature — noted in Blueprint Phase 3
- [ ] **ACP sub-workers** — Deferred until next ACP release (~next quarter)
