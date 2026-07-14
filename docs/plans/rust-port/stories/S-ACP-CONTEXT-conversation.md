# Story S-ACP-CONTEXT: Context, Conversation, Terminal, and Profiles

> **Phase:** 3 | **Depends on:** S-ACP-CORE, S-EVENTS, S-CONFIG | **Go source:** `internal/acp/context.go`, `conversation.go`, `store.go`, `terminal.go`, `profile.go`

## Goal

Port user-visible session context, history/export/rebind, terminal sessions, and
agent modes as focused components around the ACP core.

## Design

Prompt middleware returns typed ACP content blocks. Resource/image blocks are
preserved when negotiated; fallback behavior remains wire-compatible for agents
without structured-context support. Existing conversation state is preserved by
S-MIGRATE, not redesigned in this story.

## Acceptance Criteria

- [ ] Open-file, recent-edit, profile, and conversation-transfer context match Go behavior
- [ ] Structured content blocks and text fallback both pass capability tests
- [ ] Conversation list, rename, export, delete, and rebind match contract fixtures
- [ ] Terminal lifecycle, output bounds, and cleanup are tested
- [ ] Existing session metadata remains readable
