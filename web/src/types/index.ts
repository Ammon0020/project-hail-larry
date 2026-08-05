/**
 * Type definitions for the Local Agent Interface.
 * These shapes match what the Go daemon sends over WebSocket
 * and what ACP agents return. See Blueprint sections 4-11.
 */

/** Backend payload for a file or folder node in the workspace file tree
 *  (Blueprint Sec 13). The daemon always sends `path`, so it is required. */
export interface FileNode {
  name: string
  type: 'folder' | 'file'
  path: string
  children?: FileNode[]
}

/** UI-augmented file-tree node used by the explorer component — a `FileNode`
 *  plus display/interaction state. */
export interface FileTreeNode extends FileNode {
  icon?: string
  iconColor?: string
  expanded?: boolean
  active?: boolean
  unsaved?: boolean
  revision?: number
  children?: FileTreeNode[]
}

/** An open editor tab with file content and metadata. */
export interface Tab {
  id: string
  name: string
  path: string
  content: string
  revision: number
  unsaved: boolean
  language: string
  /** The workspace ID this file was opened from. Stored so save/reload target
   *  the correct workspace root even after a page reload changes the active
   *  workspace — without it, a tab opened from workspace A would be saved
   *  against workspace B's root and fail with "no such file or directory".
   *  Optional for backward compat with tabs persisted before this field
   *  existed; callers fall back to the active workspace when unset. */
  workspaceId?: string
  /** True when the file changed on disk (agent write or external edit) while
   *  the tab was open AND the user had unsaved edits — so the content was NOT
   *  auto-refreshed. Surfaces a "changed on disk" indicator + Reload action.
   *  Clean tabs are refreshed silently instead of setting this flag. */
  changedOnDisk?: boolean
  /** Tab content kind.
   *  - `'file'` (default): CodeMirror / FileViewer for a workspace file
   *  - `'settings'`: settings panel (synthetic path, not persisted)
   *  - `'preview'`: browse-preview iframe for a multi-file static site
   *    (distinct from `viewMode: 'preview'` / `isPreview` transient file tabs)
   *  - `'git-diff'`: structured diff view (GitDiffTab) for a single file's
   *    git changes. `path` is the file relative to the workspace root;
   *    `staged` selects the index diff vs. the working-tree diff.
   *  - `'git-commit-diff'`: multi-file commit diff view; `commitOid` identifies
   *    the commit and `path` is a display label. */
  kind?: 'file' | 'settings' | 'preview' | 'git-diff' | 'git-commit-diff'
  /** True when the file is binary (image, executable, archive, etc.) and
   *  cannot be edited as text. The editor renders a placeholder or image
   *  preview instead of a CodeMirror instance. */
  isBinary?: boolean
  /** True when the file has a visual preview available in FileViewer (images,
   *  PDF, 3D models, SVG, CSV, etc.). When isBinary is false, the file opens
   *  in CodeMirror with a "Preview" button; when isBinary is true, it opens
   *  directly in FileViewer. */
  previewable?: boolean
  /** View mode for text-preview files (SVG, CSV, HTML, OBJ, etc.). 'edit'
   *  shows CodeMirror (default); 'preview' shows the visual FileViewer.
   *  Binary-only files always show FileViewer regardless of this field. */
  viewMode?: 'edit' | 'preview'
  /** True when the tab is a transient preview (VS Code-style). Opening another
   *  file replaces the existing preview tab instead of accumulating tabs; the
   *  first edit or explicit tab click converts it to a persistent tab. Preview
   *  tabs are not persisted to localStorage. */
  isPreview?: boolean
  /** For `kind: 'git-diff'` tabs: whether to show the staged (index) diff
   *  (`true`) or the working-tree (unstaged) diff (`false`, default). */
  staged?: boolean
  /** For `kind: 'git-commit-diff'` tabs: the commit to inspect. */
  commitOid?: string
}

/** A registered AI agent (Blueprint Sec 5 — agent registration). */
export interface Agent {
  id: string
  name: string
  models: AgentModel[]
  /** Command executable used to launch the agent process. Present on agents
   *  returned by the backend; optional because the UI subset (id/name/models)
   *  is all most components need. */
  command?: string
  /** Args passed to the agent command. Present on backend responses; not
   *  used directly by the UI. */
  args?: string[]
  /** Backend warning (e.g. agent not found on PATH). Surfaced in SettingsPanel. */
  warning?: string
}

/** A model offered by a registered agent. */
export interface AgentModel {
  id: string
  name: string
  /** Agent's current/default model at detection time (e.g. Devin currentValue). */
  preferred?: boolean
  /** Agent-advertised image support when present. */
  supportsImages?: boolean
  /** Optional short description from the agent. Cost is not exposed by Devin. */
  description?: string
}

export interface McpServerConfig {
  enabled?: boolean
  command?: string
  args?: string[]
  env?: Record<string, string>
  type?: string
  url?: string
  headers?: Record<string, string>
  [key: string]: unknown
}

/** Session lifecycle states — mirrors the backend `SessionState` enum
 *  (`src/acp/core/registry.rs`). The daemon sends exactly these 6 values;
 *  no phantom states. */
export type SessionStatus =
  | 'created'
  | 'idle'
  | 'running'
  | 'interrupted'
  | 'failed'
  | 'closed'

