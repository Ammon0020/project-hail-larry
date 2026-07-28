# Crate Extraction & Utilities

## Scope
Finalize the extraction by moving the fully decoupled `acp` module into its own physical Cargo crate, and handling remaining utility dependencies (`procutil`, `mcp`).

## Acceptance Criteria
- Move `configure_process_group` logic from `src/procutil` into the ACP crate (as it requires only std/libc and has no daemon dependencies).
- Embed or feature-gate the `mcp` loader within the ACP crate.
- Create `crates/acp-core` Cargo package and move the modules.
- Update `local_agent`'s `Cargo.toml` to depend on `acp-core`.
- Ensure `cargo check` and `cargo test` pass cleanly.
