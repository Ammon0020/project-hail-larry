# Chat Panel Restructure — Design Document

Status: **Design only — no code changes yet.**
Scope: Restructure `web/src/components/ChatPanel.tsx` to add top tabs, move
harness/model selectors into the input area, modularize the conversation
display, and add an overflow menu. Mobile layout is addressed.

Reference mockup: `agent_chat_update.htm` (top tabs with blue active indicator,
right-side `+` / history / `...` controls, harness + model selectors inside the
input panel next to the send button).

---

## 0. Current state (what exists today)

`ChatPanel.tsx` is a single ~720-line component that owns:

- Agent/model selection state + localStorage persistence (`lai:selectedAgent`,
  `lai:selectedModel`).
- The mid-conversation switch-agent confirmation `Dialog` (truncate-length
  picker).
- A header row containing: harness `Select`, model `Select`, connection
  indicator, hamburger button that toggles `ChatHistory` popout.
- `ChatHistory` popout (session list with rename/delete/export/workspace filter).
- The message scroll area rendering `mergedEvents` via `ChatMessageItem`, with
  `useAutoscroll` + jump-to-bottom button.
- The input composer (textarea, attach button, send/stop, pending-attachment
  previews, upload error).
- Error banner + disconnected banner.

`App.tsx` owns `activeSessionId`, `sessions`, `events`, the create/select/
rebind callbacks, and the desktop/mobile layout split. `ChatPanel` is rendered
both in the desktop right rail (`lg:w-96`, persisted `rightPanelWidth`) and
full-screen on mobile (`absolute inset-0 z-30` when `mobileView === 'chat'`).
`MobileNav` is a fixed bottom bar with four icons.

Available shadcn primitives in `web/src/components/ui/`:
`button`, `dialog`, `dropdown-menu`, `popover`, `select`. **No `tabs`, no
`scroll-area`, no `sheet`, no `tooltip`, no `separator`.**

Types of note (`web/src/types/index.ts`): `Session { id, name, time, status,
active?, agentId?, modelId?, workspace? }`, `Agent`, `AgentModel`, `AppEvent`.

---

## 1. Component architecture

### Goals driving the split
- Tab bar logic must be isolated from input logic from message-render logic,
  because the message area "will be edited soon (new message types, different
  rendering)" and must be swappable without touching the rest.
- `ChatPanel.tsx` is already too big (720 lines, ~12 concerns). The restructure
  is the right moment to decompose.
- `App.tsx` should keep owning cross-panel state (`activeSessionId`,
  `sessions`, `events`, callbacks). The new subcomponents stay presentational
  / locally-stateful only — no new global stores.

### Approach A — Single component, sub-render functions (REJECTED)
Keep one `ChatPanel`, extract `renderHeader()` / `renderTabs()` / `renderInput()`
private functions inside the file. Smallest diff, but does not satisfy the
modularity goal: the message area is still tangled with everything else, and
the file stays ~700+ lines. Reject.

### Approach B — Three children + slot for messages (RECOMMENDED)
Split `ChatPanel` into a thin orchestrator plus three presentational children,
with the message area injected as a **slot** (children / render prop) so its
rendering can be swapped without `ChatPanel` knowing about it.

```
ChatPanel (orchestrator — owns agent/model state, send/upload/error,
           switch-agent dialog, history popout open state)
├── ChatTabBar
│   props: sessions, activeSessionId, onSelectSession,
│          onNewChat, onOpenHistory, onOpenOverflow
│   (renders tabs + right-side + / history / ... controls)
│   └── ChatHistory (existing) — opened by the history button,
│       positioned via the tab bar's relative container
├── <slot> — message area (default: ConversationView)
│   ConversationView
│     props: events, pendingPermissions, permissionResolution,
│            onPermissionResponse, error, scrollContainerRef, isAtBottom,
│            onJumpToBottom
│     └── ChatMessageItem (existing) per event
└── ChatComposer (input area)
    props: agents, effectiveAgentId, effectiveModelId, onAgentChange,
           onModelChange, input, onInputChange, onSend, onStop, agentRunning,
           pendingPreviews, onRemoveAttachment, onPickFiles, uploading,
           uploadError, canSend, activeSessionId
    (renders textarea + attach + harness Select + model Select + send/stop)
```

