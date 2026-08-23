# Configurable cancel grace period

> **Status:** done | **Difficulty:** small | **Urgency:** medium
> **Source:** chat reliability audit

## Problem

`CANCEL_GRACE_PERIOD` (`src/acp/core/lifecycle/mod.rs:307-320`) is
hardcoded to 10 seconds. Interrupted sessions are force-closed after
10s even if the agent is still processing the interrupt. Some agents
may need longer to clean up.

## Goal

Make the cancel grace period configurable via `config.toml`, with a
sensible default.

## Behavior

1. Add `cancel_grace_period_secs` to the ACP config section
   (default 10s).
2. `lifecycle/mod.rs` reads the configured value instead of the
   hardcoded constant.
3. The constant becomes the fallback default if unset.

## Dependencies

- None

## Acceptance

- [x] `cancel_grace_period_secs` in `config.toml`
- [x] Lifecycle reads configured value
- [x] Default remains 10s if unset
- [x] Tests: custom grace period respected
- [x] `make check` passes
