# RefreshCw status icons missing aria-hidden / reliable label

- **Difficulty:** easy
- **Urgency:** low
- **File:** `web/src/components/EditorPane.tsx`
- **Lines:** 305-310, 378-384

## Description

Two new `<RefreshCw>` icons are introduced. The tab icon (line 306-309) is a standalone status indicator with `aria-label="Changed on disk"`, but `aria-label` on an inline `<svg>` is inconsistently honored across screen-reader/browser pairs and is not a reliable accessible name. The banner icon (line 383) sits next to the visible "Reload" text but has no `aria-hidden`, so some screen readers will announce the SVG as well as the text, producing redundant/noisy output. Neither icon is keyboard-focusable (they're decorative), so they should either be hidden from AT or carry a proper label via a wrapping element.

## Recommendation

For the banner icon (paired with text), add `aria-hidden="true"`. For the tab status icon, wrap with a `<span className="inline-flex …">` containing the icon (`aria-hidden`) plus a `<span className="sr-only">Changed on disk</span>`, or append the status to the tab's existing accessible name so the tab itself conveys "filename, changed on disk".

## Verification

Read `EditorPane.tsx` lines 305-310 and 378-384; lucide-react renders an `<svg>` without `aria-hidden` by default, and neither icon supplies `aria-hidden`. The tab container is a `<div onClick=…>` with no `role`/`aria-label`, so the icon's `aria-label` is the only AT signal — and it's on an unreliable element type.