Why this shape:
- `ChatPanel` keeps the *behavior* (state + handlers) so the switch-agent
  dialog, persistence, and `useAutoscroll` wiring don't get fragmented. Children
  are mostly presentational, which keeps prop-drilling shallow and the diff
  reviewable.
- The message area is a **slot**, not a hardcoded child. `ChatPanel` renders
  `<>{children ?? <ConversationView ... />}</>` inside the scroll region. The
  upcoming "new message types / different rendering" work swaps
  `ConversationView` (or passes a custom child) without touching
  `ChatPanel`/`ChatTabBar`/`ChatComposer`. This is the lightest pattern that
  still gives full swappability — no registry, no strategy map, just React
  children.
- `ChatHistory` is reused as-is; it just gets triggered from the tab bar's
  history button instead of the old hamburger.

### Approach C — Strategy/registry pattern for messages (REJECTED)
Define a `MessageRenderer` interface + a registry of renderers keyed by event
type, injected via a context. Most flexible, but over-engineered for the stated
need (swap the *whole* message area, not per-event-type pluggability). Adds a
context, a registry file, and indirection that hurts traceability. Reject for
now; revisit if per-event-type pluggability becomes a real requirement.

### Chosen: Approach B with the slot pattern for the message area.

### File structure
```
web/src/components/
  ChatPanel.tsx              (orchestrator — slimmed, ~300 lines target)
  ChatTabBar.tsx             (NEW — tabs + right controls)
  ChatComposer.tsx           (NEW — input area with harness/model selectors)
  ConversationView.tsx       (NEW — default message-area slot implementation)
  ChatHistory.tsx            (existing — reused, triggered from tab bar)
  ChatMessageItem.tsx        (existing — reused by ConversationView)
```

No new directories. All four files stay under the existing
`web/src/components/` flat layout, matching the rest of the app
(`EditorPane`, `LeftSidebar`, etc. are all flat).

### Props sketch (key shapes only — full types at implementation time)

```ts
// ChatTabBar
interface ChatTabBarProps {
  sessions: Session[]
  activeSessionId: string | null
  onSelectSession: (id: string) => void
  onNewChat: () => void
  onToggleHistory: () => void
  historyOpen: boolean
  onOpenOverflow: () => void   // opens the ... menu
  connected: boolean           // connection indicator moves here
}

// ChatComposer
interface ChatComposerProps {
  agents: Agent[]
  effectiveAgentId: string
  effectiveModelId: string
  onAgentChange: (id: string) => void
  onModelChange: (id: string) => void
  input: string
  onInputChange: (v: string) => void
  onSend: () => void
  onStop: () => void
  agentRunning: boolean
  canSend: boolean
  // attachments
  pendingPreviews: { url: string; name: string }[]
  onRemoveAttachment: (i: number) => void
  onPickFiles: () => void
  uploading: boolean
  uploadError: string | null
  disabled?: boolean           // agents.length === 0 etc.
}

// ConversationView (default slot)
interface ConversationViewProps {
  events: AppEvent[]                 // already-merged
  pendingPermissions: PendingPermission[]
  permissionResolution: Map<string, 'granted' | 'denied'>
  onPermissionResponse: (id: string, decision: string) => void
  error: string | null
  scrollContainerRef: React.RefObject<HTMLDivElement>
  isAtBottom: boolean
  onJumpToBottom: () => void
}
```

`ChatPanel` keeps `useAutoscroll` and passes `scrollContainerRef` +
`isAtBottom` + `onJumpToBottom` into the slot so the autoscroll contract
stays owned by the orchestrator (the ref needs to be stable across slot
swaps).

---

## 2. Tab implementation

### Approach A — shadcn `Tabs` (Radix `@radix-ui/react-tabs`) (REJECTED)
shadcn's `Tabs` is not currently installed (no `tabs.tsx` in `ui/`). Adding it
pulls in `@radix-ui/react-tabs`. Radix Tabs is designed for *content panels*
(one tab → one panel, mounted/unmounted via `forceMount`). Our use case is
*session switching* — selecting a tab swaps the data behind a single shared
message area, not a different panel per tab. Radix Tabs' content-panel model
fights this: we'd either leave `TabsContent` empty (using only `TabsList`/
`TabsTrigger` for the trigger UI and ignoring the content machinery) or mount
a `TabsContent` per session, which would mount every conversation's scroll
area at once. Either way we're paying for a primitive we don't use. Reject.

