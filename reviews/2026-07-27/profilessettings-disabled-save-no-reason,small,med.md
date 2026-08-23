- name: ProfilesSettings disabled-Save gives no reason for invalid draft state
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/ProfilesSettings.tsx
- lines: 156-184, 434-442
- description: `inlineError` (lines 164-184) is computed via `useMemo` and covers: zero
  profiles, bad id pattern, label/instructions too long, and `defaultProfileId` not
  matching any profile. The Save button is `disabled={saving || !dirty || !!inlineError}`
  (line 437). But `inlineError` is only surfaced to the user inside `handleSave` (line 242:
  `setError(inlineError)`) — which is unreachable when the button is disabled. So a user
  who, e.g., deletes all profiles (possible only by deleting non-defaults one by one, but
  the last one can't be deleted because it's the default — actually the empty-profiles
  branch is reachable if the backend ever returns an empty config) or enters a label >100
  chars sees the per-field red hint (lines 587-591, 624-628) for the length cases, but the
  "defaultProfileId does not match" and "At least one profile is required" cases show ONLY
  a disabled Save button with no explanation. The error banner (lines 427-432) is empty
  because `error` is null until a save attempt or a delete-default action. Fix: render
  `inlineError` as a muted/disabled-Save hint near the Save button whenever it's non-null,
  so the user understands why Save is disabled.
- verification: Read `ProfilesSettings.tsx:164-184` (inlineError useMemo), `437` (Save
  disabled by inlineError), `240-245` (handleSave sets error from inlineError — dead path
  when disabled), `427-432` (error banner only shows `error`, not `inlineError`).
