/**
 * Pure layout utility for the Git history graph.
 *
 * Turns a `LogCommit[]` (newest-first, as returned by `git log`) into lane
 * assignments and parent-edge metadata for SVG rendering. The algorithm mirrors
 * the classic `gitk`/`tig` lane layout:
 *
 *  - Each commit occupies one lane (column). A child pre-places its parents on
 *    lanes so the parent commit, once reached, reuses that lane — this keeps
 *    branches visually continuous.
 *  - The first parent of a commit reuses the commit's own lane (straight line
 *    for linear history and the first-parent side of a merge); additional
 *    parents branch out to the first free lane.
 *  - Lanes are freed once the commit sitting on them has been drawn and no
 *    parent was placed on top of it, so columns compact downward.
 *  - Parents outside the visible window (paginated out) produce `truncated`
 *    edges with an assigned lane, so the renderer draws a dashed stub going
 *    downward toward older history.
 *
 * **Edge-driven model:** `parentEdges` is the sole source of truth for outgoing
 * graph segments. The SVG helper renders exactly one segment per edge. Lane
 * occupancy (`incomingLanes`) is used only for through-verticals — lines that
 * pass through a row without a commit dot.
 *
 * The function is deterministic: identical input order yields identical lane
 * numbers, so React can memoize the result and SVG keys stay stable across
 * re-renders / pagination appends.
 */

import type { LogCommit } from '@/lib/api/git'

/** A parent edge that is visible within the current commit window. */
export interface VisibleParentEdge {
  /** Stable ID for React keys and future hover/selection: `${childOid}:${parentOid}:${ordinal}`. */
  id: string
  parentOid: string
  /** Row index of the parent in the input array. */
  parentIndex: number
  /** Lane the parent commit occupies (or will occupy when reached). */
  parentLane: number
  visibility: 'visible'
}

/** A parent edge whose target is outside the visible window (paginated out). */
export interface TruncatedParentEdge {
  id: string
  parentOid: string
  parentIndex: null
  /** Lane assigned for the stub. Distinct truncated parents get distinct lanes
   *  so a truncated merge shows separate dashed paths. */
  parentLane: number
  visibility: 'truncated'
}

export type ParentEdge = VisibleParentEdge | TruncatedParentEdge

export interface GitGraphNode {
  /** Position of the commit in the input array (row in the graph). */
  index: number
  oid: string
  /** Column the commit's dot is drawn in. */
  lane: number
  /** One entry per parent in `LogCommit.parents`, in order. The SVG helper
   *  renders exactly one outgoing segment per edge. */
  parentEdges: ParentEdge[]
  /** Lane indices occupied coming *into* this row (pre-placed by children or
   *  carried forward from above). The SVG draws through-verticals for these,
   *  excluding the commit's own lane (which gets an upper segment to the dot). */
  incomingLanes: number[]
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
 *                become `truncated` edges with assigned lanes.
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
  const parentEdges: ParentEdge[][] = Array.from({ length: commits.length }, () => [])
  const incomingLanes: number[][] = Array.from({ length: commits.length }, () => [])

  const findFreeLane = (): number => {
    for (let k = 0; k < lanes.length; k++) {
      if (lanes[k] === null) return k
    }
    lanes.push(null)
    return lanes.length - 1
  }

  /** Snapshot of currently occupied lane indices (non-null entries). */
  const occupiedLanes = (): number[] => {
    const result: number[] = []
    for (let k = 0; k < lanes.length; k++) {
      if (lanes[k] !== null) result.push(k)
    }
    return result
  }

  for (let i = 0; i < commits.length; i++) {
    const commit = commits[i]

    // Snapshot lanes occupied *before* this commit's dot is drawn — these are
    // the lines coming from above (pre-placed by children or carried forward).
    incomingLanes[i] = occupiedLanes()

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
      const edgeId = `${commit.oid}:${parentOid}:${p}`

      if (parentIndex === undefined) {
        // Parent outside the visible window: assign a lane for the dashed stub
        // so truncated merges show distinct paths. Check if the same parent oid
        // was already placed by another child (lane reuse for convergence).
        let truncLane = lanes.indexOf(parentOid)
        if (truncLane < 0) {
          // First parent reuses the commit's lane (straight line down); subsequent
          // parents branch out to a free lane — same rule as visible parents.
          if (p === 0) {
            truncLane = lane
          } else {
            truncLane = findFreeLane()
          }
          lanes[truncLane] = parentOid
          if (truncLane === lane) placedOnCurrent = true
        }
        parentEdges[i].push({
          id: edgeId,
          parentOid,
          parentIndex: null,
          parentLane: truncLane,
          visibility: 'truncated',
        })
        continue
      }

      let pLane = lanes.indexOf(parentOid)
      if (pLane < 0) {
        // First parent continues on the current lane (straight line / merge
        // first-parent); subsequent parents branch out to a free lane.
        if (p === 0) {
          pLane = lane
        } else {
          pLane = findFreeLane()
        }
        lanes[pLane] = parentOid
        if (pLane === lane) placedOnCurrent = true
      }
      parentEdges[i].push({
        id: edgeId,
        parentOid,
        parentIndex,
        parentLane: pLane,
        visibility: 'visible',
      })
    }

    // Free this commit's lane unless a parent was placed on top of it (in which
    // case the lane is now owned by that upcoming parent).
    if (!placedOnCurrent) lanes[lane] = null
  }

  const nodes: GitGraphNode[] = commits.map((commit, i) => ({
    index: i,
    oid: commit.oid,
    lane: nodeLane[i],
    parentEdges: parentEdges[i],
    incomingLanes: incomingLanes[i],
  }))

  // `lanes.length` is the high-water mark: we only grow the array when every
  // lane is occupied, so it equals the maximum concurrent lane count.
  return { nodes, laneCount: lanes.length }
}
