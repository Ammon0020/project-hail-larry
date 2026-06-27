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
- [x] **`GET /api/sessions/{id}` returns 404** — Fixed 2026-06-27: added `GET /api/sessions/{id}` endpoint returning full `SessionInfo` (id, name, status, agentId, modelId, workspace, createdAt, updatedAt). `SessionInfo` struct expanded with 5 fields; `sessionToInfo` helper shared by create/get/rebind/list handlers. Test added (`TestGetSession`).
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

---

## 5. Feature Requests (UI Polish)

_User-requested features to implement when prioritizing UI work._

- [x] **Smart autoscroll in chat** — Implemented 2026-06-27: `useAutoscroll` hook tracks "near bottom" state (80px threshold), autoscrolls on new content only when user was near bottom, shows floating "jump to bottom" button (`ChevronDown`) when scrolled up.
- [x] **Adjustable panel widths** — Implemented 2026-06-27: left/right sidebars resizable via drag handles (clamped 180–480px / 300–700px), widths persisted to `lai:leftPanelWidth`/`lai:rightPanelWidth` localStorage, desktop only.
- [x] **System message cleanup** — Implemented 2026-06-27: `ResponseStarted`, `ConnectionRestarted`, `SessionCancelled`, `FileRevisionUpdated` now render as compact centered muted rows (`text-xs text-muted-foreground`); `AgentExited` uses `text-destructive` for visibility. User/agent messages and tool cards unchanged.
