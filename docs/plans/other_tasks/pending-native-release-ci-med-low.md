# Task: Native release CI for Windows and macOS

> **Status:** pending | **Difficulty:** med | **Urgency:** low
> **Origin:** S-BUILD audit (2026-07-18). Epic: rust-port.

## Problem

`docs/plans/rust-port/active-S-BUILD-build-release-med.md` ACs 4 and 5 are
unchecked: native Windows and macOS release-artifact builds + SPA smoke tests
are not in CI. The existing `.github/workflows/rust-ci.yml` runs only debug
`cargo build --locked` with a stub `web/dist/index.html` on those runners — no
release artifact, no SPA smoke test.

## Scope

- Add a release CI job (or extend `rust-ci.yml`) that runs on native Windows
  and macOS runners:
  - `npm install && npm run build` in `web/` (real frontend, not stub)
  - `cargo build --release` → produce a release artifact
  - SPA smoke: start `local_agent`, probe `http://localhost:7337/health` and
    `http://localhost:7337/` (confirm `index.html` is served), then stop
- Optional: dependency-age/security review step (`cargo audit` or similar)
- Linux release CI is implicitly covered by `build.sh`; add it explicitly if
  not already present

## Acceptance criteria

- [ ] Windows runner produces a `local_agent.exe` release artifact
- [ ] macOS runner produces a `local_agent` release artifact
- [ ] SPA smoke test passes on both (health check + index.html served)
- [ ] CI runs on push/merge to main (or release tag)

## Out of scope

- Code signing / notarization (separate future task)
- Universal binary for macOS (separate future task)
- Auto-release to GitHub Releases (separate future task)
