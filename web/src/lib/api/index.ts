/**
 * API client for the Local Agent Interface Go daemon.
 * All endpoints are relative to the same origin (served by the Go server).
 *
 * This barrel re-exports the domain modules so existing imports from
 * '@/lib/api' keep working unchanged, and assembles the `api` object from
 * the domain modules to preserve `api.method()` calls.
 */

// Re-export types from @/types so existing callers keep working.
export type { AppEvent, Agent, FileNode, Session, SearchOptions, SearchResult } from '@/types'

// Shared client infrastructure
export { getDeviceCredential, previewFileUrl, ApiError } from './client'

// Domain types
export type { WorkspaceInfo } from './workspaces'
export type { GitRepoInfo, FileStatus, StatusResult, GitDiffResult } from './git'
export type { SessionHistoryCapabilities, EditorSelectionInfo, UploadResult } from './sessions'
export type { PermissionOptionInfo, PendingPermission, DeviceCredential, PairingSession } from './permissions'
export type { McpServerStatus } from './mcp'
export type { ProviderInfo } from './providers'
export type { ProfileEntry, ProfileConfig } from './profiles'

// Domain functions (standalone exports)
export { listProviders, setProvider, disableProvider, UnsupportedProvidersError } from './providers'
export { getProfiles, putProfiles, setSessionProfile } from './profiles'

// Assemble the api object from domain modules — preserves `api.method()` calls.
import * as workspaces from './workspaces'
import * as git from './git'
import * as sessions from './sessions'
import * as permissions from './permissions'
import * as mcp from './mcp'

export const api = {
  ...workspaces,
  ...git,
  ...sessions,
  ...permissions,
  ...mcp,
}

// Thin re-exports so existing `import { getMcpConfig } from '@/lib/api'` works.
// Prefer `api.getMcpConfig()` for new code.
export const getMcpConfig = mcp.getMcpConfig
export const putMcpConfig = mcp.putMcpConfig
export const patchMcpServer = mcp.patchMcpServer
export const getMcpStatus = mcp.getMcpStatus
export const getPromptContextSettings = sessions.getPromptContextSettings
export const putPromptContextSettings = sessions.putPromptContextSettings
