# Open Items

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 2. High Priority

_All items addressed in Blueprint v2.4 except sub-workers (deferred)._

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
- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [ ] **Session replay** — Implementation details
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature — noted in Blueprint Phase 3
- [ ] **Client-initiated file editing** — Revision conflict UI; deferred past v1 read-only Monaco
- [ ] **ACP sub-workers** — Deferred until next ACP release (~next quarter)

---

## Resolved

- [x] **Collision prevention** — Blueprint v2.4: host-authoritative writes, revision numbers, event-stream reads, v1 read-only editor
- [x] **Permission prompt routing** — Blueprint v2.4: broadcast to all devices, first response wins
- [x] **Network discovery** — Blueprint v2.4: `0.0.0.0:7337`, mDNS `app.local`, QR + `app status` fallback
- [x] **Host CLI surface** — Blueprint v2.4: full command list in help format
- [x] **Daemon lifecycle** — Blueprint v2.4: install/start/autostart/crash/upgrade table
- [x] **Editor / file viewing** — Blueprint v2.4: Monaco read-only + diff editor
- [x] **Permission Routing subsection** — Blueprint v2.4 §8
- [x] **Write Serialization subsection** — Blueprint v2.4 §14 Collision Prevention
- [x] **Terminal / shell execution split** — Blueprint v2.3
- [x] **Non-Goals section** — Blueprint v2.3
- [x] **Device pairing auth flow** — Blueprint v2.2
- [x] **ACP-only architecture** — Blueprint v2.1
- [x] **Remote / internet access** — Non-goal in Blueprint v2.3
