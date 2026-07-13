# DefaultSystemMessages() Nil-Guard Pattern Repeated 15+ Times

## Location
- [providers.go](file:///media/adam/extex/projects/project-hail-larry/internal/acp/providers.go) — 6 occurrences
- [context.go](file:///media/adam/extex/projects/project-hail-larry/internal/acp/context.go) — 3 occurrences
- [conversation.go](file:///media/adam/extex/projects/project-hail-larry/internal/acp/conversation.go) — 3 occurrences
- [messages.go](file:///media/adam/extex/projects/project-hail-larry/internal/acp/messages.go) — 3 occurrences

## Problem

Every middleware and function that uses `*SystemMessages` follows the same boilerplate pattern:

```go
// Constructor
func NewFooMiddleware(messages *SystemMessages) *FooMiddleware {
    if messages == nil {
        messages = DefaultSystemMessages()
    }
    return &FooMiddleware{Messages: messages}
}

// Method
func (m *FooMiddleware) BeforePrompt(...) {
    sm := m.Messages
    if sm == nil {
        sm = DefaultSystemMessages()
    }
    // use sm
}
```

The nil-check is performed **twice** — once in the constructor AND once in the method body — for every middleware (`TimeMiddleware`, `OpenFilesMiddleware`, `RecentEditsMiddleware`, `OpenFilesResourceMiddleware`, `FirstPromptContextMiddleware`, `ConversationTransferMiddleware`). The constructor nil-check means `Messages` should never be nil at method-call time, making the method-level check dead code.

## Impact

- 15+ identical nil-guard blocks across the `acp` package.
- The duplicated method-level guard obscures whether `Messages` can actually be nil after construction (it can't).

## Suggested Fix

Option A: Trust the constructor — remove all method-level nil guards. The constructor already ensures `Messages != nil`.

Option B: If defensive coding is preferred, extract a helper:

```go
func (m *FooMiddleware) messages() *SystemMessages {
    if m.Messages != nil { return m.Messages }
    return DefaultSystemMessages()
}
```

`FirstPromptContextMiddleware` already has this pattern (`messages()` helper) — the others should follow suit and then the constructor guard becomes the single point of defense.

## Resolution (2026-07-12) — WONTFIX
The codebase has already been refactored to extract per-middleware `ensureMessages()` helpers (the review's recommended Option B), consolidating the nil-guard logic from 15+ duplicated blocks into 6 single helpers; the remaining method-body helper calls are a live defensive accessor pattern (the helper's nil-branch is used by constructors, skipped post-construction), not duplicated dead guards, and removing them would reduce defensiveness for marginal benefit.
