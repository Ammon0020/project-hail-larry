# S-CTX-POLICY — Policy contract and config migration

## Outcome

Replace ad-hoc prompt middleware toggles with a versioned, persisted policy
that can resolve global defaults, per-harness overrides, and session overrides
without using ACP capabilities as a proxy for native harness context.

## Work

1. Define `ContextPolicy` in `config.toml` with a schema version and safe
   defaults: `minimal`, bounded root inventory, no automatic raw Git/time/tab
   bodies, selection opt-in, and a manual refresh allowance.
2. Migrate the existing `promptContext` numeric limits into this policy without
   losing valid user settings. Old/missing configuration must load as Minimal.
3. Add a capability-neutral harness override keyed by registered agent ID:
   `minimal`, `native_workspace`, or `custom`. Document that it is an explicit
   user choice, not an autodetected ACP property.
4. Define a narrow session override model: temporary only, reset on close, and
   never silently persisted as a new global policy.
5. Expose read/update APIs with strict size/range validation and paired-device
   authorization. Reject unknown schema versions and dangerous source kinds.
6. Record the ACP facts above in a local responsibility note with links to the
   version of the spec used by this repository.

## Acceptance

- Fresh installs resolve to Minimal with no raw Git/time/repeated root prompt.
- Existing limit-only config preserves its limits under the migrated policy.
- A policy can be selected per harness without changing agent credentials or
  profile files.
- Unknown/malformed policy input fails closed and leaves live/disk policy intact.
- API tests cover loopback, paired-device auth, validation, and atomic save.

## Edge cases

- Config written by a newer daemon: preserve unknown fields or reject the
  unsupported schema explicitly; never reinterpret it as permissive.
- Agent IDs renamed/deleted: retain an orphaned override for recovery, but do
  not apply it to a different agent with a reused display name.
- Profile differs from policy: profile controls agent behavior; policy controls
  what the host contributes. Keep their settings visually and semantically
  separate.

## Verify

`cargo fmt -q`; focused config/API tests; `cargo test -q --all-targets`;
`cargo clippy -q --all-targets -- -D warnings`; frontend lint/build; contract
tests if API wire types change.

