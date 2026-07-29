# Permission approvals: show exact grant scope, add tool-kind scoping

> Status: **complete** (2026-07-29). Part 1 (transparency) and Part 2
> (tool-kind scoping) both implemented and passing `make check`.

> Difficulty: medium. Urgency: medium.
> Source: user question while reviewing the assistant-ui chat migration
> (2026-07-28) — "I would like to see exactly what 'Always allow' is
> allowing" and "I might want to allow during a session, or allow renames,
> not just 'rename that specific file'."

## Current behavior (confirmed by reading the code, not assumed)

`src/permissions/manager.rs::policy_key_for` scopes every durable decision to
`(session_id, tool_title, target_path_or_command_text)`:

- **Allow for session** — caches the decision for that exact file path (or
  exact shell command text) for the rest of the current session only.
- **Always allow** — same key, but `session_id` is blanked (`global_key`), so
  the grant persists across all sessions, forever, for that one exact file or
  command string.
- There is **no** broader scope: no "allow all renames", no "allow all edits
  under `src/`", no "allow this tool kind regardless of target". Every grant
  is pinned to one literal path or command string.
- The web UI (`ToolFallback.Approval` in `web/src/components/assistant-ui/
  tool-fallback.tsx`, wired from `web/src/lib/chatConverter.ts`) resolves the
  decision immediately on click. `ToolApprovalOption.grants` and
  `option.confirm` exist in the library specifically to preview what a
  decision will persist, but the converter never populates them, so the user
  currently gets zero preview before an "Always allow" click takes effect.

## Goal

1. **Transparency**: before an "Always allow" / "Allow for session" decision
   resolves, show the user exactly what will be persisted (tool + exact
   target/command + scope: session vs. forever) using the confirm step the
   library already supports.
2. **New scope tier (optional, larger)**: let the user additionally choose a
   broader grant — e.g. "always allow this tool kind" (all `move`/rename
   operations, regardless of target) or "always allow within this directory
   prefix" — as an explicit, separately-labeled option, never the default.

## Scope

### In scope — Part 1 (transparency, do this first)

1. **Backend**: no changes needed — the manager already has the exact
   `(tool, target, command, session vs. global)` tuple used as the grant key.
   The only gap is that this tuple isn't surfaced to the frontend in a
   renderable form.
2. **`web/src/lib/chatConverter.ts`** (`approvalOptionsFor`): for the
   `allow_session`, `allow_always`, and `reject_always` decision kinds,
   populate `grants: [describeGrantScope(pending)]` and `confirm: true` on
   the option. `describeGrantScope` renders a one-line human string, e.g.:
   - File-oriented: `` `edit_file` on `src/main.rs` — forever, all sessions ``
   - Shell-oriented: `` `execute`: `npm test` — forever, all sessions ``
   - Session-scoped variant swaps "forever, all sessions" for "for this
     session only".
   `allow_once` / `deny` (one-shot decisions) get no `confirm` — only durable
   grants need a preview.
3. **`web/src/components/assistant-ui/tool-fallback.tsx`**: no changes — the
   vendored confirm step already renders `grants` as a list and blocks on an
   explicit "Confirm" click; this task only needs to populate the data it
   already knows how to display.

### In scope — Part 2 (tool-kind scoping, larger, do after Part 1 ships)

4. **`src/interfaces/types.rs`**: extend `PermissionDecision` (or add a
   sibling enum) with a `AllowToolKindAlways` variant — mirror the existing
   `allow_always` naming so it round-trips the same way.
5. **`src/permissions/manager.rs`**: add a second policy key,
   `PolicyKey { session_id: "", tool_kind, target: "", command: "" }`
   (target/command blanked), checked as a fallback after the exact-key
   lookup in the cache-check block (~line 353-361). Document the security
   trade-off inline: this is intentionally coarser and should probably
   require the option to carry `confirm: true` with an explicit "this
   affects ALL `<tool_kind>` operations, not just this one" `grants` entry.
6. **`src/acp/core/handlers/permission.rs`**: this new decision is
   client-only (no ACP `PermissionOptionKind` maps to it) — it must be
   synthesized as an *additional* option appended to `option_details`
   alongside whatever the agent declared, not derived from `request.options`.
   Gate it behind a config flag or only offer it for a conservative allowlist
   of tool kinds (e.g. `move`/rename, not `execute` — a blanket "always allow
   all shell commands" is a much bigger risk to reintroduce here).
7. **Frontend**: `approvalOptionsFor` appends this synthesized option (its
   own `id`, distinct from the backend `PermissionDecision` values above —
   needs a matching `respond_permission` handler branch since it isn't part
   of the ACP-sourced `option_details`).

### Out of scope

- Directory-prefix scoping ("allow all edits under `src/`") — flagged as a
  possible follow-up in the goal above, but not designed here; the exact-path
  policy key would need a prefix-match mode, which changes cache-lookup
  complexity from O(1) map lookup to a scan. Revisit only if tool-kind
  scoping (Part 2) turns out to be insufficient in practice.
- Changing what `allow_once` / plain `deny` do (already one-shot, no grant to
  preview).
- Editing/revoking previously-granted policies via the UI — the manager has
  no persistence or listing API for the in-memory `policy`/`denied` sets
  today; that's a separate, larger feature (audit log exists via
  `get_audit_log`, but it's a log, not a live revocable grant list).

## Acceptance criteria

- [ ] Clicking "Always allow" or "Allow for session" on a tool-approval card
      shows a confirm step naming the exact tool, target/command, and
      persistence scope before the decision is sent.
- [ ] `allow_once` and plain `deny` are unaffected (no confirm step added).
- [ ] (Part 2) A new, clearly-labeled "Always allow all `<tool_kind>`"
      option is available, gated to a conservative tool-kind allowlist, and
      itself requires the confirm step with explicit warning language.
- [ ] `make check` passes (frontend eslint/build + `cargo test` +
      contract suite, since `PermissionDecision`/`PermissionRequest` are
      contract-tested DTOs).

## Verification

1. `make qcheck` — autofix fmt/lints + quiet tests.
2. `make check` — full gate.
3. Manual: trigger a file-edit permission prompt and a shell-command prompt
   against a real or mock agent; click "Always allow" on each and confirm
   the preview text matches the actual policy key before/after checking
   `GET /api/permissions/pending` and re-triggering the same request (should
   auto-resolve without re-prompting).

## File references

- `src/permissions/manager.rs` (`policy_key_for`, `request`)
- `src/acp/core/handlers/permission.rs` (`option_details` construction)
- `src/interfaces/types.rs` (`PermissionDecision`, `PermissionOptionInfo`)
- `web/src/lib/chatConverter.ts` (`approvalOptionsFor`)
- `web/src/components/assistant-ui/tool-fallback.tsx` (confirm/grants
  rendering — vendored, already supports this, no changes needed for Part 1)

## Depends on

None. Part 2 depends on Part 1 shipping first (the confirm-step plumbing
Part 2's new option relies on).
