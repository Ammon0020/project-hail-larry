# ACP model traits — structured agent-advertised model configuration

> **Status:** pending | **Difficulty:** medium | **Urgency:** low
> **Source:** user-noted improvements — Chat section (deferred sub-item)
> **Parent:** `pending-user_noted_improvements-large-high.md`

## Goal

Under ACP's `model_config` options, agents can advertise configurable model
traits (e.g. thinking level, fast mode, temperature, max tokens). Parse and
expose these structured traits so the frontend can render agent-specific model
configuration beyond the hardcoded thinking-level/fast toggles.

## Scope

- **Backend**: Extend `AgentModel` (or add a sibling type) with structured
  traits parsed from ACP's `configOptions` / `model_config` response. Map ACP
  trait names to a typed enum or open-ended key-value structure.
- **API**: Include traits in the model list response so the frontend can render
  them.
- **Frontend**: `ModelSelector.tsx` currently hardcodes thinking-level pills and
  a fast toggle. Generalize to render whatever traits the agent advertises,
  falling back to the current hardcoded behavior when traits are absent.
- **Out of scope**: Statistics (token usage, cost) — tracked separately in the
  context-usage work.

## Dependencies

- ACP `configOptions` probe already works (verified for Devin and Cursor Agent).
- `ModelSelector.tsx` and `modelGrouping.ts` from the model-selector-menu work.

## Acceptance

- Agent-advertised model traits are parsed from the ACP response and typed.
- `ModelSelector` renders agent-specific traits when available.
- Hardcoded thinking-level/fast behavior preserved as fallback.
- `make check` passes.

## Verification

```text
make check
```
