# Task: Native release CI for Windows and macOS

> **Status:** complete | **Difficulty:** med | **Urgency:** low
> **Origin:** S-BUILD audit (2026-07-18). Epic: rust-port.
> **Completed:** 2026-07-18

## Problem

`docs/plans/rust-port/complete-S-BUILD-build-release-med.md` ACs 4 and 5 were
unchecked: native Windows and macOS release-artifact builds + SPA smoke tests
were not in CI. The existing `.github/workflows/rust-ci.yml` ran only debug
`cargo build --locked` with a stub `web/dist/index.html` on those runners — no
release artifact, no SPA smoke test.

## What shipped

- `.github/workflows/rust-ci.yml` jobs `release-macos` and `release-windows`
  (same push/PR triggers as the rest of the workflow):
  - Real frontend: `npm ci` + `npm run build` in `web/` (Node 20)
  - `cargo build --release --locked --bin local_agent`
  - Upload artifacts `local_agent-macos` / `local_agent-windows` (14-day retention)
  - SPA smoke via `scripts/spa-smoke.sh` (macOS) and `scripts/spa-smoke.ps1`
    (Windows): isolated `LOCAL_AGENT_STATE_DIR`, free port, TLS off, probe
    `/health` + `/` HTML, then `stop`
- Existing debug `build-macos` / `build-test-windows` / Linux `contract` jobs
  unchanged (still use SPA stub; release jobs cover real embed)

## Acceptance criteria

- [x] Windows runner produces a `local_agent.exe` release artifact
- [x] macOS runner produces a `local_agent` release artifact
- [x] SPA smoke test passes on both (health check + index.html served)
- [x] CI runs on push/merge to main (or release tag)

## Out of scope (still deferred)

- Code signing / notarization
- Universal binary for macOS
- Auto-release to GitHub Releases
- Optional `cargo audit` / dependency-age gate
- Explicit Linux release CI job (`build.sh` remains the Linux path)
