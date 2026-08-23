# S-CTX-UI — Context controls and evidence UI

## Outcome

Make prompt context a user-controlled, auditable feature rather than invisible
prompt engineering.

## Work

1. Add an Agent Context settings area with global Minimal defaults and explicit
   per-harness policy override. Keep source budgets and enablement together;
   keep profile/MCP settings separate.
2. Add a session-only override and a visible "Refresh context on next prompt"
   action. Explain that refresh is useful after an agent restart/compaction.
3. Extend the existing Context Added disclosure with source, action reason,
   truncation, and policy suppression. Show actual sent text/resources only.
4. Make controls accessible on mobile, keyboard operable, loading/error safe,
   and resilient to a stale setting update from another paired device.
5. Update conversation export rules: include context metadata only when the
   user opts in, and redact resource text by default if exports leave the host.

## Acceptance

- User can answer “why was this sent?” from the trace without trusting model
  reasoning about hidden system messages.
- User can choose a Cursor-like native-workspace policy without affecting a
  Mistral session.
- Changes affect subsequent prompts only and have clear restart/rebind notes.

