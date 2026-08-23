# Model selector menu with thinking levels and pin

> **Status:** done | **Difficulty:** medium | **Urgency:** high
> **Source:** user-noted improvements — Chat section

## Goal

Replace the model dropdown selector with a menu that supports search, thinking
level selection, fast mode, and pin (replacing favorite). Instead of listing
models as "gpt xxx high, gpt xxx medium" etc, group by base model and let the
user select thinking level on models that support it.

## Behavior

Each model entry has at most:
- **Pin** (replaces favorite) — pinned models appear at top
- **Thinking** — high, medium, low, none, or any other thinking level the model supports
- **Fast** — for models that support a fast variant
- **Statistics** (token usage, cost, etc.) — optional, plan ahead for future updates

Also: support ACP's `model_config` options — agents can advertise configurable
model traits. Support any of these we can.

## Dependencies

- Backend already returns model lists via autodetect with `id` and `name` fields
- The current favorite system needs to be migrated to pin

## Acceptance

- [x] Model selector is a menu (not a dropdown) with search
- [x] Models grouped by base name with thinking level sub-options
- [x] Pin replaces favorite; pinned models at top
- [x] Fast variant toggle for models that support it
- [ ] ACP model_config traits supported where possible *(deferred — backend AgentModel has no structured traits yet; parseModelId is the fallback)*
- [x] `make check` passes
