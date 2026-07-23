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

## Profiles over ACP (complete epic — see `complete-profiles-over-acp-hard.md`)

- [ ] **Tool enumeration caching strategy** — When to call `tools/list` per MCP
  server (session-setup vs config-time vs lazy), TTL, and invalidation on MCP
  config change. S-PROF-TOOLS picks a default; confirm before UI relies on it.
- [ ] **Empty-vs-absent `tools` whitelist semantics** — `[]` = "no extra tools"
  vs absent key = "all tools"? Lock in the S-PROF-CONFIG schema.
- [ ] **Custom profile validation limits** — concrete caps for profile count,
  instruction length, file size, and allowed tool-name charset (reject path
  separators / shell metacharacters). Daemon writes/loads the file — validate.
- [ ] **REST `profile` field removal migration** — dropping it from
  `/sessions/:id/prompt` is a breaking wire change; clients must move to
  `POST /sessions/:id/profile` in the same release (S-PROF-ACP).