### Approach B — Custom tab bar with `<button>`s + a small `cva` variant (RECOMMENDED)
The mockup's tab styling (3px transparent top border cushion, blue top border
on active, right-edge separator, 40px header) is specific enough that a custom
implementation is shorter than wrangling Radix Tabs' classes into shape. Use a
`cva` variant for the active/inactive/hover states per the AGENTS.md rule
("Use `cva` for elements with 2+ visual variants"). No new dependency.

```ts
// ChatTabBar.tsx
const tabVariant = cva(
  'flex items-center gap-1.5 h-full px-3 text-xs border-t-[3px] border-r border-border ' +
  'border-t-transparent transition-colors whitespace-nowrap shrink-0',
  {
    variants: {
      state: {
        inactive: 'text-muted-foreground hover:text-foreground',
        active:   'text-foreground border-t-primary bg-foreground/[0.01]',
      },
    },
    defaultVariant: { state: 'inactive' },
  },
)
```

Active tab = `border-t-primary` (the blue indicator). The transparent 3px top
border on inactive tabs is the "jitter prevention cushion" from the mockup —
it keeps layout stable when the active border appears/disappears.

### Approach C — A tab library (e.g. `react-tabs`) (REJECTED)
Same content-panel mismatch as Radix, plus an extra dep we don't need. Reject.

### Chosen: Approach B — custom buttons + `cva`.

### Tab overflow
The desktop right rail is 300–700px wide (persisted `rightPanelWidth`, default
420). With ~16px padding per tab and 13px text, ~4–6 tabs fit before overflow.
Recommendation: **horizontal scroll with hidden scrollbars + scroll-on-wheel**,
not a "more" dropdown.

Reasoning:
- A "more" dropdown duplicates the history panel's job (history button is
  already right there). Two ways to find an old conversation is confusing.
- Horizontal scroll keeps every open conversation one click away, which matches
  the "tabs across the top" intent.
- Hidden scrollbars (`[scrollbar-width:none]` + `::-webkit-scrollbar{display:none}`)
  keep the mockup's clean look; wheel-to-scroll (`onWheel` →
  `scrollLeft += deltaY`) makes it discoverable on desktop trackpads/mice.

Implementation detail: wrap the tab buttons in a `div` with
`overflow-x-auto overflow-y-hidden flex` and the hidden-scrollbar classes. The
right-side controls (`+` / history / `...`) live in a sibling `shrink-0` flex
container so they never scroll away. When a new tab becomes active, scroll it
into view with `tabRef.current?.scrollIntoView({ inline: 'nearest', block:
'nearest' })` in a `useEffect` on `activeSessionId`.

### Tab close
**Close on hover for all tabs.** An `x` appears on hover for every tab
(including the active one). Clicking `x` **hides** the tab — it does **not**
delete the session. The agent keeps running in the background. The user can
reopen the chat from the history button. This is local UI state in
`ChatPanel` (`openTabIds: string[]`), updated on close/reopen. Hidden sessions
continue to receive events and run agents; reopening shows the current live
state by nature of ACP event streaming.

### Which sessions show as tabs?
**Only sessions the user has explicitly opened.** No sessions are auto-opened.
A tab is added when the user:
- Clicks the `+` button (creates a new chat)
- Opens a session from the history panel

Tabs are removed when the user clicks the `x` on the tab (hide, not delete).
This is local UI state in `ChatPanel` (`openTabIds: string[]`), persisted to
localStorage (`lai:openTabIds`). On reload, previously-open tabs are restored.

The history button shows a list of recent sessions (capped at ~6 most recent),
with a "see more" option at the bottom that opens a full searchable list
(dynamically loaded) of all historical chats. Clicking a session in either
view opens it as a tab.
becomes a problem. **Recommendation: start with all sessions, add LRU capping
as a fast follow.** It's a one-line filter change in `ChatTabBar` and keeps v1
behavior predictable.

---

## 3. Input area restructure

### What moves
The harness `Select` and model `Select` move from the top header into the
input area, next to the send button, per the mockup's `.input-actions-row` →
`.left-controls`. The connection indicator and the hamburger/history button
stay in the tab bar (the indicator is a header concern; history is a tab-bar
control).

