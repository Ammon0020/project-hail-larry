# Open Items

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 1. Terminal / Shell Execution

**Decision:** The web UI does **not** need an interactive terminal panel (no xterm-style tab). The host daemon **does** need headless shell execution so agents can run commands via ACP. The host `app` CLI remains the only terminal surface for setup/admin.

- [ ] Rename Blueprint Section 15 from **Terminal Access** → **Shell Execution**
- [ ] Update **Client Ownership** — replace "The terminal" with "Shell execution (on behalf of agents)"
- [ ] Update **Capability Negotiation** — clarify "terminal access" means ACP shell execution, not a terminal UI
- [ ] Update **Design Philosophy** — replace "filesystem, and terminal" with "filesystem, and shell execution"
- [ ] Document shell execution flow: permission prompt → workspace-scoped subprocess → stdout/stderr streamed as events → results returned to agent via ACP
- [ ] Document that command output appears in the **session tool timeline** (expandable), not a terminal pane
- [ ] Explicitly list **terminal UI panel** as a non-goal for v1
- [ ] (Optional, Phase 3) Note possible future **developer terminal** for user-initiated manual shell — separate from agent shell execution

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
- [ ] **Remote / internet access** — Out of scope for v1; document explicitly in Non-Goals
- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [ ] **Session replay** — Implementation details
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature (user-initiated shell, not agent-initiated)

---

## 5. Blueprint Hygiene

- [ ] Add a **Non-Goals** section to `Blueprint.md` (terminal UI v1, public internet access v1, provider-specific APIs)
- [ ] Add **Permission Routing** subsection (see item 2)
- [ ] Add **Write Serialization** subsection (see item 2)
- [ ] Align **Phase 1** scope: include headless shell execution; exclude terminal UI panel
- [ ] Bump blueprint version when open items are incorporated

---

## Resolved

_Move completed items here with date, then delete periodically._

<!-- Example:
- [x] **ACP-only architecture** — Done in Blueprint v2.1 (2026-06-18)
- [x] **Device pairing auth flow** — Done in Blueprint v2.2 (2026-06-18)
-->
