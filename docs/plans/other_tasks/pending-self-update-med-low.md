# Self-update for `local_agent`

> Story. Difficulty: med. Urgency: low.

## Goal

Let the user update `local_agent` to a newer release without manually
downloading, replacing the binary, and restarting the service. **We need to
decide the best user flow for it.**

## Scope

- Decide the best user flow: `local_agent update` CLI subcommand vs. in-app
  "Update available" prompt vs. both. CLI is scriptable and works headless;
  in-app is friendlier but the daemon can't replace its own running binary
  safely.
- Check the latest release tag via GitHub Releases API
  (`GET /repos/{owner}/{repo}/releases/latest`) and compare against the
  embedded `env!("CARGO_PKG_VERSION")`.
- Download the matching platform binary + `.sha256` sidecar, verify the
  checksum, swap atomically (write to temp, rename), and restart the service
  (systemd/launchd/HKCU).
- The CLI (a separate process) performs the swap while the daemon is stopped,
  then restarts it — the daemon cannot replace its own running binary.
- No auto-update (silent background). The user owns the host and must opt into
  a specific version.
- Out of scope: code signing verification, channel/stable-vs-prerelease
  selection, rollback. Revisit at v1.0.

## Verification

- Manual: `local_agent update` on an older version fetches, verifies, swaps,
  and restarts; the daemon comes back reporting the new version.
- Unit test for the version-comparison helper.

Suggested commit: `feat(cli): add self-update story plan`
