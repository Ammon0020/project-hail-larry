- name: searchResultLine auto-clears via setTimeout(0) before a slow file load can consume it
- file: /media/adam/extex/projects/project-hail-larry/web/src/App.tsx
- lines: 370-379, 839-847
- description: |
    `handleSearchResultSelect` (lines 839-847) sets `searchResultLine` after
    `await handleFileSelect(path)`. The clear effect at lines 375-379 wipes
    `searchResultLine` on a `setTimeout(0)` whenever it is set. The comment
    says this "defers the clear to the next macrotask, which runs after the
    EditorPane effect that performs the scroll." That ordering assumption only
    holds if the tab's content is already loaded synchronously — i.e. the file
    was already open. On the cold path (file not yet open), `handleFileSelect`
    awaits `readFile`, so by the time `setSearchResultLine(lineNumber)` runs,
    several macrotasks have already elapsed and the EditorPane may not have
    even received the new tab content yet. The `setTimeout(0)` clear can fire
    *before* EditorPane's scroll effect runs, leaving the editor at the top of
    the file instead of the matched line. The user clicks a search hit, the
    file opens, and the cursor stays at line 1 — the jump is silently lost.
    This is an intermittent perceived-perf / correctness bug that depends on
    file-load latency. Fix: have EditorPane report back when it has applied
    the scroll (or clear `searchResultLine` from inside EditorPane's effect
    after the scroll succeeds), rather than racing a zero-delay timer against
    an async file load.
- verification: |
    Read App.tsx lines 375-379: the clear is `setTimeout(() =>
    setSearchResultLine(null), 0)` keyed only on `searchResultLine`. Read
    lines 839-847: `handleSearchResultSelect` sets the line *after* an
    `await handleFileSelect(path)` that may take arbitrary time. The timer
    fires relative to when the line was set, not relative to when EditorPane
    consumed it.
