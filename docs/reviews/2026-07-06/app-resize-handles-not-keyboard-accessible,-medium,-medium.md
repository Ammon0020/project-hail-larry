# Panel resize handles are not keyboard accessible

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `web/src/App.tsx`
- **Lines:** 666-672, 688-694

## Description

Both resize handles are plain <div> elements with only onMouseDown handlers and a `title='Drag to resize'` tooltip. They have no tabIndex, no role='separator' (with aria-orientation and aria-valuenow/min/max), and no keyboard handler. Keyboard-only users cannot resize the sidebar/chat panels. A drag-only resize handle is a standard a11y gap.

## Recommendation

Give each handle `role='separator'`, `tabIndex={0}`, `aria-orientation='vertical'`, `aria-valuenow`/`aria-valuemin`/`aria-valuemax` reflecting the current width, and an onKeyDown handler that adjusts the width by a step (e.g. 16px) on ArrowLeft/ArrowRight.

## Verification

Read App.tsx lines 666-672 and 688-694 — both are <div onMouseDown=...> with only a title attribute; no role, tabIndex, or keyboard handler.
