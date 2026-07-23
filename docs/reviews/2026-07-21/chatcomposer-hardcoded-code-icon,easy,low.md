# Hardcoded Code icon for every profile in the composer

- **Difficulty:** easy
- **Urgency:** low
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/components/ChatComposer.tsx`
- **Lines:** 272-275

## Description

The profile selector always renders `<Code className="w-3.5 h-3.5" />` regardless of which profile is active. Before this epic the dropdown was hard-coded to the three built-in modes (Code / Ask / Plan) and the Code icon was arguably acceptable as a generic "profile" glyph, but the selector now sources arbitrary user-defined profiles from `GET /api/profiles` (line 274 reads `profiles.find(...)?.label`). Showing a "Code" icon next to a profile labeled "Ask" or "Plan" or "Refactor review" is misleading — the icon no longer reflects the selection.

## Recommendation

Either swap the `Code` icon for a neutral profile/people glyph (e.g. `Users` from `lucide-react`, which `ProfilesSettings` already imports), or drop the icon entirely and let the visible label carry the meaning. If per-profile icons are desired later, add an optional `icon` field to `ProfileEntry` and map it here; until then, a neutral icon avoids implying a specific mode.

## Verification

Line 272 unconditionally renders `<Code className="w-3.5 h-3.5" strokeWidth={2} />` inside the profile selector wrapper. The visible label at line 273-275 already reflects the dynamic profile label, so only the icon is stale. `lucide-react`'s `Users` icon is already imported in `ProfilesSettings.tsx` line 11, confirming the project has access to a more neutral glyph.
