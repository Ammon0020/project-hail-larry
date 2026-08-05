import type { LogCommit } from '@/lib/api/git'

export interface VisibleParentEdge {
  id: string
  parentOid: string
  parentIndex: number
  parentLane: number
  lineageId: number
  visibility: 'visible'
}

export interface TruncatedParentEdge {
  id: string
  parentOid: string
  parentIndex: null
  parentLane: number
  lineageId: number
  visibility: 'truncated'
}

export type ParentEdge = VisibleParentEdge | TruncatedParentEdge

export interface IncomingLane {
  lane: number
  lineageId: number
}

export interface GitGraphNode {
  index: number
  oid: string
  lane: number
  lineageId: number
  parentEdges: ParentEdge[]
  incomingLanes: IncomingLane[]
}

export interface GitGraphLayout {
  nodes: GitGraphNode[]
  laneCount: number
}

/**
 * Lay out a newest-first, topologically ordered commit window.
 *
 * An active lane owns a visible parent that will appear on a later row.
 * Truncated parents deliberately do not own a lane: their dashed edge ends on
 * its child row and cannot safely converge with a later, unrelated stub.
 *
 * Each commit and edge carries a `lineageId` identifying its branch lineage,
 * so colors stay stable when lanes are reused and across pagination appends.
 * The first parent of a commit continues the same lineage (same branch); other
 * parents start their own lineage (side branches). A parent already pre-placed
 * by an earlier child inherits that lane's lineage, so converging side branches
 * draw their convergence edge in the parent (main) branch's color.
 */
export function layoutGitGraph(commits: LogCommit[]): GitGraphLayout {
  const indices = new Map(commits.map((commit, index) => [commit.oid, index]))
  const active: (string | null)[] = []
  // Lineage of the branch currently occupying each active lane. Cleared when
  // the lane is freed so a reused lane gets a fresh lineage.
  const activeLineage: (number | null)[] = []
  let lineageCounter = 0
  const nodes: GitGraphNode[] = []

  const freeLane = (reserved: readonly number[] = []): number => {
    const lane = active.findIndex((oid, index) => oid === null && !reserved.includes(index))
    if (lane >= 0) return lane
    return active.push(null) - 1
  }

  for (const [index, commit] of commits.entries()) {
    const incomingLanes: IncomingLane[] = []
    for (let lane = 0; lane < active.length; lane++) {
      if (active[lane] !== null && activeLineage[lane] !== null) {
        incomingLanes.push({ lane, lineageId: activeLineage[lane] as number })
      }
    }

    let lane = active.indexOf(commit.oid)
    let lineageId: number
    if (lane < 0) {
      lane = freeLane()
      active[lane] = commit.oid
      // Fresh branch lineage for a newly placed commit.
      lineageId = lineageCounter++
      activeLineage[lane] = lineageId
    } else {
      // Pre-placed commit inherits the lineage of the lane it lands on.
      lineageId = activeLineage[lane] as number
      // A shared ancestor can be pre-placed on multiple lanes by different
      // children. Keep the first and free the rest so orphaned lanes don't
      // become phantom through-verticals down to the root.
      for (let k = 0; k < active.length; k++) {
        if (k !== lane && active[k] === commit.oid) {
          active[k] = null
          activeLineage[k] = null
        }
      }
    }

    const truncatedLanes: number[] = []
    const parentEdges = commit.parents.map((parentOid, ordinal): ParentEdge => {
      const parentIndex = indices.get(parentOid)
      if (parentIndex === undefined) {
        const parentLane = ordinal === 0 ? lane : freeLane(truncatedLanes)
        truncatedLanes.push(parentLane)
        // A truncated first parent continues the current branch offscreen;
        // a truncated side parent starts its own (stub) lineage.
        const parentLineage = ordinal === 0 ? lineageId : lineageCounter++
        return {
          id: `${commit.oid}:${parentOid}:${ordinal}`,
          parentOid,
          parentIndex: null,
          parentLane,
          lineageId: parentLineage,
          visibility: 'truncated',
        }
      }

      let parentLane = active.indexOf(parentOid)
      let parentLineage: number
      if (parentLane < 0) {
        parentLane = ordinal === 0 ? lane : freeLane()
        active[parentLane] = parentOid
        if (ordinal === 0) {
          // First parent continues the current commit's branch lineage.
          parentLineage = lineageId
        } else {
          // Side parent starts a new branch lineage.
          parentLineage = lineageCounter++
        }
        activeLineage[parentLane] = parentLineage
      } else {
        // Parent was pre-placed by an earlier child — inherit its lane's
        // lineage so a converging side branch draws in the parent's color.
        parentLineage = activeLineage[parentLane] as number
      }
      return {
        id: `${commit.oid}:${parentOid}:${ordinal}`,
        parentOid,
        parentIndex,
        parentLane,
        lineageId: parentLineage,
        visibility: 'visible',
      }
    })

    // Only a visible parent continues below this row. Root and truncated
    // parent edges end here, so their lanes must be reusable immediately.
    if (!parentEdges.some((edge) => edge.visibility === 'visible' && edge.parentLane === lane)) {
      active[lane] = null
      activeLineage[lane] = null
    }
    nodes.push({ index, oid: commit.oid, lane, lineageId, parentEdges, incomingLanes })
  }

  return { nodes, laneCount: active.length }
}
