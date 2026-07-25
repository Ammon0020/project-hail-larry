# Remote workspace registration accepts arbitrary absolute paths (sandbox escape when flag enabled)

- **Difficulty:** trivial
- **Urgency:** critical
- **File:** `src/api/mod.rs`
- **Lines:** 448-503 (handler), `src/workspace/mod.rs:155-178` (register)

## Description

When `allow_remote_workspace_registration: true` is set in config, any paired device can `POST /api/workspaces` with `{"path": "/etc"}`, `{"path": "/home"}`, `{"path": "~/.ssh"}`, or any other absolute directory. `register` (workspace/mod.rs:155-178) only checks `metadata.is_dir()` and canonicalizes — there is **no** constraint that the path be under the user's home or an allow-listed root. The path then becomes a fully accessible workspace: `read_file`, `raw_file`, `search`, `file_tree`, `write_file`, `delete_path`, `rename_path`, `mkdir` all operate on it. This is a complete sandbox escape: a paired device (which AGENTS.md treats as a semi-trusted peer, not a host admin) can read `/etc/shadow`-style secrets, `~/.ssh/id_rsa`, `~/.gnupg/`, and write anywhere the daemon's user can write. The grace-period path (api/mod.rs:489-502) just defers the same unconstrained `register_workspace(&path)` call (pairing/mod.rs:490-496 → daemon.rs:357-378). `config::add_workspace` (config/model.rs:199-209) likewise only checks non-empty and dedup — no path validation.

## Recommendation

Introduce an allow-list of root prefixes (default: the user's home directory and explicit `extra_workspace_roots` in config). In `register`, after canonicalization, verify `root.starts_with(allowed_root)` for some allowed root and reject otherwise with 403. Apply the same check in `config::add_workspace` and in the pairing registrar path. Document that `allow_remote_workspace_registration` is host-admin-only and that even then paths are confined.

## Verification

api/mod.rs:460-464 gates only on the boolean flag; lines 468-486 call `state.workspaces.register(&payload.path)` with the raw user-supplied string. workspace/mod.rs:155-178 performs no allow-root check. Grep across `src/` for `allowed_root|is_within_home|must_be_under` returns no workspace-registration hits. config/model.rs:199-209 (`add_workspace`) only checks emptiness/dedup.