### Layout (matches mockup)
```
┌─────────────────────────────────────────────┐
│ textarea                                     │   ← grows; placeholder
├─────────────────────────────────────────────┤
│ [📎] [Harness ▾] [Model ▾]            [↑]   │   ← input-actions-row
└─────────────────────────────────────────────┘
```

- The whole composer sits in one bordered "card" (mockup's
  `.textarea-wrapper`: `bg-input`/`bg-panel`, `border`, `rounded-lg`,
  `p-3`), with the textarea on top and the actions row below a thin
  `border-t border-white/5` divider. This is a visual change from today's
  bare textarea + floating send button — it groups the controls with the
  input, which is the whole point of moving the selectors down here.
- Pending-attachment previews + upload error render **above** the card
  (unchanged from today), so the card itself stays clean.

### Compact selector design
**Recommendation: keep the existing shadcn `Select`**, just restyle the
trigger to the mockup's compact pill: `bg-main`/`bg-background`, `border`,
`rounded-md`, `text-xs`, `px-2.5 py-1.5`, muted text that goes foreground on
hover. The current code already passes `size="sm"` and `text-xs`; we just drop
the `border-primary/50 text-primary` accent on the model selector (the mockup
uses neutral styling for both) and tighten padding.

Why not a segmented control / custom pill:
- A segmented control implies a small fixed set; agents and models are
  variable-length lists that can be long → needs a dropdown anyway.
- A custom pill dropdown would reimplement `Select`. We already have shadcn
  `Select` (Radix `@radix-ui/react-select`, already installed). Reuse it.
- The model selector's `border-primary/50 text-primary` accent was a
  "this is the model you're talking to right now" cue. With selectors moved
  into the composer, that cue is redundant (the composer *is* the
  "what you're about to send" area), so neutral styling is correct.

### Where the attach button goes
Leftmost in the actions row, exactly as the mockup shows (`[📎]` then harness
then model). Today the attach button is to the *left of* the textarea; moving
it into the actions row aligns with the mockup and frees the textarea's left
edge for cleaner text input. The hidden `<input type="file">` stays where it
is (a ref-triggered picker).

### Send / stop button
Stays bottom-right of the actions row as a circular accent button (mockup's
`.send-btn`: `bg-primary`, `rounded-full`, 28×28, white arrow). The current
code uses a rounded-rectangle send button inside the textarea; switch to the
mockup's circular button in the actions row for consistency with the new
layout. Stop button (when `agentRunning`) takes the same slot, destructive
style — unchanged behavior, just relocated.

### Switch-agent dialog
Stays owned by `ChatPanel` (it's behavioral state: `pendingAgentId`,
`truncateLength`, `confirmSwitchAgent`). `ChatComposer` calls
`onAgentChange(id)`; `ChatPanel` decides whether to switch immediately or open
the dialog. The dialog markup can stay inline in `ChatPanel` or be extracted
into a tiny `SwitchAgentDialog.tsx` if the orchestrator file gets long —
prefer inline for v1, extract if `ChatPanel` exceeds ~350 lines.

---

## 4. `...` overflow menu

### Initial contents
- **MCP servers** — opens a sub-menu or popover listing all configured MCP
  servers with enable/disable toggles. This is the primary v1 feature. When a
  server is toggled while a session is running, show an inline banner in the
  chat panel: "MCP config changed — restart to apply" with a restart button
  (calls `session/load` or `session/resume` with the updated MCP server list).
  ACP only accepts `mcpServers` on `session/new`/`session/load`/`session/resume`
  — there is no live add/remove in v1.
