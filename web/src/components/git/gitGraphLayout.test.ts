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
    expect(nodes[0].parentEdges).toHaveLength(1)
    expect(nodes[0].parentEdges[0]).toMatchObject({ parentIndex: 1, parentLane: 0, visibility: 'visible' })
    expect(nodes[1].parentEdges).toHaveLength(1)
    expect(nodes[1].parentEdges[0]).toMatchObject({ parentIndex: 2, parentLane: 0, visibility: 'visible' })
    expect(nodes[2].parentEdges).toEqual([])
  })

  it('marks paginated-out parents as truncated with an assigned lane', () => {
    // Only c2 is in the window; its parent c1 was not fetched.
    const commits = [commit('c2', ['c1'])]
    const { nodes, laneCount } = layoutGitGraph(commits)

    expect(laneCount).toBe(1)
    expect(nodes[0].lane).toBe(0)
    expect(nodes[0].parentEdges).toHaveLength(1)
    expect(nodes[0].parentEdges[0]).toMatchObject({
      parentOid: 'c1',
      parentIndex: null,
      parentLane: 0,
      visibility: 'truncated',
    })
  })

  it('branches a side branch onto a second lane and frees it when it ends', () => {
    //   c4 (main)
    //   c3 (merge: c1 + c2')
    //   c2' (side) -> c1
    //   c1 (root)
    const commits = [
      commit('c4', ['c3']),
      commit('c3', ['c1', "c2'"]),
      commit("c2'", ['c1']),
      commit('c1', []),
    ]
    const { nodes, laneCount } = layoutGitGraph(commits)

    expect(laneCount).toBe(2)
    const lanes = nodes.map((n) => n.lane)
    expect(lanes[0]).toBe(0) // c4
    expect(lanes[1]).toBe(0) // c3 (merge, first parent)
    expect(lanes[2]).toBe(1) // c2' (side branch)
    expect(lanes[3]).toBe(0) // c1

    // Merge commit c3 has two edges: to c1 (lane 0) and to c2' (lane 1).
    const mergeEdges = nodes[1].parentEdges
    expect(mergeEdges).toHaveLength(2)
    expect(mergeEdges[0]).toMatchObject({ parentIndex: 3, parentLane: 0, visibility: 'visible' })
    expect(mergeEdges[1]).toMatchObject({ parentIndex: 2, parentLane: 1, visibility: 'visible' })

    // Side branch c2' connects back to c1 on lane 0 — the convergence edge.
    expect(nodes[2].parentEdges).toHaveLength(1)
    expect(nodes[2].parentEdges[0]).toMatchObject({ parentIndex: 3, parentLane: 0, visibility: 'visible' })
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
    const commits = [
      commit('m', ['c1', 'c2', 'c3']),
      commit('c1', []),
      commit('c2', []),
      commit('c3', []),
    ]
    const { nodes, laneCount } = layoutGitGraph(commits)

    expect(laneCount).toBe(3)
    const merge = nodes[0]
    expect(merge.parentEdges).toHaveLength(3)
    expect(merge.parentEdges[0].parentLane).toBe(merge.lane)
    expect(merge.parentEdges[1].parentLane).not.toBe(merge.lane)
    expect(merge.parentEdges[2].parentLane).not.toBe(merge.lane)
    expect(merge.parentEdges.every((e) => e.visibility === 'visible')).toBe(true)
  })

  describe('incomingLanes', () => {
    it('reports no incoming lanes for the first row', () => {
      const { nodes } = layoutGitGraph([commit('c1', [])])
      expect(nodes[0].incomingLanes).toEqual([])
    })

    it('shows the commit lane as incoming for linear history after the first row', () => {
      const commits = [commit('c3', ['c2']), commit('c2', ['c1']), commit('c1', [])]
      const { nodes } = layoutGitGraph(commits)
      // c3: no incoming. c2: lane 0 incoming (pre-placed by c3). c1: lane 0 incoming.
      expect(nodes[0].incomingLanes).toEqual([])
      expect(nodes[1].incomingLanes).toEqual([0])
      expect(nodes[2].incomingLanes).toEqual([0])
    })

    it('shows two incoming lanes during a merge with a side branch', () => {
      const commits = [
        commit('c4', ['c3']),
        commit('c3', ['c1', "c2'"]),
        commit("c2'", ['c1']),
        commit('c1', []),
      ]
      const { nodes } = layoutGitGraph(commits)
      // c3 merge row: lane 0 incoming (from c4's first-parent pre-placement).
      expect(nodes[1].incomingLanes).toContain(0)
      // c2' row: both lanes incoming (lane 0 for c1, lane 1 for c2' itself).
      expect(nodes[2].incomingLanes).toContain(0)
      expect(nodes[2].incomingLanes).toContain(1)
    })
  })

  describe('convergence edges (the regression guard)', () => {
    it('side branch commit has a visible edge to the main lane', () => {
      // C → B(merge) → A, B → D → A
      const commits = [
        commit('C', ['B']),
        commit('B', ['A', 'D']),
        commit('D', ['A']),
        commit('A', []),
      ]
      const { nodes } = layoutGitGraph(commits)

      // D is on lane 1, its parent A is on lane 0 (pre-placed by merge B).
      const dNode = nodes[2]
      expect(dNode.lane).toBe(1)
      expect(dNode.parentEdges).toHaveLength(1)
      const edge = dNode.parentEdges[0]
      expect(edge.visibility).toBe('visible')
      expect(edge.parentLane).toBe(0) // convergence to main lane
      expect(edge.parentIndex).toBe(3) // A is at row 3
    })

    it('keeps a shared ancestor on the first child’s pre-placed lane', () => {
      // The current branch reaches A first (rows 0–1). The main line appears
      // later (rows 2–3), so its edge must converge to A's already-selected
      // lane rather than pre-place A on lane 1.
      const commits = [
        commit('current-tip', ['current']),
        commit('current', ['A']),
        commit('main-tip', ['main']),
        commit('main', ['A']),
        commit('A', []),
      ]
      const { nodes, laneCount } = layoutGitGraph(commits)

      expect(laneCount).toBe(2)
      expect(nodes.map((node) => node.lane)).toEqual([0, 0, 1, 1, 0])

      // main is on lane 1, but its first-parent edge curves into A on lane 0.
      expect(nodes[3].parentEdges).toMatchObject([
        { parentOid: 'A', parentIndex: 4, parentLane: 0, visibility: 'visible' },
      ])
      // A must not inherit a duplicate lane-1 through-vertical.
      expect(nodes[4].incomingLanes).toEqual([0])
    })

    it('layout contract: parent lane matches the parent node\'s lane', () => {
      const commits = [
        commit('C', ['B']),
        commit('B', ['A', 'D']),
        commit('D', ['A']),
        commit('A', []),
      ]
      const { nodes } = layoutGitGraph(commits)
      for (const node of nodes) {
        for (const edge of node.parentEdges) {
          if (edge.visibility === 'visible') {
            expect(nodes[edge.parentIndex].lane).toBe(edge.parentLane)
          }
        }
      }
    })
  })

  describe('truncated edges', () => {
    it('does not carry a truncated side-parent lane into later rows', () => {
      // c2's second parent d is outside the window. Its dashed stub ends on
      // c2's row; c1 must not receive a phantom lane-1 through-vertical.
      const commits = [
        commit('c3', ['c2']),
        commit('c2', ['c1', 'd']),
        commit('c1', []),
      ]
      const { nodes } = layoutGitGraph(commits)

      expect(nodes[1].parentEdges[1]).toMatchObject({
        parentOid: 'd',
        parentLane: 1,
        visibility: 'truncated',
      })
      expect(nodes[2].incomingLanes).toEqual([0])
    })

    it('assigns distinct lanes to distinct truncated parents', () => {
      // Merge with two parents, both outside the window.
      const commits = [commit('m', ['a', 'b'])]
      const { nodes, laneCount } = layoutGitGraph(commits)

      expect(laneCount).toBe(2)
      const edges = nodes[0].parentEdges
      expect(edges).toHaveLength(2)
      expect(edges[0].parentLane).toBe(0) // first parent on merge's lane
      expect(edges[1].parentLane).toBe(1) // second parent on a free lane
      expect(edges.every((e) => e.visibility === 'truncated')).toBe(true)
    })

    it('does not converge separate truncated stubs onto an active lane', () => {
      // The two "offscreen-shared" edges end on their own rows. Reusing the
      // first stub's lane for the second would join it to main-root.
      const commits = [
        commit('shared-child-1', ['offscreen-shared']),
        commit('main-tip', ['main-root']),
        commit('shared-child-2', ['offscreen-shared']),
        commit('main-root', []),
      ]
      const { nodes } = layoutGitGraph(commits)

      expect(nodes[2].lane).toBe(1)
      expect(nodes[2].incomingLanes).toEqual([0])
      expect(nodes[2].parentEdges[0]).toMatchObject({
        parentOid: 'offscreen-shared',
        parentLane: 1,
        visibility: 'truncated',
      })
    })
  })

  it('keeps the actual main merge and side branch on their assigned lanes', () => {
    // First 18 rows from `git log --all --format='%H %P %s'` in this repo.
    const commits = [
      commit('2d5d96e', ['18d367e']),
      commit('18d367e', ['1bfbe5e']),
      commit('1bfbe5e', ['64b04a0']),
      commit('64b04a0', ['1a31a48']),
      commit('1a31a48', ['0bd7952']),
      commit('0bd7952', ['e9bfb00']),
      commit('e9bfb00', ['a39565a']),
      commit('a39565a', ['444d026']),
      commit('444d026', ['d1fe918']),
      commit('d1fe918', ['f856504']),
      commit('f856504', ['b6687e1']),
      commit('b6687e1', ['b25b357']),
      commit('b25b357', ['c7e8c19']),
      commit('c7e8c19', ['cb1475a']),
      commit('cb1475a', ['dfb60ba']),
      commit('dfb60ba', ['9dfebdf', 'f4612ff']),
      commit('f4612ff', ['c339b9d']),
      commit('c339b9d', ['c7a54b7']),
    ]
    const { nodes } = layoutGitGraph(commits)

    expect(nodes[15].lane).toBe(0)
    expect(nodes[15].parentEdges).toMatchObject([
      { parentOid: '9dfebdf', parentLane: 0, visibility: 'truncated' },
      { parentOid: 'f4612ff', parentLane: 1, visibility: 'visible' },
    ])
    expect(nodes[16].lane).toBe(1)
    expect(nodes[16].incomingLanes).toEqual([1])
    expect(nodes[17].lane).toBe(1)
    expect(nodes[17].incomingLanes).toEqual([1])
  })
})
