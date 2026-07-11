# MCP Server Configuration — Design Research

Status: **Research / design document. No code changes.**
Date: 2026-06-05
Scope: How the Local Agent Interface should let users configure, edit, enable/disable, and document MCP servers that are passed to ACP agents on `session/new` and `session/load`.

---

## 0. TL;DR (recommendations)

| Question | Recommendation |
|---|---|
| Storage format | **Claude Desktop–compatible `mcpServers` map** stored as `~/.local-agent/mcp.json`, wrapped on disk with a small envelope that carries `enabled` flags. On-the-wire to ACP we translate to `acp.McpServer`. |
| Storage location | **Global first** (`~/.local-agent/mcp.json`). Per-workspace override is a *later* addition (`.local-agent/mcp.json` next to the workspace root), not v1. |
| JSON editor | **CodeMirror 6 + `@codemirror/lang-json`** (already a dependency via `@uiw/react-codemirror`). Add `codemirror-json-schema` for inline validation against our schema. No Monaco. |
| Enable/disable | **Flag, do not delete.** Keep the entry in `mcpServers`; add a sibling `enabled: bool` in our envelope. Disabled servers are filtered out before being sent to ACP. |
| Live session updates | **Not in v1.** ACP only accepts `mcpServers` on `session/new`, `session/load`, and `session/resume`. Changing MCP config requires starting a new session (or resuming). Surface this in the UI. |
| Help/docs | **Inline collapsible help panel + a "Copy example" button + a link to `docs/reference/mcp/`.** No external site dependency. |
| New deps | `@codemirror/lang-json` (tiny), `codemirror-json-schema` (optional, recommended). No MCP client library — ACP handles MCP plumbing; we only configure it. |

---

## 1. Config storage format

### 1.1 What `acp.McpServer` actually looks like

From `go doc github.com/coder/acp-go-sdk`:

```go
type McpServer struct {
    Http *McpServerHttpInline `json:"-"` // requires mcp_capabilities.http
    Sse  *McpServerSseInline  `json:"-"` // requires mcp_capabilities.sse
    Acp  *McpServerAcpInline  `json:"-"` // UNSTABLE; requires mcp_capabilities.acp
    Stdio *McpServerStdio     `json:"-"` // ALL agents MUST support this
}
```

All four transport fields are `json:"-"` — the type has custom `MarshalJSON`/`UnmarshalJSON` that emits exactly one of them based on which pointer is set. The on-the-wire JSON is therefore a *discriminated union* with no outer `type` tag for stdio (the spec adds `type` for http/sse/acp).

Transport shapes:

```go
type McpServerStdio struct {
    Meta    map[string]any `json:"_meta,omitempty"`
    Args    []string       `json:"args"`
    Command string         `json:"command"`
    Env     []EnvVariable  `json:"env"`     // [{name, value}] — NOT a map
    Name    string         `json:"name"`
}

type McpServerSseInline struct {  // also McpServerHttpInline
    Meta    map[string]any `json:"_meta,omitempty"`
    Headers []HttpHeader   `json:"headers"`  // [{name, value}]
    Name    string         `json:"name"`
    Type    string         `json:"type"`     // "sse" or "http"
    Url     string         `json:"url"`
}

type EnvVariable  struct { Name string; Value string; Meta map[string]any }
type HttpHeader   struct { Name string; Value string; Meta map[string]any }
```

Two important gotchas for our config layer:
1. ACP's `Env`/`Headers` are **arrays of `{name,value}`**, not the `{"KEY":"val"}` map that Claude Desktop uses. We must translate.
2. ACP has no concept of "disabled" — `McpServer` is purely connection config. Enable/disable is *our* concern, layered above.

### 1.2 How other editors store MCP config