- (Placeholder/coming-soon entries are fine to show greyed-out, but only if
  they're genuinely soon — otherwise omit.)

### Likely future contents
- **Skills** — enable/disable configured skills for the active session.
  (Agent-side per ACP spec — would open the agent's own config files, not an
  ACP protocol feature.)
- **Rules** — view/edit the active ruleset (project + user rules).
  (Also agent-side per ACP spec.)
- **Export conversation** — already exists per-session in `ChatHistory`; surfacing it here for the *active* conversation is a reasonable duplication.

### Approach A — shadcn `DropdownMenu` (RECOMMENDED)
Already installed (`ui/dropdown-menu.tsx`). Trigger = the `...` icon button in
the tab bar; content = menu items with checkmarks for toggles (MCP tools on/off)
and separators between groups. Keyboard navigable, focus-trapped, closes on
outside click — all free. Fits a small, flat action list perfectly.

### Approach B — shadcn `Popover` with custom content
Also installed (`ui/popover.tsx`). More flexible layout (could host a small
form for "rules"), but for a menu of toggles/actions it's overkill — we'd
rebuild menu semantics (arrow keys, item roles) by hand. Reject for v1; keep
in mind for the future "rules editor" if it needs inline editing rather than
navigating to a settings page.

### Approach C — shadcn `Sheet` (NOT INSTALLED)
A slide-over panel. Would require adding `@radix-ui/react-dialog`-based sheet
primitive. Justified only if the overflow menu grows into a full settings
panel (skills + rules + MCP config with rich UI). Too heavy for v1's single
toggle. Reject for now; revisit if/when "rules" needs an inline editor.

### Chosen: Approach A — `DropdownMenu`.

The MCP-tools toggle state lives in `ChatPanel` (it's a per-conversation-view
preference that the message slot consumes), passed down as
`showMcpTools`/`onToggleMcpTools`. The `...` button's `onOpenOverflow` prop on
`ChatTabBar` is actually unnecessary under Approach A — `ChatTabBar` renders
the `DropdownMenu` inline (trigger + content) and calls a prop like
`onToggleMcpTools` directly from the menu item. Keep `ChatTabBar` free of
behavioral state by having it accept `showMcpTools` and `onToggleMcpTools` as
props and just wire the menu item to them.

---

## 5. Library recommendations

Be honest — minimize new deps. shadcn/ui already covers most of this.

| Need | Recommendation |
|---|---|
| Tab bar | **No library.** Custom `<button>`s + `cva` (§2 Approach B). Radix Tabs' content-panel model is wrong for session-switching. |
| Overflow menu | **No new dep.** Use existing `ui/dropdown-menu.tsx` (Radix DropdownMenu already installed). |
| Harness/model selectors | **No new dep.** Keep existing `ui/select.tsx`. |
| Switch-agent dialog | **No new dep.** Keep existing `ui/dialog.tsx`. |
| Tab overflow scroll | **No library.** Native `overflow-x-auto` + hidden-scrollbar utility classes + `onWheel` handler. A scroll-area lib (e.g. Radix `ScrollArea`) would fight horizontal tab layout and add a dep. |
| Animations (tab enter/exit, menu transitions) | **No new dep for v1.** Tailwind transitions (`transition-colors`) cover the hover/active states. Radix DropdownMenu/Dialog already animate their own content. If later we want spring animations for tab reordering or message transitions, add `motion` (formerly `framer-motion`) at that time — not now. |
| Tooltips on the `+`/history/`...` buttons | **Optional add:** shadcn `tooltip` (`@radix-ui/react-tooltip`). Today the code uses `title=""` attributes, which work but are ugly and slow. Adding the tooltip primitive is a small, broadly-useful dep. **Recommend: add it** — it's the one genuinely worth-it addition, and it benefits the whole app (activity bar, file tree, etc.), not just this restructure. |

**Net new deps: 0 required, 1 recommended (`@radix-ui/react-tooltip` via shadcn
`tooltip`).** No tab library, no scroll-area library, no animation library.

---

## 6. Mobile design

### Constraints from the current layout
- On mobile (`!isDesktop`, i.e. `< 1024px`), `ChatPanel` is rendered
  `absolute inset-0 z-30` — full screen — when `mobileView === 'chat'`.
- `MobileNav` is a fixed `h-16` bottom bar. The chat input already adds
  `pb-20 lg:pb-3` so the textarea clears the bottom nav.
- Width is the full viewport (≤ ~768px in practice).

### Tabs on mobile
**Recommendation: same horizontal-scroll tab strip as desktop, with two
mobile-specific tweaks:**

1. **Shorter tab labels.** Session names can be long ("Code Review
   Refactoring"). On mobile, truncate tab text to ~12–14 chars with
   `max-w-[7rem] truncate` so 2–3 tabs fit visibly. Full name shows on hover
   via the tooltip (or `title` until tooltips land).
2. **The right-side controls stay.** `+` / history / `...` remain in the
   header — they're essential and fit comfortably in ~90px. The connection
   indicator can hide on mobile (`hidden sm:inline`) to save space, since
   mobile users are either on the same device (connected) or seeing the
   reconnect banner already.

Why not a different mobile layout (e.g. tabs collapsed into the history
panel, or a dropdown tab selector):
- A dropdown tab selector on mobile duplicates the history button — confusing.
- Collapsing tabs entirely loses the "switch between active conversations in
  one tap" value, which is *more* valuable on mobile (small screen, no
  sidebar) than desktop.
- Horizontal scroll is the standard mobile pattern for browser-style tabs and
  plays well with touch (swipe already scrolls the overflow container).

### Input area on mobile
The composer's actions row (`[📎] [Harness] [Model] [↑]`) is the tightest part
on mobile. Two options:

- **A — Keep all three selectors inline.** With truncated tab labels and a
  compact textarea, `[📎] [Harness ▾] [Model ▾] ... [↑]` fits on a 360px
  screen if the selectors show only the short name (e.g. "GLM-5.2" not
  "GLM-5.2 High"). Use `SelectValue` with a custom child that truncates.
- **B — Collapse harness+model into a single "model" pill on mobile.** Show
  just the model name; tapping it opens a small popover/sheet with both
  harness and model pickers. Saves ~80px but adds a tap.

**Recommendation: A for v1** (less code, matches desktop, and the harness list
is usually 1–3 items so the selector is narrow). Revisit B if real-device
testing shows crowding.

The `pb-20` bottom padding on the composer stays so the send button clears the
`MobileNav`'s `h-16`. The mockup's circular send button sits in the actions
row, not floating over the textarea, so we lose the current "floating send
button" — on mobile that's fine (the actions row is always visible above the
bottom-nav clearance).

