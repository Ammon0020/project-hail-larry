# Load() does not apply default CredentialInactivityTTLSeconds to existing configs

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/config/config.go`
- **Lines:** 146-152

## Description

`Load()` intentionally does not zero-fill `CredentialInactivityTTLSeconds` from the default. The comment explains the rationale (a plain int cannot distinguish "omitted" from "explicitly 0"). The consequence is that the product decision "sliding expiry ON by default (30 days)" applies only to fresh installs (`DefaultOrError`). Any user with a pre-existing `config.json` that lacks the field gets `0` → expiry disabled, with no log or migration notice. This is a documented tradeoff, but it directly contradicts the "on by default" claim in the field docstring (lines 36-42) and `docs/STATUS.md`, and there is no way for an existing user to discover they are on the less-secure default without reading the config file.

## Recommendation

Either accept the tradeoff and correct the docstring to say "on by default for fresh installs; existing configs keep their prior behavior," or use a `*int`/separate "set" flag to distinguish omitted-from-explicit-0 and apply the 30-day default on load when omitted. At minimum, log a one-time notice when an existing config is loaded with the field absent.

## Verification

Read `Load` lines 143-152 — every other field is zero-filled from `def`, but `CredentialInactivityTTLSeconds` is explicitly skipped. `daemon.New` (lines 164-166) only calls `SetInactivityTTL` when `> 0`, so a loaded `0` stays disabled. `DefaultOrError` (line 89) sets the 30-day default, but `Load` only returns `DefaultOrError` when the config file is entirely absent (lines 104-108).
