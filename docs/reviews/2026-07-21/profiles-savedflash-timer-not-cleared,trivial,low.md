# savedFlash setTimeout is not cleared on unmount

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/components/ProfilesSettings.tsx`
- **Lines:** 213

## Description

`handleSave` schedules `setTimeout(() => setSavedFlash(false), 2000)` to clear the "Saved" indicator, but the timer handle is discarded and never cleared. If the user switches away from the Profiles settings tab (which unmounts `ProfilesSettings` because `SettingsPanel` only renders the active tab's content — line 763 `{activeTab === 'profiles' && <ProfilesSettings />}`) within 2 seconds of saving, the callback fires `setSavedFlash` on an unmounted component. React 18 no longer warns for this, but it is still a latent setState-after-unmount and a small leak of a pending timer.

## Recommendation

Store the timer in a `useRef<ReturnType<typeof setTimeout> | undefined>(undefined)` and clear it in the effect cleanup / on unmount, e.g.:

```ts
const savedFlashTimer = useRef<ReturnType<typeof setTimeout>>()
// in handleSave:
savedFlashTimer.current = setTimeout(() => setSavedFlash(false), 2000)
// cleanup:
useEffect(() => () => clearTimeout(savedFlashTimer.current), [])
```

Note: the same pattern exists in `SettingsPanel.tsx` lines 192 and 777 (`setTimeout(() => setMcpSaved(false), 2000)` and `setTimeout(() => setCopied(false), 2000)`), so this is consistent with the existing codebase — but it is still worth fixing here and ideally back-filling the older call sites.

## Verification

Line 213 calls `setTimeout(() => setSavedFlash(false), 2000)` with no return value captured. A grep for `savedFlashTimer` / `clearTimeout` in this file returns no matches. `SettingsPanel.tsx` line 763 conditionally renders `<ProfilesSettings />` only when `activeTab === 'profiles'`, so switching tabs unmounts the component.
