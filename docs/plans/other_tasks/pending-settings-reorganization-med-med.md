# Settings panel reorganization — hierarchical subsections

> UI reorganization. Difficulty: medium. Urgency: medium.
> Source: settings inventory + future-settings audit (2026-07-26).

## Goal

Replace the flat 4-tab settings panel (Agents & Models, MCP Servers, General,
Profiles) with a hierarchical left-bar navigation: top-level groups with
collapsible subsections. Each subsection is a focused page with 2-6 settings.
This scales as new settings arrive (editor config, permissions, server/network,
context sources) without reorganizing existing tabs.

## Why

The "General" tab is a grab-bag mixing appearance, workspace security, agent
behavior, and session-scoped providers. Editor settings are scattered (font
size in StatusBar, word wrap in TabBar, tab size/line numbers hardcoded in
EditorPane with no UI). Eight backend config fields have no UI. Roadmap items
(permission policy, context sources, command timeout, log level) need homes.
A flat tab structure would hit 8-10 tabs within a few epics; hierarchical
subsections absorb new settings without reorganization.

## Target structure

```
Appearance
  ├─ Theme              (dark/light/system — from General)
  └─ Editor             (font size, word wrap, tab size, line numbers,
                         fold gutter, bracket matching, auto-indent —
                         consolidate from StatusBar/TabBar/hardcoded)

Agents & AI
  ├─ Harnesses          (current Agents & Models — agent registration)
  ├─ Profiles           (current Profiles tab — unchanged content)
  ├─ Prompt Context     (open file limit, workspace file list limit —
                         from General; future: context policy, git/AGENTS.md)
  └─ Providers          (advanced, session-scoped — from General;
                         only visible when a session is active)

Tools
  └─ MCP Servers        (current MCP Servers tab — unchanged content)

Workspace
  ├─ Preview            (trust state — from General; future: live reload debounce)
  └─ Permissions        (future: auto-approve rules, command timeout — empty for now,
                         show "coming soon" placeholder)

Server & Network
  ├─ Connection         (port, host, TLS enabled, HTTPS port, TLS cert dir —
                         currently no UI; read-only display with "edit config.toml"
                         guidance, since these require daemon restart)
  ├─ Pairing            (pairing TTL, credential inactivity TTL, allow remote
                         workspace registration — currently no UI; same read-only
                         pattern)
  └─ Security           (revocation grace period — currently no UI; same read-only
                         pattern; future: audit log retention)
```

## Scope

### In scope

1. Restructure SettingsPanel left bar: top-level groups with collapsible
   subsections (chevron + nested list). Active subsection highlighted.
2. Move existing settings into their new subsections (no setting content
   changes — pure reorganization).
3. Add Editor subsection: consolidate font size + word wrap (currently in
   StatusBar/TabBar) into settings; add tab size, line numbers, fold gutter,
   bracket matching, auto-indent as new settings (persisted to localStorage,
   read by EditorPane).
4. Add Server & Network section: read-only display of port, host, TLS, pairing
   TTLs, etc. with "edit config.toml" guidance (these require daemon restart;
   no live editing in this story).
5. Add Workspace → Permissions placeholder ("coming soon").
6. Mobile: subsections render as a scrollable list within the settings panel
   (same left-bar pattern, just narrower).

### Out of scope

- Implementing permission policy settings (separate story).
- Implementing context source settings (separate story).
- Live-editing server/network config (requires daemon restart flow — separate
  story).
- Changing the inline quick-access controls (StatusBar font size, TabBar word
  wrap, chat MCP toggles) — they stay as shortcuts; settings panel is the full
  view.

## Architecture decisions

- **Editor settings storage**: localStorage (client-side), matching the
  existing pattern for font size (`lai:fontSize`) and word wrap (`lai:wrap`).
  New keys: `lai:tabSize`, `lai:lineNumbers`, `lai:foldGutter`,
  `lai:bracketMatching`, `lai:autoIndent`. Server-side config is not involved
  — these are per-client preferences like theme.
- **Server & Network display**: read-only. Fetch current config via existing
  `GET /api/config` (or whichever endpoint exposes config — verify during
  implementation). Show values with a note: "Edit `config.toml` and restart
  the daemon to change these." No PUT endpoint in this story.
- **Subsection navigation**: collapsible groups in the left bar. On desktop,
  the left bar shows top-level items with a chevron; clicking expands to show
  subsections. On mobile, same pattern in a narrower column. Active subsection
  is highlighted. State (which groups are expanded) is ephemeral (resets on
  panel close) — no persistence needed.
- **Providers subsection visibility**: only shown when a session is active
  (same gating as current General tab Providers section). When no session,
  the subsection is hidden or shows "Start a session to configure providers."

## Acceptance criteria

- [ ] Left bar shows 6 top-level groups: Appearance, Agents & AI, Tools,
      Workspace, Server & Network (and any existing group that doesn't fit
      these — verify during implementation).
- [ ] Each top-level group expands to show its subsections via a chevron.
- [ ] Clicking a subsection shows its settings page in the main panel area.
- [ ] Active subsection is visually highlighted in the left bar.
- [ ] Theme + Editor settings appear under Appearance; Editor subsection
      includes font size, word wrap, tab size, line numbers, fold gutter,
      bracket matching, auto-indent — all functional and persisted to
      localStorage.
- [ ] EditorPane reads the new localStorage settings and applies them to
      CodeMirror (tab size, line numbers, fold gutter, bracket matching,
      auto-indent).
- [ ] Existing inline controls (StatusBar font size, TabBar word wrap) still
      work and stay in sync with the settings panel.
- [ ] Agents & Models content moves to Agents & AI → Harnesses unchanged.
- [ ] Profiles content moves to Agents & AI → Profiles unchanged.
- [ ] Prompt Context settings move to Agents & AI → Prompt Context unchanged.
- [ ] Providers section moves to Agents & AI → Providers; only visible with
      an active session.
- [ ] MCP Servers content moves to Tools → MCP Servers unchanged.
- [ ] Preview Trust moves to Workspace → Preview unchanged.
- [ ] Workspace → Permissions shows a "coming soon" placeholder.
- [ ] Server & Network shows read-only config values with "edit config.toml"
      guidance.
- [ ] Mobile: left bar renders as a scrollable narrow column; subsections
      work the same way.
- [ ] `make check` passes (fmt + clippy + cargo test + frontend eslint/build
      + contract suite).

## File references

- `web/src/components/SettingsPanel.tsx` — main panel, tab structure
- `web/src/components/ProfilesSettings.tsx` — profiles tab content
- `web/src/components/EditorPane.tsx` — CodeMirror config (hardcoded settings)
- `web/src/components/StatusBar.tsx` — font size controls
- `web/src/components/TabBar.tsx` — word wrap toggle
- `web/src/App.tsx` — font size + word wrap state (localStorage)
- `web/src/lib/theme.ts` — theme storage
- `src/config/model.rs` — Config struct (server/network fields)

## Depends on

None — standalone UI reorganization. The workspace trust feature (just
implemented) moves from General to Workspace → Preview as part of this.
