/**
 * Git endpoints: repo detection, status, stage/unstage, commit, push, init,
 * ignore, and diff.
 */

import { apiFetch, ApiError } from './client'

/** Read-only git repo detection snapshot (GET /api/workspaces/{id}/git).
 *  `repo_detected: false` with null fields when the workspace is not a git
 *  repo; the frontend uses this to show the breadcrumb branch and gate the
 *  git action bar item. */
export interface GitRepoInfo {
  repoDetected: boolean
  headBranch: string | null
  headOid: string | null
  isShallow: boolean
  hasUncommittedChanges: boolean
}

/** A changed path returned by GET /api/workspaces/{id}/git/status. */
export interface FileStatus {
  path: string
  oldPath: string | null
  status: 'added' | 'modified' | 'deleted' | 'renamed' | 'untracked' | 'conflicted'
  staged: boolean
}

/** Current branch, synchronization, and changed-path information for a repository. */
export interface StatusResult {
  headBranch: string | null
  headOid: string | null
  upstream: string | null
  ahead: number
  behind: number
  branches: string[]
  files: FileStatus[]
}

/** Result of GET /api/workspaces/{id}/git/diff?path=<rel>&staged=<bool>
 *  (S-GIT-DIFF-VIEWER). `unified` is a unified-diff string for raw display /
 *  copy; `base` and `head` are the full file contents on each side of the diff
 *  so the merge viewer can render a structured side-by-side or unified view.
 *  `truncated` is true when the backend capped the response at its size limit
 *  (the viewer shows a banner so the user knows the diff is incomplete). */
export interface GitDiffResult {
  unified: string
  base: string
  head: string
  truncated: boolean
}

export function getGitState(workspaceId: string) {
  return apiFetch<GitRepoInfo>(`/workspaces/${workspaceId}/git`)
}

export function getGitStatus(workspaceId: string) {
  return apiFetch<StatusResult>(`/workspaces/${workspaceId}/git/status`)
}

export function gitStage(workspaceId: string, paths: string[], all: boolean) {
  return apiFetch<{ staged: number }>(`/workspaces/${workspaceId}/git/stage`, {
    method: 'POST',
    body: JSON.stringify({ paths, all }),
  })
}

export function gitUnstage(workspaceId: string, paths: string[]) {
  return apiFetch<{ unstaged: number }>(`/workspaces/${workspaceId}/git/unstage`, {
    method: 'POST',
    body: JSON.stringify({ paths }),
  })
}

export async function gitCommit(workspaceId: string, message: string, amend: boolean, headOid: string | null) {
  try {
    return await apiFetch<{ oid: string }>(`/workspaces/${workspaceId}/git/commit`, {
      method: 'POST',
      // Omit If-Match for the initial commit (no HEAD yet); the backend
      // treats a missing precondition as "only allow when HEAD is unborn".
      headers: headOid ? { 'If-Match': headOid } : undefined,
      body: JSON.stringify({ message, amend }),
    })
  } catch (err) {
    if (err instanceof ApiError && err.status === 409) {
      throw new Error(
        'Commit rejected because HEAD changed. Status has been refreshed; review and try again.',
        { cause: err },
      )
    }
    throw err
  }
}

export function gitPush(workspaceId: string, remote: string | null = null, setUpstream = false) {
  return apiFetch<{ ok: true; stderr: string }>(`/workspaces/${workspaceId}/git/push`, {
    method: 'POST',
    body: JSON.stringify({ remote, setUpstream }),
  })
}

export function gitFetch(workspaceId: string, remote: string | null = null) {
  return apiFetch<{ ok: true; stderr: string }>(`/workspaces/${workspaceId}/git/fetch`, {
    method: 'POST',
    body: JSON.stringify({ remote }),
  })
}

export function gitPull(workspaceId: string, remote: string | null = null) {
  return apiFetch<{ ok: true; stderr: string }>(`/workspaces/${workspaceId}/git/pull`, {
    method: 'POST',
    body: JSON.stringify({ remote }),
  })
}

export function gitCheckout(workspaceId: string, branch: string) {
  return apiFetch<{ ok: true; stderr: string }>(`/workspaces/${workspaceId}/git/checkout`, {
    method: 'POST',
    body: JSON.stringify({ branch }),
  })
}

export function gitInit(workspaceId: string) {
  return apiFetch<{ oid: string }>(`/workspaces/${workspaceId}/git/init`, { method: 'POST' })
}

/** POST /api/workspaces/{id}/git/ignore — append patterns to `.gitignore`.
 *  Returns the list of patterns actually added (empty when all were dupes). */
export function gitIgnore(workspaceId: string, patterns: string[]) {
  return apiFetch<{ added: string[] }>(`/workspaces/${workspaceId}/git/ignore`, {
    method: 'POST',
    body: JSON.stringify({ patterns }),
  })
}

/** POST /api/workspaces/{id}/git/discard — restore tracked files to their
 *  index state and delete untracked files. Returns the count of paths
 *  processed. Replaces the fragile readFile→diff→saveFile workaround that
 *  failed on deleted/binary files and revision conflicts. */
export function gitDiscard(workspaceId: string, paths: string[]) {
  return apiFetch<{ discarded: number }>(`/workspaces/${workspaceId}/git/discard`, {
    method: 'POST',
    body: JSON.stringify({ paths }),
  })
}

/** GET /api/workspaces/{id}/git/diff?path=<rel>&staged=<bool> — fetches the
 *  base/head contents and unified diff for a single file. `staged` selects
 *  the index (staged) diff vs. the working-tree (unstaged) diff. */
export function getGitDiff(workspaceId: string, path: string, staged: boolean) {
  return apiFetch<GitDiffResult>(
    `/workspaces/${workspaceId}/git/diff?path=${encodeURIComponent(path)}&staged=${staged}`,
  )
}
