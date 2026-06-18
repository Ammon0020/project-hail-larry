# Open Items

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 1. Terminal / Shell Execution

**Decision:** The web UI does **not** need an interactive terminal panel (no xterm-style tab). The host daemon **does** need headless shell execution so agents can run commands via ACP. The host `app` CLI remains the only terminal surface for setup/admin.

_All items addressed in Blueprint v2.3._

---

## 2. High Priority

- [ ] **Collision prevention mechanics** — How does the host prevent overwrite collisions across paired clients? (e.g., host serializes all file writes; clients read-only until explicit user edit in a later phase)
- [ ] **Permission prompt routing** — When multiple devices are paired, which client(s) receive `session/request_permission`? (e.g., broadcast to all; first response wins; or pin to device that sent the last prompt)
- [ ] **Network discovery** — Document bind address (`0.0.0.0`), default port, and optional mDNS/Bonjour (e.g., `app.local`)
- [ ] **Host CLI surface** — Document full `app` command set beyond `add-folder` and `pair` (e.g., `start`, `stop`, `status`, `devices`, `revoke`)
- [ ] **Daemon lifecycle** — Install, autostart on boot, upgrade, crash recovery
- [ ] **Editor / file viewing** — How users view file contents and agent-proposed diffs in the web UI
- [ ] **Sub-workers** — Define in terminology: what "spins up sub-workers" means (ACP child sessions, parallel workers, agent-internal?)

---

## 3. Medium Priority

- [ ] **Pairing TTL** — How long is a pairing session valid before QR/mnemonic expires?
- [ ] **Device credential expiry** — Permanent until revoked, or time-limited?
- [ ] **Reconnection behavior** — Phone drops Wi‑Fi mid-session; WebSocket reconnect; in-flight permission prompts
- [ ] **Image upload flow** — How whiteboard photos / images reach the agent via ACP
- [ ] **Event persistence** — Where the event log lives on the host (e.g., SQLite, files)
- [ ] **Multi-user vs multi-device** — One user's devices only, or can multiple people pair to the same daemon?

---

## 4. Lower Priority / Future

- [ ] **TLS on LAN** — Optional HTTPS for local network traffic
- [x] **Remote / internet access** — Non-goal in Blueprint v2.3
- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [ ] **Session replay** — Implementation details
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature (user-initiated shell, not agent-initiated) — noted in Blueprint Phase 3

---

## 5. Blueprint Hygiene

- [ ] Add **Permission Routing** subsection (see item 2)
- [ ] Add **Write Serialization** subsection (see item 2)

---

## Resolved

- [x] **Terminal / shell execution split** — Blueprint v2.3: Section 15 Shell Execution, Non-Goals, tool timeline output, Phase 1/3 scope
- [x] **Non-Goals section** — Blueprint v2.3 (terminal UI, remote access, provider APIs)
- [x] **Phase 1 shell execution scope** — Blueprint v2.3
- [x] **Blueprint version bump** — v2.3
- [x] **ACP-only architecture** — Blueprint v2.1
- [x] **Device pairing auth flow** — Blueprint v2.2