| Editor | File | Shape | Notes |
|---|---|---|---|
| **Claude Desktop** | `~/Library/Application Support/Claude/claude_desktop_config.json` (mac), `%APPDATA%\Claude\claude_desktop_config.json` (win), `~/.config/Claude/claude_desktop_config.json` (linux) | `{ "mcpServers": { "<name>": { "command", "args", "env": {K:V} } } }` | The de-facto standard. Map keyed by server name. stdio implied when no `type`. |
| **Claude Code** | `~/.claude.json` (user) and `.mcp.json` (project) | Same `mcpServers` map; adds `type: "http"|"sse"` for remote. Supports `${VAR}` env expansion. | Project-scope `.mcp.json` is meant to be committed. |
| **Cursor** | `~/.cursor/mcp.json` (global) and `.cursor/mcp.json` (project) | Identical to Claude Desktop. | Explicitly advertised as copy/paste-compatible with Claude. |
| **Windsurf** | `~/.codeium/windsurf/mcp_config.json` | Same `mcpServers` map. | UI also exposes a marketplace; raw JSON is editable. |
| **VS Code** | `.vscode/mcp.json` (workspace) and user profile mcp.json | `{ "servers": { "<name>": { "type", "command", "args", "env", "cwd", "envFile", "inputs", "sandboxEnabled" } }, "inputs": [...], "sandbox": {...} }` | Uses `servers` not `mcpServers`. Adds `inputs` for `${input:api-key}` prompts, `envFile`, sandbox. |

