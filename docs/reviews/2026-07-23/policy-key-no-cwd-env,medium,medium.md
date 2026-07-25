# allow_always / allow_session keyed on command text only, not cwd/env (latent TOCTOU)

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `src/permissions/manager.rs`
- **Lines:** 71-97 (`PolicyKey`, `policy_key_for`), 359-363 (cache hit)

## Description

For shell/execute tools (`target` empty) the policy cache key is `(session_id, tool_kind, command)` — `command` is the only discriminator (manager.rs:86-97). `cwd` and the env overlay are **not** part of the key. Once a user picks `allow_always` for `ls`, every subsequent `request_permission` for `ls` auto-approves regardless of cwd or env. The prompt the user saw is not bound to the execution context: the agent could first prompt `ls` in the workspace root with a clean env, get `allow_always`, then issue `ls` with `LD_PRELOAD=/workspace/evil.so` and a cwd that is a symlink to `/etc` — same command text, auto-approved, different effect. This is currently latent because `create_terminal` does not check permissions at all (see create-terminal-no-approval finding), but it becomes a live TOCTOU the moment approval is enforced on execution.

## Recommendation

Include `cwd` and a hash of the env overlay (or at least the dangerous-var subset) in `PolicyKey` for execute tools. Better: only offer `allow_always`/`allow_session` for tool kinds where the full execution context is part of the key; for shell, default to `allow_once` only.

## Verification

manager.rs:71-77 — `PolicyKey { session_id, tool_kind, target, command }`, no cwd/env fields. manager.rs:93-95 — `command` is set only when `target.is_empty()`. manager.rs:359-363 — a cache hit returns immediately with no re-validation of context.