/** A chat session with an agent (Blueprint Sec 4 — session). */
export interface Session {
  id: string
  name: string
  /** Human-readable relative timestamp (e.g. "2m ago"). Kept for backward
   *  compat with mock data; the backend sends `updatedAt` instead. */
  time?: string
  /** ISO timestamp of the last session update, sent by the backend. Preferred
   *  over `time` for sorting and display. */
  updatedAt?: string
  status: SessionStatus
  active?: boolean
  agentId?: string
  modelId?: string
  workspace?: string
}

/** An image (or other file) attached to a prompt or event. The `uri` is
 *  populated by the backend on events that echo back attachments; for
 *  pending uploads the frontend builds the URL from the session + upload id. */
export interface Attachment {
  id: string
  name: string
  mimeType: string
  uri?: string
}

/** Text the daemon added to a user prompt before sending it to the agent. */
export interface InjectedContext {
  name: string
  content: string
}

/** Host-wide limits for automatic workspace/editor path context. */
export interface PromptContextSettings {
  openFileLimit: number
  workspaceFileListLimit: number
}

/**
 * ACP stop reason for the final StreamUpdate of a turn.
 * Mirrors the ACP spec / coder/acp-go-sdk StopReason union. Kept local
 * because the frontend consumes backend-projected events, not raw ACP
 * wire types (architecture: UI never talks to agents directly).
 */
export type StopReason =
  | 'end_turn'
  | 'max_tokens'
  | 'max_turn_requests'
  | 'refusal'
  | 'cancelled'

/**
 * Event types from the event stream (Blueprint Sec 11).
 * The UI renders chat, tool timelines, and permissions from these.
 */
export type EventType =
  | 'PromptSubmitted'
  | 'ResponseStarted'
  | 'StreamUpdate'
  | 'ToolCompleted'
  | 'ToolStarted'
  | 'PlanUpdated'
  | 'PermissionRequested'
  | 'PermissionGranted'
  | 'PermissionDenied'
  | 'ShellCommandStarted'
  | 'ShellOutputStreamed'
  | 'ShellCommandCompleted'
  | 'FileRevisionUpdated'
  | 'FileWritten'
  | 'SessionInterrupted'
  | 'SessionCancelled'
  | 'AgentExited'
  | 'ConnectionRestarted'
  | 'SessionResumed'
  | 'FileChangedOnDisk'
  | 'ModelChanged'
  | 'SessionCreated'
  | 'SessionClosed'
  | 'UsageUpdated'

/** A single event in the immutable event log (Blueprint Sec 11). */
export interface AppEvent {
  /** Monotonic event id assigned by the backend SQLite store. Optional
   *  because mock data and merged stream events may not carry an id. */
  id?: number
  type: EventType
  sessionId: string
  role?: 'user' | 'agent'
  content?: string
  streaming?: boolean
  tool?: string
  target?: string
  summary?: string
  command?: string
  options?: string[]
  requestId?: string
  toolKind?: string
  toolCallId?: string
  thought?: boolean
  exitCode?: number
  workspaceId?: string
  attachments?: Attachment[]
  /** Exact daemon-provided prompt context, retained with the user message so
   * it can be inspected without being mistaken for user-authored text. */
  injectedContext?: InjectedContext[]
  /** ACP stop reason for the final StreamUpdate of a turn. Empty on
   *  intermediate chunks. The frontend surfaces non-normal terminations
   *  subtly. Mirrors the ACP spec StopReason union — kept as a local type
   *  because the frontend talks to our backend, not to ACP directly. */
  stopReason?: StopReason
  /** Tokens currently in context (ACP `usage_update.used`). Set on
   *  `UsageUpdated` events only. */
  tokensUsed?: number
  /** Total context window size in tokens (ACP `usage_update.size`). Set on
   *  `UsageUpdated` events only. */
  tokensSize?: number
  /** Cumulative session cost amount (ACP `usage_update.cost.amount`). Set on
   *  `UsageUpdated` events only when the agent reports cost. */
  costAmount?: number
  /** ISO 4217 currency code for `costAmount` (e.g. "USD"). Set on
   *  `UsageUpdated` events only when the agent reports cost. */
  costCurrency?: string
}

/** Left panel view options (Blueprint Sec 17 — activity bar). */
export type LeftPanel = 'files' | 'search' | 'git'

/** Mobile bottom-nav views (Blueprint Sec 17 — mobile layout). */
export type MobileView = 'explorer' | 'editor' | 'chat'

/** Options for a workspace content search (mirrors Go search.Options). */
export interface SearchOptions {
  /** Regex pattern to search for (required). */
  pattern: string
  /** Case-insensitive matching when true. */
  ignoreCase?: boolean
  /** Cap on the number of results (default 200 on the backend). */
  maxResults?: number
  /** Optional glob restricting file names (e.g. "*.go"). */
  filePattern?: string
  /** Context lines around each match (rg only). */
  contextLines?: number
}

/** A single search match within a file (mirrors Go search.Result). */
export interface SearchResult {
  /** File path relative to the workspace root. */
  path: string
  /** 1-based line number of the match. */
  lineNumber: number
  /** Full text of the matched line. */
  lineContent: string
  /** 0-based byte offset within lineContent where the match begins. */
  matchStart: number
  /** 0-based byte offset within lineContent where the match ends (exclusive). */
  matchEnd: number
}