The "MCP JSON configuration" format (Claude Desktop's `mcpServers` map) is the emergent standard that FastMCP, Cursor, Windsurf, and Claude Code all share. VS Code is the outlier with `servers` + `inputs`, but its values are otherwise the same shape.

### 1.3 Three candidate formats for us

**Option A — Pure Claude Desktop format (no envelope).**
File: `~/.local-agent/mcp.json` containing exactly `{ "mcpServers": { ... } }`.
- ✅ Byte-for-byte copy/paste with Claude Desktop, Cursor, Windsurf.
- ❌ No place to put `enabled`. Would have to delete entries to disable (loses config) or use a sidecar file.
- ❌ No place for our own metadata (last edited, source, notes).

**Option B — Claude Desktop format + sidecar enable/state file.**
`mcp.json` is pure Claude format; a separate `mcp_state.json` holds `{ "disabled": ["github", "linear"] }`.
- ✅ Main file stays portable.
- ❌ Two files to keep in sync; deleting a server in `mcp.json` leaves stale entries in `mcp_state.json`; confusing for users who hand-edit.

**Option C (recommended) — Claude Desktop–compatible `mcpServers` map inside a small envelope.**
```jsonc
// ~/.local-agent/mcp.json
{
  "$schema": "https://local-agent.dev/schemas/mcp.json",
  "version": 1,
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "${GITHUB_TOKEN}" },
      "enabled": true
    },
    "linear": {
      "type": "http",
      "url": "https://mcp.linear.app/mcp",
      "headers": { "Authorization": "Bearer ${LINEAR_TOKEN}" },
      "enabled": false
    }
  }
}
```
- ✅ The `mcpServers` *value* is Claude Desktop–compatible — copy/paste of the inner object works against Claude/Cursor/Windsurf. The only thing we add is an `enabled` field per server, which other editors simply ignore (unknown fields are ignored by their parsers).
- ✅ One file. `enabled` survives round-trips through other editors (they drop it on save, which is fine — re-enable in our UI).
- ✅ `version` lets us migrate later. `$schema` powers editor autocomplete for users who open the file in VS Code.
- ✅ Env is a `{"K":"V"}` map (Claude style) on disk; we translate to ACP's `[]EnvVariable` at session-start time.
- ⚠️ We add `enabled` and `version`/`$schema` — strictly a superset, not byte-identical to Claude Desktop at the top level. This is the same tradeoff VS Code made.

**Recommendation: Option C.** It maximizes copy/paste portability (the part users actually care about — the per-server object) while giving us a clean home for `enabled` and future fields.

### 1.4 On-the-wire translation (Go)

We introduce an internal type, **not** persisted directly:

```go
// internal/mcp/config.go (proposed)
package mcp

// ServerConfig is our on-disk representation of one MCP server entry.
// It is a superset of the Claude Desktop / Cursor / Windsurf shape.
type ServerConfig struct {
    Type    string            `json:"type,omitempty"`    // "http" | "sse" | "stdio" (default stdio)
    Command string            `json:"command,omitempty"` // stdio
    Args    []string          `json:"args,omitempty"`     // stdio
    Env     map[string]string `json:"env,omitempty"`      // stdio (Claude-style map)
    Cwd     string            `json:"cwd,omitempty"`      // stdio (optional)
    Url     string            `json:"url,omitempty"`      // http/sse
    Headers map[string]string `json:"headers,omitempty"`  // http/sse
    Enabled *bool             `json:"enabled,omitempty"`  // nil => true
}

// File is the on-disk envelope.
type File struct {
    Schema    string                  `json:"$schema,omitempty"`
    Version   int                     `json:"version"`
    McpServers map[string]ServerConfig `json:"mcpServers"`
}
```

Translation to `acp.McpServer` happens in one place, e.g. `mcp.ToACP(name, cfg) (acp.McpServer, error)`, called from `internal/acp/transport.go` where `McpServers: []acp.McpServer{}` currently lives (lines 488 and 510). Disabled servers are filtered before translation.

### 1.5 `${VAR}` env expansion

Claude Code, Cursor, and VS Code all support `${VAR}` (and VS Code adds `${input:name}`). We should support `${VAR}` expansion against `os.Getenv` at session-start time, in `mcp.ToACP`. This keeps secrets out of `mcp.json` so it can be shared/snapshotted. Document it.

---

## 2. Config storage location

### 2.1 How `internal/config/` works today

`internal/config/config.go`:
- One `Config` struct, persisted as `~/.local-agent/config.json` (mode 0600, dir 0700).
- `Load()` returns `Default()` if the file is missing; fills per-field defaults on partial files.
- `Save()` is mutex-guarded (`c.mu`); helpers `UpsertAgent`, `DeleteAgent`, `RemoveWorkspacePath` all lock-then-save.
- Already imports `internal/acp` for `acp.AgentInfo`. Adding an `mcp` subpackage or a field on `Config` are both clean.

Two integration options:

**2.1.a — Add `McpServers` to the existing `Config` struct.**
```go
type Config struct {
    // ...existing fields...
    McpServers map[string]mcp.ServerConfig `json:"mcpServers,omitempty"`
}
```
- ✅ Single file, single mutex, single load/save. Matches how `Agents` already lives on `Config`.
- ❌ Mixes "app settings" with "potentially large, frequently-edited MCP JSON." Users who want to copy/paste the whole `mcp.json` from a friend have to extract one nested object.

**2.1.b (recommended) — Separate `~/.local-agent/mcp.json`, loaded/saved by a new `internal/mcp` package.**
- ✅ The file *is* the portable artifact — users can `cp friend/mcp.json ~/.local-agent/mcp.json`.
- ✅ Keeps `config.json` small and stable.
- ✅ Mirrors how Claude Desktop/Cursor/Windsurf all keep MCP config in its own file.
- The `internal/mcp` package exposes `Load() (*File, error)`, `(f *File) Save() error`, `Enabled() []acp.McpServer`, and `ToACP(name, cfg)`. Same mutex pattern as `internal/config`.

### 2.2 Global vs per-workspace

ACP sends `mcpServers` per session, and sessions are per-workspace, so per-workspace config is *technically* the natural unit. But:

- v1 has no UI for per-workspace settings, and the settings modal is global today.
- Most MCP servers (GitHub, Linear, filesystem) are wanted everywhere.
- Cursor/Claude Code/Windsurf all default to global with *optional* project-scope override.

**Recommendation: global only for v1** (`~/.local-agent/mcp.json`). Reserve the design for a future `<workspace>/.local-agent/mcp.json` that *overrides* (merges, last-write-wins per server name) the global file. The `internal/mcp` package should already be written to take a "search paths" list so workspace override is additive later.

---

## 3. JSON editing UI

### 3.1 What's already in the app

`web/package.json` already pulls in `@uiw/react-codemirror` plus a full CodeMirror 6 bundle (`@codemirror/state`, `@codemirror/view`, `@codemirror/language`, `@codemirror/lang-{css,html,javascript,markdown,python}`, `@codemirror/theme-one-dark`, `@codemirror/search`, `@codemirror/autocomplete`, `@codemirror/language-data`). `EditorPane.tsx` uses `@uiw/react-codemirror` directly. **Monaco is not a dependency.**

Notably **`@codemirror/lang-json` is not yet installed** — it's the one missing piece.

### 3.2 Three candidate editors

**Option A — Plain `<textarea>` with a syntax-highlight overlay.**
- ✅ Zero deps, trivial.
- ❌ No validation, no autocomplete, no brace matching. Feels cheap. Errors only on save.

**Option B — `@uiw/react-codemirror` + `@codemirror/lang-json` (recommended).**
- ✅ Reuses the exact stack `EditorPane` already uses. One new dep (`@codemirror/lang-json`, ~10KB).
- ✅ JSON syntax highlighting, bracket matching, fold, lint gutter out of the box.
- ✅ Copy/paste works as plain text — the editor's buffer *is* the file contents, so pasting a Claude Desktop `mcpServers` block and saving produces a byte-identical file. This is the user's stated requirement.
- ✅ Optional: add `codemirror-json-schema` for live validation against our JSON Schema (red squiggles, hover diagnostics). Schema is small and self-hosted.
- ⚠️ Slightly more wiring than a textarea; trivial since `EditorPane` already shows the pattern.

**Option C — Monaco Editor.**
- ✅ Full VS Code experience, JSON schema support built in.
- ❌ ~2MB bundle, separate worker setup, conflicts with the CodeMirror-everywhere choice. Adds a second editor engine for one panel. **Not worth it.**

**Option D — `react-json-view` / `jsoncrack` (tree/graph viewers).**
- These are *viewers*, not editors. Useful as a read-only "what's configured" list, but the user explicitly wants JSON editing for copy/paste. Skip for v1; maybe add a tree view alongside the JSON editor later.

**Recommendation: Option B**, with `codemirror-json-schema` for inline validation. The settings panel becomes:

```
┌─ Settings ────────────────────────────────────────┐
│ [Agents] [MCP Servers] [General]                  │
│                                                   │
│ MCP Servers                          (?) Help  ↻   │
│ ┌───────────────────────────────────────────────┐ │
│ │ {                          [CodeMirror, JSON]  │ │
│ │   "mcpServers": {                              │ │
│ │     "github": { ⚠️ missing 'command' }         │ │
│ │     ...                                        │ │
│ │   }                                            │ │
│ │ }                                              │ │
│ └───────────────────────────────────────────────┘ │
│ ▸ Quick reference (collapsed by default)          │
│   - stdio example    [Copy]                       │
│   - http example     [Copy]                       │
│   - ${VAR} expansion                             │
│   - Docs: docs/reference/mcp/                     │
│                                                   │
│ [Save]  [Revert]   ⚠ 3 servers, 1 disabled        │
└───────────────────────────────────────────────────┘
```

The "enabled" toggle can live *both* in the JSON (`"enabled": false`) and as a row of quick-toggle chips above the editor ("github ✓ | linear ✗ | filesystem ✓") that edit the JSON in place. This satisfies "easily turn off an MCP server at any time" without forcing the user to find it in the JSON.

### 3.3 Validation flow

1. CodeMirror `codemirror-json-schema` lints as the user types (syntax + schema).
2. On **Save**, the frontend POSTs the raw text to `PUT /api/mcp` (proposed). The Go side parses with `json.Unmarshal` into `mcp.File`, applies defaults, expands `${VAR}` *only at session start* (not on save — save stores the literal `${VAR}`), and writes `~/.local-agent/mcp.json` atomically (write temp + rename, like `config.saveLocked` should).
3. Parse errors come back as `400` with a JSON `{error, line, column}` if we want to be fancy; otherwise a plain message.

---

## 4. Enable/disable semantics

### 4.1 Storage

- **Flag, never delete.** `enabled` lives on each `ServerConfig` (Section 1.3). Default `true` when the field is omitted (so pasted Claude Desktop config is fully enabled by default — matches user expectation).
- The toggle chips in the UI flip `enabled` in the in-memory `File` and re-save.
- Deleting a server is a separate, explicit action (trash icon on the chip, or removing the key in the JSON editor).

### 4.2 Interaction with active sessions

ACP only accepts `mcpServers` on three methods: `session/new`, `session/load`, and `session/resume` (confirmed in the spec at `agentclientprotocol.com/protocol/v1/session-setup`). There is **no** `session/mcp/add` or `session/mcp/remove` method in v1. The ACP v2 proposal mentions moving MCP under `session.mcp` but does not add live add/remove.

Concretely, in our code (`internal/acp/transport.go`):
- `NewSession` (line 485) builds `acp.NewSessionRequest{ McpServers: []acp.McpServer{} }`.
- `LoadSession` (line 506) builds `acp.LoadSessionRequest{ McpServers: []acp.McpServer{} }`.

Both currently pass an empty list. The change is: build the list from `mcp.Enabled()` (filtered to the agent's advertised `mcp_capabilities` — drop http servers if the agent only supports stdio, etc.).

**Implications for the UI:**
- Toggling `enabled` on a running session's MCP server does **not** affect that session. ACP mandates a restart — there is no live add/remove in v1.
- **Decided UX:** When the user toggles a server while a session is open, show an inline banner in the chat panel: *"MCP config changed — restart to apply"* with a "Restart session" button. The restart button calls `session/load` (if the agent supports it) or starts a new session.
- If the agent advertised `sessionCapabilities.resume`, we can call `session/resume` with the new MCP list to apply changes with minimal disruption (no history replay). This is the cleanest "apply now" path and worth detecting.

### 4.3 Capability filtering

At session start, filter the enabled MCP servers by what the agent advertised in `InitializeResponse.AgentCapabilities.McpCapabilities`:
- `http` true → include `type: "http"` servers
- `sse`  true → include `type: "sse"` servers
- `stdio` is always supported (spec: "All Agents MUST support this transport")
- `acp`  → only if we ever surface ACP-transport servers (unstable; skip in v1)

If a user has an http server configured but the agent doesn't support http, log a warning and skip it (don't fail the session). Surface this in the settings UI as a per-server badge: "⚠ not supported by current agent" — though since settings is global and agent is per-session, this badge is best shown in the *chat panel's* MCP popover, not in global settings.

---

## 5. Help / documentation

### 5.1 Where the docs live

Per `AGENTS.md`, stable reference material goes in `docs/reference/<topic>/`. Create `docs/reference/mcp/` with:
- `config.md` — the format, field-by-field, with stdio/http/sse examples.
- `transports.md` — when to use stdio vs http vs sse.
- `env-vars.md` — `${VAR}` expansion, why secrets should be referenced not inlined, examples with `GITHUB_TOKEN`.
- `compatibility.md` — "this format is compatible with Claude Desktop, Cursor, Windsurf; here's how to copy a server block between editors."
- `examples/` — drop-in example JSON snippets (github, linear, filesystem, postgres, brave-search).

### 5.2 How help surfaces in the UI

Three layers, in order of intrusiveness:

1. **Inline collapsible "Quick reference" panel** under the editor (Section 3.2). Always one click away, no navigation. Contains 2–3 copy-pasteable examples and the `${VAR}` rule. This is the "optional help" the user asked for.
2. **Tooltips / `title` attributes** on the toggle chips: hover "github" → "stdio · npx @modelcontextprotocol/server-github · enabled". Hover the warning badge → the validation message.
3. **"Open full docs" link** to a rendered page. Since the app is self-hosted with no external site dependency, link to a route served by our own daemon, e.g. `GET /docs/mcp/config` → render `docs/reference/mcp/config.md` (or just serve the markdown and let the browser show it). Avoid linking to external sites that may be unreachable on a LAN-only device.

### 5.3 What the docs must cover

- The file location (`~/.local-agent/mcp.json`) and that it's separate from `config.json`.
- The `mcpServers` map shape, with one stdio and one http example.
- `enabled` field semantics (default true, toggling doesn't delete).
- `${VAR}` expansion (when it happens, what happens if the var is unset — leave the literal? fail the server? recommend: leave literal, let the MCP server fail with a clear error).
- Compatibility note: "the per-server object is byte-compatible with Claude Desktop / Cursor / Windsurf; paste the inner object, give it a key."
- Security note: don't commit raw API keys; use `${VAR}`; the file is mode 0600.
- Limitation: changes require a new/resumed session.

---

## 6. Library recommendations (honest)

### 6.1 JSON editing

| Library | Verdict |
|---|---|
| `@codemirror/lang-json` | **Add.** Tiny, required for JSON highlighting in our existing CodeMirror setup. |
| `codemirror-json-schema` | **Add (recommended).** Gives live validation against our schema. ~small. Without it, errors only surface on Save. If we want to ship v1 faster, skip and rely on server-side validation; add later. |
| `@uiw/react-codemirror` | Already installed. Reuse. |
| `monaco-editor` / `@monaco-editor/react` | **Skip.** 2MB, second editor engine, no marginal benefit over CodeMirror for a single JSON panel. |
| `react-json-view` / `jsoncrack` / `react-json-tree` | **Skip for v1.** Viewers, not editors. A tree view is a nice-to-have alongside the JSON editor, not instead of it. |
| `@rjsf/*` (JSON Forms) | **Skip.** Form-generation from schema is overkill; users explicitly want raw JSON for copy/paste. |

### 6.2 JSON schema validation (server side)

- Go stdlib `encoding/json` for parsing.
- For schema validation, **`github.com/santhosh-tekuri/jsonschema/v6`** is the standard Go JSON Schema validator if we want server-side schema checks beyond "does it parse." Probably unnecessary for v1 — structural validation (right keys, right types) can be hand-coded in `internal/mcp` in ~50 lines since our schema is tiny. Add the library only if the schema grows.
- The *frontend* schema (for `codemirror-json-schema`) is a small JSON file we ship, e.g. `web/src/schemas/mcp.json` and also serve at `/schemas/mcp.json` for the `$schema` field.

### 6.3 MCP client libraries

- **None needed.** ACP defines MCP plumbing as the *agent's* responsibility — the agent connects to the MCP servers we list; we just pass config. We do not speak MCP directly. Do not add `mark3labs/mcp-go` or similar.

### 6.4 Net new dependencies

- `@codemirror/lang-json` (required)
- `codemirror-json-schema` (recommended, optional)
- Possibly `github.com/santhosh-tekuri/jsonschema/v6` later (optional, only if server-side schema validation grows beyond trivial)

No other deps. No MCP client. No Monaco.

---

## 7. Proposed file/code structure (for the implementing agent — not implemented here)

```
internal/mcp/
  config.go      // File, ServerConfig, Load, Save, Enabled, ToACP
  config_test.go // round-trip, ${VAR} expansion, capability filtering, enable/disable
  schema.go      // (optional) embeds mcp.json schema for validation
internal/server/
  api.go         // handlers: GET /api/mcp, PUT /api/mcp
  server.go      // register routes
internal/acp/
  transport.go   // NewSession/LoadSession: replace `McpServers: []acp.McpServer{}` 
                 //   with mcp.Enabled() filtered by agent capabilities
web/src/schemas/mcp.json
web/src/components/
  McpSettings.tsx        // the [MCP Servers] tab inside SettingsModal
  McpJsonEditor.tsx      // CodeMirror + lang-json + json-schema
  McpQuickReference.tsx  // collapsible help + copyable examples
docs/reference/mcp/
  config.md
  transports.md
  env-vars.md
  compatibility.md
  examples/github.json
  examples/linear.json
  examples/filesystem.json
~/.local-agent/mcp.json   // user file, created on first save
```

API surface (proposed):
- `GET  /api/mcp` → returns the `File` (with `enabled` flags; `${VAR}` left literal).
- `PUT  /api/mcp` → accepts raw JSON text or a parsed `File`; validates; saves atomically.
- `GET  /api/mcp/servers` → convenience: `[{name, type, enabled, summary}]` for the toggle chips.
- `PATCH /api/mcp/servers/{name}` → `{ "enabled": bool }` for the toggle chips without rewriting the whole file.

The `PATCH` endpoint matters: the toggle chips should not require sending the entire JSON back. It also means a user editing JSON in the editor and a user flipping a chip on another paired device won't clobber each other as badly (still last-write-wins on the file, but the chip flip only touches one key).

---

## 8. Open questions for the implementer (not blocking this research)

1. When `${VAR}` is unset at session start, do we (a) drop the server with a warning, (b) pass the literal `${VAR}` through and let the MCP server fail, or (c) fail session start? Recommend (a) with a clear log + UI warning, since a missing secret shouldn't brick the whole chat.
2. Should `mcp.json` be per-device or shared across paired devices? Since `~/.local-agent/` is on the host daemon and all paired devices are thin clients, it's inherently shared — confirm this is desired.
3. Do we want a "test connection" button per server? ACP doesn't expose MCP health to us; we'd have to spawn the stdio process ourselves just to ping it. Recommend **no** for v1 — the agent reports MCP failures during `session/new`.
4. Should we support VS Code's `inputs` / `${input:api-key}` mechanism? Recommend **no** for v1 — `${VAR}` covers the 90% case and is what Claude/Cursor/Windsurf use.
