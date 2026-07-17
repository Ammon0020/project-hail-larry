---
name: ui-development
description: Build and refactor this project's React and Tailwind interface with maintainable component boundaries, tokens, variants, responsive behavior, and accessible states. Use for UI components, layouts, pages, visual refinements, or frontend styling work under web/.
---

# UI Development

Implement UI using utilities by default while extracting only meaningful,
reusable product concepts.

## Workflow

1. Inspect nearby components, `web/src/components/ui`, and theme tokens before
   adding a new pattern.
2. Keep local layout and styling in JSX. Use semantic tokens and mobile-first
   utilities; order classes layout, spacing, typography, color, then state.
3. Extract a React component only when markup, behavior, and styling recur as a
   meaningful UI concept across files. Do not extract utility-only wrappers.
4. Give reusable components a small public API. Use `cva` for stable visual
   variants and `cn` for conditional classes; avoid conflicting overrides.
5. Promote values repeated in multiple places to a theme token or component
   variant. Keep arbitrary values for genuine one-offs.
6. Use CSS only for theme/base rules, third-party markup, or a selector that
   utilities cannot express clearly. Do not use `@apply` for ordinary app UI.

## Quality bar

- Preserve keyboard, focus, disabled, loading, error, and dark-theme behavior.
- Test the smallest screen first, then add breakpoints only for needed changes.
- Add brief comments only for non-obvious design constraints or behavior.
- Run `npm run lint --silent` and `npm run build --silent` in `web/` for UI
  changes.
