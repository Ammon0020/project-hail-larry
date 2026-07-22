# Story S-PROF-UI: Settings Profiles Tab

> **Status:** pending | **Difficulty:** med
> **Epic:** [profiles-over-acp](../pending-profiles-over-acp-hard.md).
> **Depends on:** S-PROF-REST, S-PROF-TOOLS.

## Goal

Add a "Profiles" tab to the Settings panel where users create, rename, edit, and
delete profiles — setting each profile's label, instructions, and per-tool
whitelist (checkboxes populated from live MCP tool enumeration).

## Background / current behavior

- `web/src/components/SettingsPanel.tsx` has tabs `'agents' | 'mcp' | 'general'`
  (line 83); MCP tab at line 529, General at line 588. No profiles surface.
- Data comes from `GET/PUT /api/profiles` (S-PROF-REST); tool list from the
  MCP enumeration surfaced by S-PROF-TOOLS.

## Desired behavior

- New `'profiles'` tab value + panel following the existing MCP/General tab
  patterns and UI SKILL (semantic tokens, `cn`, mobile-first).
- Profile list with add / rename / delete; select one to edit its `label`,
  `instructions` (textarea), and `tools` whitelist (checkbox list from
  enumerated MCP tools, grouped by server).
- A `defaultProfileId` selector. Save calls `PUT /api/profiles`; validation
  errors from the backend are surfaced inline (fail loudly, no silent revert).
- Deleting the default profile is prevented or forces choosing a new default.

## Acceptance criteria

- [ ] Profiles tab lists profiles from `GET /api/profiles` and shows built-in
      defaults when no file exists.
- [ ] Add / rename / delete / edit instructions / toggle tools all persist via
      `PUT /api/profiles` and survive reload.
- [ ] Tool checkboxes reflect live enumerated MCP tools; stale/unknown tools in
      a saved whitelist are shown as such (not silently dropped in the editor).
- [ ] Backend validation errors (bad name, oversized, dangling default) are
      shown inline; the panel does not claim success on failure.
- [ ] Cannot leave the config without a valid `defaultProfileId`.
- [ ] `npm run build` and lint clean; mobile layout verified.

## Out of scope

- Chat selector wiring / persistence (S-PROF-CHAT).
- Backend enumeration/CRUD (S-PROF-TOOLS, S-PROF-REST).
