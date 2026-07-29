- name: Mobile settings nav overlay has no Escape/close-on-outside-click and hardcoded top offset
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/SettingsPanel.tsx
- lines: 480-501
- description: The mobile nav overlay (lines 494-501) is toggled by a hamburger button
  (line 483) and closed only by tapping a nav item (line 469: `setShowMobileNav(false)`).
  There is no Escape handler (confirmed: no `onKeyDown`/`Escape` anywhere in the file) and
  no backdrop/outside-click close — once opened, the only way to dismiss it without
  navigating is to toggle the hamburger again. The overlay also uses a hardcoded
  `top-[53px]` (line 496) to sit below the mobile header; if the header height changes
  (padding, font, safe-area inset), the overlay overlaps or leaves a gap. Compare to the
  radix Dialog/Popover primitives used elsewhere which handle Escape, focus trap, and
  outside-click automatically. This overlay is hand-rolled and skips all of them. On
  mobile this is the primary settings navigation surface, so the gap is user-facing.
- verification: Read `SettingsPanel.tsx:480-501` (overlay markup) and grepped the file for
  `Escape`/`onKeyDown` (no matches). `top-[53px]` is an arbitrary value, not a token.
