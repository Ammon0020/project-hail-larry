# Review 2026-07-27

Comprehensive review of the full codebase, prioritizing user-facing issues.
**All high-urgency findings resolved.** 62 fixed across 12 commits; 28 remain open (21 med, 7 low — no high left).

## Fixed (62)

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
| status-bar-hardcoded-values | small | high | e273d74 |
| dialog-close-target-and-overflow | medium | high | e273d74 |
| no-error-boundary-blank-screen-on-crash | medium | high | e273d74 |
| preview-trust-broken-deny-still-renders | medium | high | e273d74 |
| tabbar-keyboard-accessibility | medium | high | 867ba01 |
| usepanelresize-no-touch-and-hidden-state-not-persisted | medium | high | 867ba01 |
| filetree-no-keyboard-nav-aria | medium | high | 867ba01 |
| editorpane-shared-extensions-all-tabs | medium | high | 867ba01 |
| usebackend-event-list-cap-and-pagination | medium | high | 6a65919 |
| chatmessageitem-prose-invert-breaks-light-theme | small | med | 6a65919 |
| editorpane-hardcoded-dark-theme | small | med | 6a65919 |
| diff-viewer-hardcoded-dark-theme | medium | med | 6a65919 |
| mcppopout-hardcoded-colors-not-semantic-tokens | small | low | 6a65919 |
| banner-no-aria-live-role | small | med | 6a65919 |
| permission-card-and-error-banner-no-aria-live | small | med | 6a65919 |
| reconnecting-banner-no-failure-state | small | med | 6a65919 |
| api-error-type-not-exported (partial: export added) | small | high | 979f52e |
| usebackend-commit-events-o-n-per-ws-event | medium | med | 9fb348d |
| chatpanel-conversationview-unmemoized-derivations | medium | med | 9fb348d |
| usekeyboardshortcuts-re-subscribes-when-handler-identity-changes | small | med | 9fb348d |
| file-open-race-condition | medium | med | 9fb348d |
| browsepreview-reloads-on-any-write-flicker | medium | med | 9fb348d |
| mobilenav-safe-area-inset | small | med | 9fb348d |
| popover-fixed-width-and-no-scroll | small | med | 9fb348d |
| dropdown-subcontent-overflow-and-item-targets | small | med | 9fb348d |
| chatpanel-handlesend-error-restore-overwrites-new-input | small | med | e694fa4 |
| chatpanel-pendingcreatedsessionids-never-drains | small | med | e694fa4 |
| chatmessageitem-shelloutputstreamed-dropped | medium | med | e694fa4 |
| commit-ctrl-enter-missing | small | med | e694fa4 |
| diff-tab-error-retry-and-fetch-race | small | med | e694fa4 |
| fileviewer-pdf-sandbox-breaks-firefox | small | med | e694fa4 |
| fileviewer-modelviewer-no-resize | small | med | e694fa4 |
| tab-close-activates-last-not-neighbor | small | med | e65f9a0 |
| permissioncard-reject-variant-normalization-incomplete | small | med | e65f9a0 |
| switchagentdialog-no-busy-state-on-confirm | small | med | e65f9a0 |
| switchagentdialog-chars-vs-bytes-mismatch | small | med | e65f9a0 |
| searchpanel-truncation-and-hidden-matches | small | med | e65f9a0 |
| fileviewer-duplicate-image-svg-viewer | small | low | 8f95315 |
| fileicon-d-ts-dead-code | small | low | 8f95315 |
| types-duplicate-file-node | medium | low | 8f95315 |
| usetheme-duplicate-matchmedia-listener | small | low | 8f95315 |
| theme-localstorage-and-listener-leak | small | med | 8f95315 |
| usebackend-reportcontext-timer-not-cleared-on-unmount | small | low | 8f95315 |
| usefilechangedetection-readfile-overwrites-tab-without-revision-guard | medium | med | 8f95315 |
| settingspanel-megafile-split-sections | medium | low | 4e5baa8 |

## Open (28)

All remaining findings are med or low urgency. No high-urgency items remain.

### Medium urgency (21)

Grouped by cluster. Files are named `<slug>,<difficulty>,<urgency>.md`.

**Performance**
- `usechattabs-render-phase-setstate-derived-state,medium,med.md`

**Mobile/touch**
- `mobilenav-badges-always-on,small,med.md`
- `left-sidebar-mobile-overlay,medium,med.md`
- `settingspanel-mobile-nav-no-escape-hardcoded-offset,small,med.md`

**Error states / silent failures**
- `fileviewer-csv-no-virtualization-large-files,medium,med.md`
- `settingspanel-prompt-context-inputs-silent-reject,small,med.md`
- `profilessettings-disabled-save-no-reason,small,med.md`
- `profilessettings-delete-default-no-mobile-hint,small,med.md`
- `api-no-fetch-timeout,medium,med.md`

**Races**
- `search-result-line-clear-race,small,med.md`

**A11y**
- `command-palette-a11y,small,med.md`
- `chattabbar-close-tab-not-keyboard-accessible,small,med.md`

**Other**
- `chatcomposer-placeholder-references-nonexistent-at-mention,small,med.md`
- `filetree-active-file-not-revealed,small,med.md`
- `filetree-mobile-rename-longpress-bugs,small,med.md`
- `language-dotfiles-no-highlighting,small,med.md`
- `native-browser-dialogs,large,med.md`
- `settingspanel-nav-highlight-feedback-loop,small,med.md`
- `sw-autoupdate-stale-running-tab,small,med.md`
- `usebackend-loadsessions-called-inside-setstate-updater,small,med.md`
- `uselocalstorage-no-cross-tab-storage-event-sync,medium,med.md`

### Low urgency (7)

Minor a11y, dead code, style token, and maintainability items.
- `api-content-type-on-get,small,low.md`
- `breadcrumb-bar-a11y,small,low.md`
- `button-touch-targets-below-44px,small,low.md`
- `git-panel-error-banner-a11y,small,low.md`
- `header-activity-bar-magic-width,small,low.md`
- `profilessettings-legacy-tools-no-clear-action,small,low.md`
- `workspace-header-status-button-a11y,small,low.md`
