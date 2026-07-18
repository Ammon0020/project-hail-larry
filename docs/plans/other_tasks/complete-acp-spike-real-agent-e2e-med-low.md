# Task: ACP spike — real-agent E2E prompt round trip

> **Status:** complete (2026-07-18) | **Difficulty:** med | **Urgency:** low
> **Origin:** S-ACP-SPIKE audit (2026-07-18). Epic: rust-port.

## Problem

S-ACP-SPIKE had one unchecked AC: real-agent opt-in E2E prompt round trip.
Mock-agent coverage was complete; live agents were not exercised in CI.

## Solution

- Added `spike_real_agent_prompt_round_trip` in `tests/spike_acp.rs`
- Gated by `ACP_E2E_AGENT` (`codex`/`cursor`/`vibe`/`claude`/`gemini`/cmd)
- Skips when unset — CI needs no keys or adapters
- Uses temp cwd only (no `~/.local-agent` writes)

## Acceptance criteria

- [x] At least one real agent completes a full prompt round trip
  — Verified with `ACP_E2E_AGENT=codex` (`codex-acp`): EndTurn + streamed text
- [x] Test is opt-in (env var gated), does not break CI without keys
- [x] Results recorded in `docs/STATUS.md`

## Out of scope (unchanged)

- Testing all known agents (one is sufficient for spike closure)
- Performance benchmarking
- Mid-prompt cancel against a live agent (mock cancel + connection teardown
  cover client ownership; optional post-cutover polish)
