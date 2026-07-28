# Abstract Configuration DTOs

## Scope
The `acp` module relies on specific Data Transfer Objects (DTOs) from `src/config` (`AgentInfo`, `AgentModel`, `PromptContextSettings`, `MAX_PROMPT_CONTEXT_PATHS`). To break this dependency, move these structs out of the daemon's config module and into the ACP boundary.

## Acceptance Criteria
- Move the DTOs into the ACP module.
- Re-export the types in `src/config/model.rs` so the rest of the daemon code doesn't break.
- Remove all `use crate::config::*` statements from `src/acp`.
