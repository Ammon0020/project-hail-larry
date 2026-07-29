- name: SettingsPanel nav highlight + deep-link can feedback-loop on scroll
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/SettingsPanel.tsx
- lines: 165, 424-440
- description: `activeView` is local state driven by an `IntersectionObserver` (line 436:
  `setActiveView(...)` + `onSectionChange?.(...)`). `onSectionChange` flows up to App's
  `setSettingsSection` (App.tsx:1115), which flows back down as the `activeSection` prop.
  The deep-link effect (line 429) runs on every `activeSection` change and calls
  `scrollToSection(activeSection)` → `scrollIntoView({behavior:'smooth'})`. So: user
  scrolls → observer fires → App state updates → prop flows back → effect scrolls again.
  In practice this is usually a no-op when the section is already at the top, but smooth
  scroll can re-trigger the observer during the animation and cause visible jitter / the
  nav highlight flickering between two adjacent sections near a boundary. The design is
  fragile because the controlled `activeSection` and the observed `activeView` are two
  sources of truth for the same concept. Safer pattern: keep `activeView` purely
  observer-driven for highlight, and have the deep-link effect scroll ONLY when the
  incoming `activeSection` differs from the current `activeView` (i.e., a true external
  deep-link, not an echo of the observer's own update).
- verification: Read `SettingsPanel.tsx:165` (activeView init from activeSection),
  `429` (effect scrolls on activeSection change), `432-440` (observer calls
  onSectionChange which updates activeSection in App.tsx:1115).
