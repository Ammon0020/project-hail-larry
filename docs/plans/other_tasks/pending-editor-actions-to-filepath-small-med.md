# Move editor action buttons to filepath line

> **Status:** pending | **Difficulty:** small | **Urgency:** medium
> **Source:** user-noted improvements — UI section

## Goal

Move the preview, line wrap, and save buttons from the tabs bar at the top of
the main canvas to the right side of the filepath line below the editor (the
breadcrumb/filepath bar). This keeps them visible without taking space from
tabs, especially on mobile.

## Details

- Current location: `TabBar.tsx` editor actions section (lines 312-366)
- Target location: the filepath/breadcrumb bar below the tabs
- The buttons should be right-aligned on that line
- On mobile, they should remain accessible without crowding the filepath

## Acceptance

- [ ] Preview, wrap, and save buttons moved from TabBar to filepath line
- [ ] Buttons remain visible and accessible on desktop and mobile
- [ ] Tab bar has more space for tabs
- [ ] `make check` passes