### Bottom-nav interaction
No change to `MobileNav`. The chat tab in the bottom nav already switches
`mobileView` to `'chat'` and shows `ChatPanel` full-screen. The new tab strip
lives *inside* `ChatPanel`, so it appears only when the chat view is active —
correct.

### One mobile gotcha
`ChatPanel`'s root uses `absolute inset-0 z-30 lg:relative`. The tab bar's
`ChatHistory` popout uses `absolute top-full left-0 right-0` — on mobile this
spans the full screen width correctly (good). The `...` `DropdownMenu` from
Radix renders in a portal, so it's viewport-positioned and works on mobile
without extra effort. No mobile-specific positioning fixes needed for either.

---

## 7. Implementation order (suggested, not part of this design)

1. Extract `ConversationView.tsx` (pure move of the message area + autoscroll
   slot wiring). Verify desktop + mobile render identically.
2. Extract `ChatComposer.tsx` (move textarea + attach + send/stop; *keep
   selectors in the old header for this step*). Verify.
3. Move harness/model `Select`s into `ChatComposer`'s actions row. Verify
   switch-agent dialog still triggers.
4. Extract `ChatTabBar.tsx` with the `+` / history / `...` controls (history
   reuses `ChatHistory`; `...` is a no-op placeholder menu for this step).
   Verify.
5. Implement the tab strip (custom buttons + `cva` + horizontal scroll +
   active blue top border).
6. Wire the `...` `DropdownMenu` with the MCP-tools toggle (plumb
   `showMcpTools` into `ConversationView`).
7. (Optional) Add shadcn `tooltip` and replace `title=""` attrs across the new
   components.
8. Update `docs/STATUS.md` row for the chat panel restructure.

Each step is independently shippable and `npm run build`-verifiable, per
AGENTS.md ("Build with `./build.sh`... `npm run build`").

---

## 8. Resolved decisions

1. **Tab membership:** Only sessions the user has explicitly opened show as
   tabs. No auto-opening. Tabs are added on new chat or opening from history.
   Local UI state (`openTabIds`), persisted to localStorage.
2. **Tab close button:** Close on hover for all tabs. Close = hide (not
   delete). Agent keeps running. History button shows recent ~6 with "see
   more" for full searchable list. Reopening shows current live state.
3. **Model selector accent:** Drop the `border-primary/50 text-primary` accent
   now that it lives in the composer. Neutral styling for both selectors.
4. **Tooltip primitive:** Add `@radix-ui/react-tooltip` now. Benefits whole app.
5. **MCP toggle UX:** Inline banner in chat panel when MCP config changes
   while a session is running: "MCP config changed — restart to apply" with
   a restart button. ACP mandates restart (no live add/remove in v1).
