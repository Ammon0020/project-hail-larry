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
}

/** A registered AI agent (Blueprint Sec 5 — agent registration). */
export interface Agent {
  id: string
  name: string
  models: AgentModel[]
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
  time: string
  status: SessionStatus
  active?: boolean
  agentId?: string
  modelId?: string
}

/** A paired device (Blueprint Sec 19 — device pairing). */
export interface PairedDevice {
  id: string
  name: string
  icon: string
  pairedAt: string
}

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
}

/** Left panel view options (Blueprint Sec 17 — activity bar). */
export type LeftPanel = 'files' | 'search'

/** Mobile bottom-nav views (Blueprint Sec 17 — mobile layout). */
export type MobileView = 'explorer' | 'editor' | 'chat' | 'settings'
