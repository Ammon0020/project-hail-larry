import { describe, expect, it } from 'vitest'
import { layoutGitGraph } from '@/components/git/gitGraphLayout'
import type { LogCommit } from '@/lib/api/git'

/** Build a LogCommit with sensible defaults; only oid/parents matter for layout. */
function commit(
  oid: string,
  parents: string[] = [],
  extra: Partial<LogCommit> = {},
): LogCommit {
  return {
    oid,
    parents,
    message: extra.message ?? `${oid} message`,
    author: extra.author ?? { name: 'n', email: 'e', time: '0' },
    branchLabels: extra.branchLabels ?? [],
    isHead: extra.isHead ?? false,
  }
}

describe('layoutGitGraph', () => {
  it('lays out a linear history on a single lane with offscreen root edge', () => {
    // c3 -> c2 -> c1 -> (root, parent missing from window)
    const commits = [
      commit('c3', ['c2']),
      commit('c2', ['c1']),
      commit('c1', []),
    ]
    const { nodes, laneCount } = layoutGitGraph(commits)

    expect(laneCount).toBe(1)
    expect(nodes.map((n) => n.lane)).toEqual([0, 0, 0])
    // c3 -> c2 (in-window), c2 -> c1 (in-window), c1 has no edges (root).
    expect(nodes[0].edges).toEqual([{ parentIndex: 1, parentLane: 0, offscreen: false }])
    expect(nodes[1].edges).toEqual([{ parentIndex: 2, parentLane: 0, offscreen: false }])
    expect(nodes[2].edges).toEqual([])
  })

  it('marks paginated-out parents as offscreen', () => {
    // Only c2 is in the window; its parent c1 was not fetched.
    const commits = [commit('c2', ['c1'])]
    const { nodes, laneCount } = layoutGitGraph(commits)

    expect(laneCount).toBe(1)
    expect(nodes[0].lane).toBe(0)
    expect(nodes[0].edges).toEqual([{ parentIndex: -1, parentLane: null, offscreen: true }])
  })

  it('branches a side branch onto a second lane and frees it when it ends', () => {
    //   c4 (main)
    //   |
    //   c3 (main) -- merge -- c2' (side, merged)
    //   |___________________/
    // main: c4 -> c3 -> c1
    // side: c2' -> c1
    // c3 is a merge with parents [c1, c2']
    const commits = [
      commit('c4', ['c3']),
      commit('c3', ['c1', "c2'"]),
      commit("c2'", ['c1']),
      commit('c1', []),
    ]
    const { nodes, laneCount } = layoutGitGraph(commits)

    // Two lanes are needed concurrently (during the merge row).
    expect(laneCount).toBe(2)

    // c4 and c3 stay on lane 0 (first-parent line); the side branch c2' is on
    // lane 1; c1 returns to lane 0 after the side branch rejoins.
    const lanes = nodes.map((n) => n.lane)
    expect(lanes[0]).toBe(0) // c4
    expect(lanes[1]).toBe(0) // c3 (merge, first parent)
    expect(lanes[2]).toBe(1) // c2' (side branch)
    expect(lanes[3]).toBe(0) // c1

    // Merge commit c3 has two edges: to c1 (lane 0) and to c2' (lane 1).
    const mergeEdges = nodes[1].edges
    expect(mergeEdges).toHaveLength(2)
    expect(mergeEdges[0]).toEqual({ parentIndex: 3, parentLane: 0, offscreen: false })
    expect(mergeEdges[1]).toEqual({ parentIndex: 2, parentLane: 1, offscreen: false })

    // Side branch c2' connects back to c1 on lane 0.
    expect(nodes[2].edges).toEqual([{ parentIndex: 3, parentLane: 0, offscreen: false }])
  })

  it('is deterministic: same input yields identical lane assignments', () => {
    const commits = [commit('a', ['b']), commit('b', ['c', 'd']), commit('c', []), commit('d', ['c'])]
    const first = layoutGitGraph(commits)
    const second = layoutGitGraph(commits)
    expect(second).toEqual(first)
  })

  it('handles an empty log without crashing', () => {
    const { nodes, laneCount } = layoutGitGraph([])
    expect(nodes).toEqual([])
    expect(laneCount).toBe(0)
  })

  it('keeps octopus (3-parent) merges connected to three distinct lanes', () => {
    // m merges c1, c2, c3 (all visible). First parent stays on m's lane; the
    // other two branch out to free lanes.
    const commits = [
      commit('m', ['c1', 'c2', 'c3']),
      commit('c1', []),
      commit('c2', []),
      commit('c3', []),
    ]
    const { nodes, laneCount } = layoutGitGraph(commits)

    expect(laneCount).toBe(3)
    const merge = nodes[0]
    expect(merge.edges).toHaveLength(3)
    // First parent reuses the merge's lane; others fan out.
    expect(merge.edges[0].parentLane).toBe(merge.lane)
    expect(merge.edges[1].parentLane).not.toBe(merge.lane)
    expect(merge.edges[2].parentLane).not.toBe(merge.lane)
    // All three parents are in-window.
    expect(merge.edges.every((e) => !e.offscreen)).toBe(true)
  })
})
