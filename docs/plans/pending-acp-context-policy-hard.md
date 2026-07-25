# Epic: ACP Context Policy

> **Status:** Pending. **Created:** 2026-07-25.
> **Related:** `docs/plans/agent-context.md` (superseded design reference),
> `docs/plans/Blueprint.md`, ACP prompt lifecycle and session configuration.

## Goal

Give every ACP session a small, explicit, inspectable context policy. The
daemon sends only context it owns and only when it changed or the session needs
an initial snapshot. It must not duplicate a harness's undocumented native
workspace, Git, or system context.

## Protocol position

ACP makes `cwd` part of `session/new`, and `session/prompt` carries a user
message plus capability-gated resources. It does not define a system-message
role, a workspace/Git injection convention, or a capability saying that an
agent already supplies native workspace context. `embeddedContext` means only
that an agent accepts embedded resources. Agent-advertised session config
options may carry a mode/profile, but cannot be assumed to exist.

Therefore the client owns injection scheduling; each harness gets an explicit
policy instead of an inferred one.

## Product decisions to lock in S-CTX-POLICY

1. **Default policy is Minimal.** Do not repeat `cwd`, platform, raw Git, or
   the current time in prompts. `cwd` already arrives through `session/new`.
2. **First usable prompt gets one compact snapshot:** optional bounded root
   inventory, bounded `AGENTS.md`, and fallback profile instructions only when
   the agent lacks a usable profile/mode config option.
3. **Later prompts are deltas.** Open-file state, selection, profile fallback,
   and tracked instruction files are sent only after their version changes.
   A rebind, restore, harness restart, or lost local ledger forces one fresh
   snapshot; this avoids relying on an agent's retained token memory.
4. **Git is opt-in and structured.** Suppress it for non-repositories, empty
   repositories, and all-untracked roots. Never inject raw porcelain output by
   default; a future enabled summary is branch + ahead/behind + bounded changed
   path counts.
5. **Harness behavior is not inferred.** A per-harness policy may select
   `minimal`, `native_workspace`, or `custom`; `embeddedContext` does not
   select any of these. New/unrecognized harnesses start at `minimal`.
6. **No automatic file bodies from tabs.** Explicit editor selections and
   `AGENTS.md` remain separately disclosed, bounded, and configurable.

## Story index

| ID | Story | Size | Depends on | Acceptance |
|---|---|---:|---|---|
| S-CTX-POLICY | [Policy contract and config migration](acp-context-policy/pending-ctx-policy-med.md) | med | — | Policy schema, migration, protocol notes locked |
| S-CTX-LEDGER | [Session context ledger and delta engine](acp-context-policy/pending-ctx-ledger-hard.md) | hard | POLICY | First snapshot + later deltas exact and inspectable |
| S-CTX-SOURCES | [Project-aware sources and Git summary](acp-context-policy/pending-ctx-sources-med.md) | med | LEDGER | No raw Git/tree dump; safe changed-source rules |
| S-CTX-UI | [Context controls and evidence UI](acp-context-policy/pending-ctx-ui-med.md) | med | POLICY, LEDGER | Global/harness/session controls and accurate trace |
| S-CTX-MATRIX | [Harness conformance matrix](acp-context-policy/pending-ctx-matrix-small.md) | small | POLICY | Cursor/Mistral/native behavior recorded; no inference |

## Boundaries

In scope: ACP prompt construction, persisted host settings, per-harness
overrides, session-local scheduling state, UI controls, trace/export
redaction, and ACP/mock-agent coverage.

Out of scope: reverse-engineering another harness's private system prompt,
agent-specific integrations, sending workspace file contents automatically,
or making ACP's resource capability imply native context support.

## Cross-cutting risks

- **Restart/rebind:** an in-memory ledger is lost; force a compact snapshot.
- **Multiple paired clients:** current open-tabs reports are last-writer-wins.
  Decide whether context belongs to the active prompting device, a union, or a
  named editor source before treating a delta as authoritative.
- **Session/profile changes:** a profile config option must be preferred over
  injected instructions; fallback instructions need a content revision.
- **Agent compaction:** deltas reduce cost but an agent can forget earlier
  turns. A manual "refresh context" action must be available.
- **Privacy:** all automatic path/resource input remains workspace-contained,
  relative where possible, bounded before serialization, and visible in the
  prompt trace. No raw stderr, secrets, or unrestricted terminal content.
- **Backwards compatibility:** old prompt events remain immutable. New policy
  fields default safely when absent; old sessions receive a fresh snapshot.

## Verification bar

- Unit tests for policy resolution, validation, source hashing, and all delta
  transitions (unchanged, add/remove, edit, reset, rebind, profile change).
- Mock-agent assertions for exact ACP blocks on prompt 1, prompt 2 unchanged,
  and each changed source. Test resource-capable and text-fallback agents.
- API/config tests for defaults, invalid limits, migration, persistence, and
  paired-device authorization.
- UI tests for policy selection, disabled-source explanation, manual refresh,
  and "Context added" trace accuracy.
- Manual matrix against Cursor and Mistral with the same workspace; record
  observed context only, never infer hidden system messages from model prose.
- Run Rust, frontend, and contract suites; add a targeted security review for
  path containment, cross-device context ownership, secret retention, and DoS.

