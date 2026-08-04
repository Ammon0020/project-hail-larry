# Interface types split

> Difficulty: medium. Urgency: medium. Status: pending.

## Goal

Split `src/interfaces/types.rs` by wire-domain while preserving serde names,
DTO shapes, and public re-exports.

## Scope

```text
src/interfaces/
├── types.rs       facade/re-exports and serde helpers
├── events.rs      events, payloads, attachments, context
├── workspace.rs   workspace DTOs
├── session.rs     session DTOs
├── permissions.rs permission DTOs
├── search.rs      search DTOs
└── pairing.rs     device/pairing DTOs
```

Move serde attributes verbatim. Coordinate with the pending ACP crate-extraction
work before implementation.

## Acceptance

- Existing imports and public API remain unchanged through re-exports.
- Interface golden tests and contract fixtures remain unchanged.
- No wire-format or TypeScript mirror changes are needed.

## Verification

```text
cargo fmt --all -- --check
cargo clippy -q --all-targets -- -D warnings
cargo test -q --all-targets
make check
```
