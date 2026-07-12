/**
 * Type definitions for the Local Agent Interface.
 * These shapes match what the Go daemon sends over WebSocket
 * and what ACP agents return. See Blueprint sections 4-11.
 */

/** A file or folder node in the workspace file tree (Blueprint Sec 13). */
export interface FileTreeNode {
  name: string
  type: 'folder' | 'file'
  path?: string
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
  /** Tab content kind. 'file' (default) renders CodeMirror; 'settings' renders
   *  the settings panel. Settings tabs use a synthetic path like 'settings'
   *  and are not persisted to localStorage. */
  kind?: 'file' | 'settings'
  /** True when the file is binary (image, executable, archive, etc.) and
   *  cannot be edited as text. The editor renders a placeholder or image
   *  preview instead of a CodeMirror instance. */
  isBinary?: boolean
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
}

/** Session lifecycle states (Blueprint Sec 10). */
export type SessionStatus =
  | 'created'
  | 'starting'
  | 'running'
  | 'waiting_permission'
  | 'interrupted'
  | 'completed'
  | 'failed'
  | 'archived'

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
  /** ACP stop reason for the final StreamUpdate of a turn. Empty on
   *  intermediate chunks. The frontend surfaces non-normal terminations
   *  subtly. Mirrors the ACP spec StopReason union — kept as a local type
   *  because the frontend talks to our backend, not to ACP directly. */
  stopReason?: StopReason
}

/** Left panel view options (Blueprint Sec 17 — activity bar). */
export type LeftPanel = 'files' | 'search'

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
