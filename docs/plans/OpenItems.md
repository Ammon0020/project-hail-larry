# OpenItems

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 2. High Priority

_All items resolved. TLS on LAN (Work Stream 4a) and Event persistence (Phase 1) both shipped._

- [x] **TLS on LAN** — Self-signed ECDSA P-256 cert generated on first daemon start (trust-on-first-use: reused, never overwritten). SANs include localhost, 127.0.0.1, and all LAN IPv4 addresses. Configurable via `tlsEnabled` / `tlsCertDir` / `httpsPort` in config.json. When `tlsEnabled` is true (default) the daemon runs in **dual HTTP+HTTPS mode**: HTTP on `host:port` (default `0.0.0.0:7337`) and HTTPS on `host:httpsPort` (default `port + 1` → `7338`) simultaneously, so the user picks a scheme by typing `http://` or `https://` in the browser without restarting. `tlsEnabled: false` falls back to HTTP-only. (Work Stream 4a)
- [x] **Event persistence** — SQLite event store (`internal/events`) in WAL mode; append-only log with query/replay. Daemon recovers state after restart. (Phase 1)

---

## 3. Medium Priority

- [x] **Pairing TTL** — Configurable via `pairingTtlSeconds` in config.json (default 300s / 5 min). `pairing.Manager.SetTTL` setter wired from daemon. (Work Stream 4b)
- [x] **ACP transport end-to-end verification** — Verified 2026-06-27 with `mistral-vibe` / `devstral-small`: full flow daemon → handshake → session → prompt → workspace-context injection → `fs/read_text_file` tool call → 245 streaming chunks → clean completion. No permission prompts needed (file reads auto-approved). Not yet verified: `LoadSession` across restart, shell-command permission prompts, UI-side event rendering.
- [x] **Events endpoint default limit** — Raised from 100 to 1000 in `internal/server/api.go` and `internal/events/events.go`. `?limit` still works for callers wanting fewer. Test added (`TestGetSessionEventsDefaultLimit`). Fixed 2026-06-27.
- [x] **`GET /api/sessions/{id}` returns 404** — Fixed 2026-06-27: added `GET /api/sessions/{id}` endpoint returning full `SessionInfo` (id, name, status, agentId, modelId, workspace, createdAt, updatedAt). `SessionInfo` struct expanded with 5 fields; `sessionToInfo` helper shared by create/get/rebind/list handlers. Test added (`TestGetSession`).
- [x] **Device credential expiry** — Implemented 2026-07-12: sliding inactivity window. Each successful auth renews `LastSeen` in `internal/pairing/Manager.ValidateCredential` (throttled disk persist). Default 30 days (2592000s) for fresh installs via `defaultCredentialInactivityTTLSeconds`; legacy config files that omit the field load as 0 (disabled) to avoid silently re-enabling expiry on upgrade. Set `credentialInactivityTtlSeconds` in `~/.local-agent/config.json`; `0` = never expire. Wired from daemon via `pairing.Manager.SetInactivityTTL`.
- [x] **Reconnection behavior** — Implemented 2026-07-12: (1) WS exponential backoff + jitter (1s→30s), immediate reconnect on `online` / tab-foreground `visibilitychange`, no stacked sockets. (2) On reconnect: re-sync sessions, events (cursor merge), pending permissions, active workspace. (3) 60s daemon ticker runs `permissions.CleanupStale()` so 5min-old prompts auto-deny even with all devices offline. (4) SPA shell offline cache via hand-written service worker (`web/public/sw.js`) for reload-while-offline. Live data still needs the network.
- [x] **Image upload flow** — Implemented: `POST /api/sessions/{id}/uploads` (multipart) stores the file and returns an upload ID; `GET /api/sessions/{id}/uploads/{uploadID}` serves it back. `sendPrompt` accepts `attachments: [{uploadID, mimeType, name}]` which the daemon injects into the ACP prompt as image content blocks. Frontend `api.ts` `uploadFile` + `ChatPanel`/`ChatComposer` attach images and render them inline.
- [~] **Multi-user vs multi-device** — **Decision: multi-device, single user.** The daemon runs on the host; phone + laptop (and any other paired device) are thin clients that sync to the host in real time via the WebSocket hub. There is no multi-user/multi-tenant surface — all paired devices share one user's workspaces, sessions, and credentials. Reconnection/live sync robustness (mid-session Wi‑Fi drops, in-flight permission prompts) is still open — see "Reconnection behavior" above.
- [x] **ACP spec compliance (P1+P2+P4 near-term)** — Deviations and gaps from the audit are implemented. MCP provisioned end-to-end. AdditionalDirectories (P4.5) + MCP health UX shipped 2026-07-12. Remaining future: elicitation, MCP-over-ACP, provider management, session fork. Plan: `docs/plans/acp-spec-compliance.md`.

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
