# S-CTX-LEDGER — Session context ledger and delta engine

## Outcome

Build a deterministic, session-scoped ledger that decides exactly which
client-owned context block accompanies a prompt and explains why. It sends a
fresh compact snapshot only when the session/harness cannot safely rely on the
previous one; otherwise it sends source-specific deltas.

## Work

1. Model source revisions for profile fallback text, root inventory,
   `AGENTS.md`, open files, recent edits, and explicit selection. Hash bounded,
   normalized values; never hash/read arbitrary paths outside the workspace.
2. Store the last sent revision per source and an ACP actor/session generation.
   Reset the ledger on close, rebind, restore, actor replacement, harness
   restart, workspace change, or explicit refresh.
3. Produce named context actions: `initial_snapshot`, `source_changed`,
   `manual_refresh`, `suppressed_by_policy`, and `unchanged`. Persist only the
   actual sent additions in the event trace; keep comparison hashes transient.
4. Send open/recent state as a bounded relative-path delta (`added`, `removed`,
   `active`) rather than a full list every prompt. Coalesce rapid editor
   reports before changing a revision.
5. Treat selection as explicit, bounded context with its own revision. Empty
   selection removes selection context and emits no body. Do not turn normal
   open tabs into file-content resources.
6. Prefer an advertised ACP profile/mode config option. For fallback agents,
   inject profile instructions once and a one-time replacement when the
   profile revision changes.
7. Define multi-device ownership before implementation: include the prompting
   device's reported editor state, or a documented union with source labels;
   never allow an unseen last writer to silently replace another device's
   context.

## Acceptance

- Prompt 1 gets only sources allowed by its policy; unchanged prompt 2 gets no
  repeated client context.
- Each state change generates one minimal, attributable delta.
- Reset/rebind sends a new compact snapshot, never a stale delta.
- The user-facing trace names source, reason, item count, truncation, and
  exact sent text. It does not claim a hidden harness system prompt is visible.
- Cancellation/failure before dispatch does not advance the ledger; durable
  prompt append and ledger advancement have a defined ordering.

## Edge cases

- ACP agent rejects a prompt/resource after persistence: retain the trace as
  attempted or mark it failed; do not falsely mark it delivered.
- Concurrent prompts/cancel: serialize one generation's ledger mutations with
  actor turn ownership; nested prompts must not race snapshots.
- Agent-side history compaction: manual refresh remains available, and a
  policy may opt into periodic re-snapshots later without changing defaults.
- Selection/open-file paths with `..`, roots, symlinks, invalid UTF-8, or
  duplicate case variants are rejected/normalized before ledger comparison.

## Verify

Use a mock ACP agent to capture every `session/prompt` block. Cover resource
and fallback transport, first/second prompt, add/remove, profile change,
selection change, rebind, actor crash, cancellation, and two-device reports.

