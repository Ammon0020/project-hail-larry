import { GitDiffViewer } from './GitDiffViewer'

/**
 * Editor-tab wrapper for a fetched agent edit diff. The parent fetches and
 * stores the immutable snapshots so reopening the tab does not issue another
 * request or accidentally compare against a newer file revision.
 */
export function AgentDiffTab({
  path,
  base,
  head,
  truncated,
}: {
  path: string
  base: string
  head: string
  truncated?: boolean
}) {
  return <GitDiffViewer path={path} base={base} head={head} truncated={truncated} />
}
