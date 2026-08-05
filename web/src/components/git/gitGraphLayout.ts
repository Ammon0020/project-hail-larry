import type { LogCommit } from '@/lib/api/git'

export interface VisibleParentEdge {
  id: string
  parentOid: string
  parentIndex: number
  parentLane: number
  visibility: 'visible'
}

export interface TruncatedParentEdge {
  id: string
  parentOid: string
  parentIndex: null
  parentLane: number
  visibility: 'truncated'
}

export type ParentEdge = VisibleParentEdge | TruncatedParentEdge

export interface GitGraphNode {
  index: number
  oid: string
  lane: number
  parentEdges: ParentEdge[]
  incomingLanes: number[]
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
 */
export function layoutGitGraph(commits: LogCommit[]): GitGraphLayout {
  const indices = new Map(commits.map((commit, index) => [commit.oid, index]))
  const active: (string | null)[] = []
  const nodes: GitGraphNode[] = []

  const freeLane = (reserved: readonly number[] = []): number => {
    const lane = active.findIndex((oid, index) => oid === null && !reserved.includes(index))
    if (lane >= 0) return lane
    return active.push(null) - 1
  }

  for (const [index, commit] of commits.entries()) {
    const incomingLanes = active.flatMap((oid, lane) => (oid === null ? [] : [lane]))
    let lane = active.indexOf(commit.oid)
    if (lane < 0) {
      lane = freeLane()
      active[lane] = commit.oid
    } else {
      // A shared ancestor can be pre-placed on multiple lanes by different
      // children. Keep the first and free the rest so orphaned lanes don't
      // become phantom through-verticals down to the root.
      for (let k = 0; k < active.length; k++) {
        if (k !== lane && active[k] === commit.oid) active[k] = null
      }
    }

    const truncatedLanes: number[] = []
    const parentEdges = commit.parents.map((parentOid, ordinal): ParentEdge => {
      const parentIndex = indices.get(parentOid)
      if (parentIndex === undefined) {
        const parentLane = ordinal === 0 ? lane : freeLane(truncatedLanes)
        truncatedLanes.push(parentLane)
        return {
          id: `${commit.oid}:${parentOid}:${ordinal}`,
          parentOid,
          parentIndex: null,
          parentLane,
          visibility: 'truncated',
        }
      }

      let parentLane = active.indexOf(parentOid)
      if (parentLane < 0) {
        parentLane = ordinal === 0 ? lane : freeLane()
        active[parentLane] = parentOid
      }
      return {
        id: `${commit.oid}:${parentOid}:${ordinal}`,
        parentOid,
        parentIndex,
        parentLane,
        visibility: 'visible',
      }
    })

    // Only a visible parent continues below this row. Root and truncated
    // parent edges end here, so their lanes must be reusable immediately.
    if (!parentEdges.some((edge) => edge.visibility === 'visible' && edge.parentLane === lane)) {
      active[lane] = null
    }
    nodes.push({ index, oid: commit.oid, lane, parentEdges, incomingLanes })
  }

  return { nodes, laneCount: active.length }
}
