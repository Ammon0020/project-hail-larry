/**
 * Pure layout utility for the Git history graph.
 *
 * Turns a `LogCommit[]` (newest-first, as returned by `git log`) into a set of
 * lane/column assignments plus parent-edge metadata suitable for rendering an
 * SVG graph. The algorithm mirrors the classic `gitk`/`tig` lane layout:
 *
 *  - Each commit occupies one lane (column). A child pre-places its parents on
 *    lanes so the parent commit, once reached, reuses that lane — this keeps
 *    branches visually continuous.
 *  - The first parent of a commit reuses the commit's own lane (straight line
 *    for linear history and the first-parent side of a merge); additional
 *    parents branch out to the first free lane.
 *  - Lanes are freed once the commit sitting on them has been drawn and no
 *    parent was placed on top of it, so columns compact downward.
 *  - Parents that are not in the visible window (paginated out, or a true
 *    root reached via `--max-count`) produce an `offscreen` edge so the view
 *    can render a stub running off the top of the graph.
 *
 * The function is deterministic: identical input order yields identical lane
 * numbers, so React can memoize the result and SVG keys stay stable across
 * re-renders / pagination appends.
 */

import type { LogCommit } from '@/lib/api/git'

export interface GitGraphEdge {
  /** Index into the input `commits` array of the parent, or -1 when the parent
   *  is missing from the visible window. */
  parentIndex: number
  /** Lane of the parent commit when present, otherwise `null`. */
  parentLane: number | null
  /** True when the parent is outside the visible window (pagination / root). */
  offscreen: boolean
}

export interface GitGraphNode {
  /** Position of the commit in the input array (row in the graph). */
  index: number
  oid: string
  /** Column the commit's dot is drawn in. */
  lane: number
  /** One entry per parent in `LogCommit.parents`, in order. */
  edges: GitGraphEdge[]
}

export interface GitGraphLayout {
  nodes: GitGraphNode[]
  /** Total number of lanes (columns) needed — the high-water mark of
   *  concurrently occupied lanes. Use this to size the SVG horizontally. */
  laneCount: number
}

/**
 * Lays out a window of git log commits for SVG rendering.
 *
 * @param commits Newest-first log entries (as returned by `getGitLog`).
 *                Pagination windows are fine: parents missing from the array
 *                become `offscreen` edges rather than errors.
 */
export function layoutGitGraph(commits: LogCommit[]): GitGraphLayout {
  // oid -> row index, for resolving parent references within the window.
  const oidToIndex = new Map<string, number>()
  for (let i = 0; i < commits.length; i++) oidToIndex.set(commits[i].oid, i)

  // lanes[k] = oid currently expected at lane k. A lane holds either the oid
  // of a commit that was just drawn (transiently, until freed) or a parent
  // pre-placed by a child so the parent reuses this lane when reached.
  // `null` marks a free, reusable lane.
  const lanes: (string | null)[] = []
  const nodeLane = new Array<number>(commits.length).fill(-1)
  const edges: GitGraphEdge[][] = Array.from({ length: commits.length }, () => [])

  const findFreeLane = (): number => {
    for (let k = 0; k < lanes.length; k++) {
      if (lanes[k] === null) return k
    }
    lanes.push(null)
    return lanes.length - 1
  }

  for (let i = 0; i < commits.length; i++) {
    const commit = commits[i]

    // Locate or assign this commit's lane. A child may have pre-placed it.
    let lane = lanes.indexOf(commit.oid)
    if (lane < 0) {
      lane = findFreeLane()
      lanes[lane] = commit.oid
    }
    nodeLane[i] = lane

    // Place each parent on a lane and record the edge.
    let placedOnCurrent = false
    for (let p = 0; p < commit.parents.length; p++) {
      const parentOid = commit.parents[p]
      const parentIndex = oidToIndex.get(parentOid)

      if (parentIndex === undefined) {
        // Parent outside the visible window: edge runs off the top.
        edges[i].push({ parentIndex: -1, parentLane: null, offscreen: true })
        continue
      }

      let parentLane = lanes.indexOf(parentOid)
      if (parentLane < 0) {
        // First parent continues on the current lane (straight line / merge
        // first-parent); subsequent parents branch out to a free lane.
        if (p === 0) {
          parentLane = lane
        } else {
          parentLane = findFreeLane()
        }
        lanes[parentLane] = parentOid
        if (parentLane === lane) placedOnCurrent = true
      }
      edges[i].push({ parentIndex, parentLane, offscreen: false })
    }

    // Free this commit's lane unless a parent was placed on top of it (in which
    // case the lane is now owned by that upcoming parent).
    if (!placedOnCurrent) lanes[lane] = null
  }

  const nodes: GitGraphNode[] = commits.map((commit, i) => ({
    index: i,
    oid: commit.oid,
    lane: nodeLane[i],
    edges: edges[i],
  }))

  // `lanes.length` is the high-water mark: we only grow the array when every
  // lane is occupied, so it equals the maximum concurrent lane count.
  return { nodes, laneCount: lanes.length }
}
