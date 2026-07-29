- name: ProfilesSettings delete-default error banner is far from the delete button
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/ProfilesSettings.tsx
- lines: 208-222, 540-552, 427-432
- description: `handleDelete` (line 210-213) sets `setError('Cannot delete the default
  profile. Pick another default first.')` and returns. The error renders in a banner at
  the BOTTOM of the entire settings card (lines 427-432), while the Delete button is in
  the `ProfileEditor` header (line 540-552) at the TOP-right of the editor pane. On a
  tall viewport or mobile (where the list/editor stack and the banner is below the fold),
  the user clicks Delete, nothing visibly changes in the editor, and the explanation is
  off-screen. The Delete button does have a `title` tooltip (line 543-547: "Cannot delete
  the default profile — pick another default first.") and is `disabled` when
  `!canDelete` (line 542), so the click is actually a no-op — wait, re-reading: the button
  IS disabled (`disabled={!canDelete}`, line 542), so `handleDelete` for the default is
  only reachable from the list-level delete (there is none) — actually `canDelete` is
  `draft.defaultProfileId !== selectedId` (line 391), so the button is disabled for the
  default profile and `handleDelete(selectedId)` cannot fire from it. So the
  `setError` branch in `handleDelete` (line 210-213) is effectively dead code for the
  editor's Delete button. It's still reachable if `handleDelete` is called with the
  default id from elsewhere — but there's no other caller. So: (a) the dead defensive
  branch is fine, but (b) the real UX issue is that the disabled Delete button's only
  explanation is a hover `title` tooltip, which does NOT appear on touch/mobile. A mobile
  user sees a greyed-out Delete button with no explanation. Fix: show an inline muted hint
  under the button ("Set another profile as default to delete this one") instead of
  relying on a `title` tooltip.
- verification: Read `ProfilesSettings.tsx:540-552` (Delete button disabled when
  !canDelete, only a `title` tooltip explains why), `391` (canDelete = not default),
  `208-222` (handleDelete's default-guard sets a far-away banner error — dead via this
  button). Confirmed no inline hint next to the disabled button.
