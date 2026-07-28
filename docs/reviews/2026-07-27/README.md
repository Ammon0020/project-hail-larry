# Review 2026-07-27

Comprehensive review of the full codebase, prioritizing user-facing issues.
17 findings fixed across 4 commits (see git log); 73 remain open.

## Fixed (17)

| Finding | Difficulty | Urgency | Commit |
|---|---|---|---|
| chatcomposer-enter-sends-during-ime-composition | small | high | be15582 |
| chathistory-rename-input-refocuses-every-render | small | high | be15582 |
| chathistory-escape-closes-popout-when-canceling-rename | small | high | be15582 |
| toolexecutionblock-controlled-open-attribute | small | high | be15582 |
| errors-is-session-not-found-too-broad | small | high | be15582 |
| tab-close-discards-unsaved | small | high | a876e5a |
| file-open-silent-error | small | high | a876e5a |
| filetree-delete-no-confirmation | small | high | a876e5a |
| fileviewer-silent-token-failure-infinite-spinner | small | high | a876e5a |
| editorpane-reload-no-confirm-and-banner-a11y | small | high | a876e5a |
| gitstate-loading-flash | small | high | 979f52e |
| first-commit-blocked | medium | high | 979f52e |
| diff-split-toggle-mobile-confusing | small | high | 979f52e |
| lockscreen-passcode-normalization-mismatch | small | high | 979f52e |
| lockscreen-no-lockout-recovery-ux | medium | high | 979f52e |
| settingspanel-add-agent-form-silent-failures | small | high | 979f52e |
| settingspanel-mcp-toggle-discards-unsaved-edits | medium | high | 979f52e |
| refresh-button-no-feedback | small | med | 979f52e |
| searchpanel-results-flicker-on-keystroke | small | high | 2525ad8 |
| select-viewport-height-forced-to-trigger | small | high | 2525ad8 |
| usemcpservers-silent-errors-and-no-initial-health-load | medium | high | 2525ad8 |
| useautoscroll-setstate-on-every-scroll-event | medium | med | 2525ad8 |

## Open (73)

Grouped by area. Files are named `<slug>,<difficulty>,<urgency>.md`.

### High urgency (still open)

- `dialog-close-target-and-overflow,medium,high.md` — mobile dialog close target ~16px, no max-h/scroll
- `editorpane-shared-extensions-all-tabs,medium,high.md` — all CodeMirror instances share active tab's extensions
- `filetree-no-keyboard-nav-aria,medium,high.md` — no role/treeitem/tabIndex/arrow keys
- `no-error-boundary-blank-screen-on-crash,medium,high.md` — no error boundary anywhere
- `preview-trust-broken-deny-still-renders,medium,high.md` — trust prompt is a no-op
- `status-bar-hardcoded-values,small,high.md` — status bar lies (0 errors, Ln 1 Col 1, etc.)
- `tabbar-keyboard-accessibility,medium,high.md` — tabs are divs with onClick, no keyboard
- `usebackend-event-list-cap-and-pagination,medium,high.md` — 1000-event cap, shows oldest not newest
- `usepanelresize-no-touch-and-hidden-state-not-persisted,medium,high.md` — resize dead on mobile

### Medium urgency (still open)

See filenames in this directory. Notable clusters:
- **Performance**: `chatpanel-conversationview-unmemoized-derivations`, `usebackend-commit-events-o-n-per-ws-event`, `usechattabs-render-phase-setstate-derived-state`
- **Mobile**: `mobilenav-badges-always-on`, `mobilenav-safe-area-inset`, `left-sidebar-mobile-overlay`, `settingspanel-mobile-nav-no-escape-hardcoded-offset`, `popover-fixed-width-and-no-scroll`, `dropdown-subcontent-overflow-and-item-targets`
- **Theme**: `chatmessageitem-prose-invert-breaks-light-theme`, `diff-viewer-hardcoded-dark-theme`, `editorpane-hardcoded-dark-theme`
- **Error states**: `reconnecting-banner-no-failure-state`, `diff-tab-error-retry-and-fetch-race`, `browsepreview-reloads-on-any-write-flicker`, `fileviewer-pdf-sandbox-breaks-firefox`, `permissioncard-reject-variant-normalization-incomplete`, `switchagentdialog-no-busy-state-on-confirm`
- **Silent failures**: `settingspanel-prompt-context-inputs-silent-reject`, `profilessettings-disabled-save-no-reason`, `api-no-fetch-timeout`
- **Races**: `file-open-race-condition`, `search-result-line-clear-race`, `usefilechangedetection-readfile-overwrites-tab-without-revision-guard`, `usekeyboardshortcuts-re-subscribes-when-handler-identity-changes`
- **A11y**: `banner-no-aria-live-role`, `command-palette-a11y`, `permission-card-and-error-banner-no-aria-live`, `chattabbar-close-tab-not-keyboard-accessible`

### Low urgency (still open)

Minor a11y, dead code, style token, and maintainability items. See filenames.
