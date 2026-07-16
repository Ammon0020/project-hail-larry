# OpenItems

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 3. Medium Priority

- [~] **Multi-user vs multi-device** — **Decision: multi-device, single user.** The daemon runs on the host; phone + laptop (and any other paired device) are thin clients that sync to the host in real time via the WebSocket hub. There is no multi-user/multi-tenant surface — all paired devices share one user's workspaces, sessions, and credentials. Reconnection/live sync robustness (mid-session Wi‑Fi drops, in-flight permission prompts) remains open.

---

## 4. Lower Priority / Future

- [ ] **Missing workspace user warning** — Do not auto-prune missing/invalid workspace paths from config; show a user-visible warning (UI list + CLI) and let the user remove them. Temporary auto-prune lives in `daemon.go` today. Story: `docs/plans/stories/pending-missing_workspace_user_warning-med-med.md`
- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [ ] **Editor on mobile** — CodeMirror 6 is lighter than Monaco but still needs touch-optimized configuration (larger line heights, disable drag-and-drop, simplified gutter) for small edits on phones
- [ ] **Session replay** — Implementation details
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature — noted in Blueprint Phase 3
- [ ] **ACP sub-workers** — Deferred until next ACP release (~next quarter)
