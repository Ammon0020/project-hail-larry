# Switching to 'system' theme at runtime does not subscribe to OS preference changes

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `web/src/hooks/useTheme.ts`
- **Lines:** 12-15

## Description

useTheme.setTheme calls applyAndStore(next), which calls applyTheme('system') — resolving the OS preference ONCE at click time and toggling the .dark class. But no matchMedia 'change' listener is registered. The only listener is added in initTheme() (theme.ts lines 50-60), and only when the INITIAL stored theme on app startup is already 'system'. So a user who picks 'system' from MobileSettings at runtime gets a one-shot resolution: if they later toggle their OS dark/light setting, the app theme will not follow. This defeats the purpose of the 'system' option and is a regression vs. the documented behavior ('keeps it in sync with OS changes', theme.ts line 47).

## Recommendation

Have useTheme register a matchMedia change listener (in a useEffect) whenever the current theme === 'system', calling applyTheme('system') on change, and cleaning up on unmount or when the preference changes away from 'system'. Alternatively, move the listener registration into setTheme/applyTheme so it is always present for 'system'.

## Verification

Read useTheme.ts (no useEffect, no matchMedia). Read theme.ts lines 50-60 confirming the listener is only attached inside initTheme and only when `stored === 'system'`. grep for addEventListener('change' across web/src returns only theme.ts line 58 — no other subscription path exists.
