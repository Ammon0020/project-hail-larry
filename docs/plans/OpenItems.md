# OpenItems

Tracked gaps and decisions to address in the Local Agent Interface blueprint and implementation. Check items off as they land in `Blueprint.md` or ship in code. Delete sections when fully resolved.

---

## 3. Medium Priority

- [~] **Multi-user vs multi-device** — **Decision: multi-device, single user.** The daemon runs on the host; phone + laptop (and any other paired device) are thin clients that sync to the host in real time via the WebSocket hub. There is no multi-user/multi-tenant surface — all paired devices share one user's workspaces, sessions, and credentials. Reconnection/live sync robustness (mid-session Wi‑Fi drops, in-flight permission prompts) remains open.

---

## 4. Lower Priority / Future

- [x] **Missing workspace user warning** — Done (Go+Rust): retain unavailable
  roots, list/CLI `available:false` + error, no config auto-prune. Story:
  `docs/plans/stories/pending-missing_workspace_user_warning-med-med.md`
- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [x] **Editor on mobile** — CodeMirror touch config in `EditorPane.tsx`:
  scaled line height, no text DnD/fold gutter, soft-keyboard keep-in-view
- [ ] **Session replay** — Superseded by the active agent-owned history epic:
  `docs/plans/active-acp-agent-session-history-med.md` →
  `docs/plans/acp-session-history/`. Keep until epic Q1–Q8 lock direction.
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature — noted in Blueprint Phase 3
- [ ] **ACP sub-workers** — Deferred until next ACP release (~next quarter)

---

## Profiles over ACP

The original profile epic is complete. Its tool-enumeration design was
superseded by profile-level MCP-server allowlists; the ACP-safe transition UX
is tracked in `docs/plans/other_tasks/active-profile-mcp-transition-hard-high.md`.
