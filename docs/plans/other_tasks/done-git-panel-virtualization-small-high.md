# Git panel: virtualize file lists with @tanstack/react-virtual

> Difficulty: small. Urgency: high.
> Source: `--untracked-files=all` regression janks the editor with thousands of
> untracked rows (2026-07-28).

## Goal

Stop rendering every git file row with `files.map()`. Use
`@tanstack/react-virtual` so the panel only renders the visible rows, keeping
the editor smooth when `target/` or `node_modules/` expands into thousands of
untracked entries.

## Scope

### In scope

1. **Dependency** — add `@tanstack/react-virtual` (`^3.13.0`, an older stable
   release — do NOT pull the very latest 3.14.9 from 2026-07-28) to
   `web/package.json`; run `npm install` in `web/` to update the lockfile.

2. **`web/src/components/git/GitPanel.tsx`** — replace `ChangeSection`'s
   `files.map(...)` with `useVirtualizer`:
   - One scroll parent (the existing `flex-1 overflow-y-auto` wrapper) holding
     both `ChangeSection`s; a single virtualizer over the combined
     staged+unstaged list keeps the single-scroll UX and avoids two competing
     scroll containers. Section headers render as non-virtual rows above their
     slice of the list.
   - `estimateSize` ~28px (row height: `py-1.5` + `text-xs`).
   - Render only `virtualItems` inside a `position: relative; height: totalSize`
     container; each row absolutely positioned via `translateY(vi.start)`.
   - Brief comment explaining the single-virtualizer-over-combined-list choice.

### Out of scope

- Perfect windowing for 100k+ rows — the goal is "1000 rows doesn't jank".
- Sticky section headers (headers can scroll with the list).
- Custom row-measurement (`estimateSize` is sufficient).

## Acceptance criteria

- [ ] `@tanstack/react-virtual` is in `web/package.json` at `^3.13.0` (or older
      stable) and the lockfile is updated.
- [ ] `ChangeSection` no longer calls `files.map(...)` for full render; only
      visible virtual items are mounted.
- [ ] Section headers (Staged Changes / Changes) still render above their rows.
- [ ] Single scroll parent preserved (panel-level `overflow-y-auto`).
- [ ] Row height ~28px; layout matches the pre-virtualization appearance.
- [ ] `make check` passes (frontend eslint + build included).

## Verification

1. `make qcheck` — autofix fmt/lints + quiet tests.
2. `make check` — full gate (frontend eslint/build).
3. Manual: open a workspace with a large untracked dir; confirm the panel
   scrolls smoothly and only visible rows are in the DOM (DevTools elements).

## File references

- `web/package.json`, `web/src/components/git/GitPanel.tsx`

## Depends on

None. Part 1+3 (gitignore endpoint + context menu) is independent and tracked
separately.
