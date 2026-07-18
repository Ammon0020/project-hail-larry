# Task: ACP spike — real-agent E2E prompt round trip

> **Status:** pending | **Difficulty:** med | **Urgency:** low
> **Origin:** S-ACP-SPIKE audit (2026-07-18). Epic: rust-port.

## Problem

`docs/plans/rust-port/active-S-ACP-SPIKE-sdk-proof-med.md` has one genuinely
unchecked AC: "Configured real agent completes opt-in E2E prompt round trip."
The spike verified all SDK APIs against a mock agent, but no real agent
(Claude Code, Codex, Gemini CLI) was configured for a live end-to-end test.

## Scope

- Configure one real ACP agent (Claude Code or Codex CLI) with API keys
- Run an opt-in E2E test: initialize → create session → prompt → stream → cancel → close
- Verify file/shell callbacks fire and permission prompts deliver
- Gate behind an env var (e.g. `ACP_E2E_AGENT=claude`) so CI doesn't require keys

## Acceptance criteria

- [ ] At least one real agent completes a full prompt round trip
- [ ] Test is opt-in (env var gated), does not break CI without keys
- [ ] Results recorded in `docs/STATUS.md`

## Out of scope

- Testing all known agents (one is sufficient for spike closure)
- Performance benchmarking
