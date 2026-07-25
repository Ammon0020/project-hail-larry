# S-CTX-SOURCES — Project-aware sources and Git summary

## Outcome

Make initial workspace context useful for code projects and quiet for generic
folders such as Downloads. Replace raw command output with bounded structured
signals and source-specific invalidation.

## Work

1. Define a root inventory of immediate entries only, with relative name and
   kind. Apply the policy cap before rendering. Do not recursively traverse or
   derive file contents.
2. Detect a meaningful repository cheaply: Git available, root is a worktree,
   and at least one commit. If absent, empty, or only untracked, suppress Git.
3. When Git is enabled, render branch/head, ahead/behind, tracked changed
   counts, and a bounded sample of relative tracked paths. Never inject raw
   `git status --short -b` output or untracked Downloads inventory by default.
4. Hash the rendered bounded source, not a broad filesystem walk. Invalidate
   it only on an explicit refresh, workspace switch/rebind, or a bounded Git
   probe policy.
5. Keep `AGENTS.md` distinct from inventory. Cap and disclose it, refresh only
   when changed, and omit it when binary/unreadable/outside workspace.

## Acceptance

- A Downloads-like uncommitted folder sends no Git blob and no recursive tree.
- A normal repository sends a compact, stable summary only if policy enables it.
- All displayed paths are relative and workspace-contained; truncation is
  explicit in the trace.

## Future improvements

- Optional project manifest summary (`Cargo.toml`, package manifest) after a
  separate privacy/token review.
- File-watcher-driven source invalidation instead of prompt-time probes.
- A user-invoked "attach project summary" command for one richer turn.

