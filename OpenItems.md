# Open Items

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 2. High Priority

_All items addressed in Blueprint v2.4 except sub-workers (deferred)._

- [ ] **TLS on LAN** — Plain HTTP exposes pairing tokens and file contents to anyone on the same network. Needed before claiming coffee-shop safety.
- [ ] **Event persistence** — Architecture is event-sourced but no storage layer is defined. Daemon can't recover state after restart without it.

---

## 3. Medium Priority

- [ ] **Pairing TTL** — How long is a pairing session valid before QR/mnemonic expires?
- [ ] **Device credential expiry** — Permanent until revoked, or time-limited?
- [ ] **Reconnection behavior** — Phone drops Wi‑Fi mid-session; WebSocket reconnect; in-flight permission prompts
- [ ] **Image upload flow** — How whiteboard photos / images reach the agent via ACP
- [ ] **Multi-user vs multi-device** — One user's devices only, or can multiple people pair to the same daemon?

---

## 4. Lower Priority / Future

- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [ ] **Monaco on mobile** — Monaco is heavy and poor for touch; need a lightweight fallback (e.g., textarea) for small edits on phones
- [ ] **Session replay** — Implementation details
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature — noted in Blueprint Phase 3
- [ ] **ACP sub-workers** — Deferred until next ACP release (~next quarter)
