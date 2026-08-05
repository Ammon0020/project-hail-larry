import { describe, expect, it } from 'vitest'
import { layoutGitGraph } from '@/components/git/gitGraphLayout'
import {
  buildContinuationVerticals,
  buildRowSegments,
  DOT_Y,
  laneX,
  LANE_WIDTH,
} from '@/components/git/gitGraphSvg'
import type { LogCommit } from '@/lib/api/git'

function commit(oid: string, parents: string[] = []): LogCommit {
  return {
    oid,
    parents,
    message: `${oid} message`,
    author: { name: 'n', email: 'e', time: '0' },
    branchLabels: [],
    isHead: false,
  }
}

const ROW_HEIGHT = 44

describe('buildRowSegments', () => {
  it('produces a single dot and no verticals for a lone root commit', () => {
    const { nodes } = layoutGitGraph([commit('c1', [])])
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    expect(seg.verticals).toEqual([])
    expect(seg.curves).toEqual([])
    expect(seg.dot.lane).toBe(0)
    expect(seg.dot.y).toBe(DOT_Y)
    expect(seg.dot.isMerge).toBe(false)
  })

  it('draws a through-lane vertical for a lane passing through a row', () => {
    // c3 -> c2 -> c1: at the c2 row, lane 0 passes through (incoming and outgoing).
    const { nodes } = layoutGitGraph([commit('c3', ['c2']), commit('c2', ['c1']), commit('c1', [])])
    const seg = buildRowSegments(nodes[1], ROW_HEIGHT)
    // Lane 0 is incoming, so it gets an upper vertical (0→DOT_Y).
    // The first-parent edge (c2→c1, same lane) produces a lower vertical (DOT_Y→44).
    const upper = seg.verticals.find((v) => v.lane === 0 && v.y0 === 0)
    const lower = seg.verticals.find((v) => v.lane === 0 && v.y1 === ROW_HEIGHT)
    expect(upper).toBeDefined()
    expect(lower).toBeDefined()
    expect(upper!.y1).toBe(DOT_Y)
    expect(lower!.y0).toBe(DOT_Y)
  })

  it('produces a curve for a merge second parent (new lane)', () => {
    // C → B(merge: A + D), D → A
    const commits = [
      commit('C', ['B']),
      commit('B', ['A', 'D']),
      commit('D', ['A']),
      commit('A', []),
    ]
    const { nodes } = layoutGitGraph(commits)
    const mergeSeg = buildRowSegments(nodes[1], ROW_HEIGHT)
    // The merge has a curve to lane 1 (D's lane).
    const curve = mergeSeg.curves.find((c) => c.toLane === 1 && !c.dashed)
    expect(curve).toBeDefined()
    expect(curve!.fromLane).toBe(0)
    expect(curve!.y0).toBe(DOT_Y)
    expect(curve!.y1).toBe(ROW_HEIGHT)
  })

  it('produces a convergence curve for the side branch back to the main lane', () => {
    // The regression test: D (lane 1) → A (lane 0) must produce a curve.
    const commits = [
      commit('C', ['B']),
      commit('B', ['A', 'D']),
      commit('D', ['A']),
      commit('A', []),
    ]
    const { nodes } = layoutGitGraph(commits)
    const dSeg = buildRowSegments(nodes[2], ROW_HEIGHT)
    // D is on lane 1, its parent A is on lane 0. This must produce a curve.
    const convergenceCurve = dSeg.curves.find((c) => c.fromLane === 1 && c.toLane === 0 && !c.dashed)
    expect(convergenceCurve).toBeDefined()
    expect(convergenceCurve!.y0).toBe(DOT_Y)
    expect(convergenceCurve!.y1).toBe(ROW_HEIGHT)
  })

  it('produces a dashed downward stub for truncated parents', () => {
    // c2's parent c1 is outside the window.
    const { nodes } = layoutGitGraph([commit('c2', ['c1'])])
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    const dashed = seg.curves.find((c) => c.dashed)
    expect(dashed).toBeDefined()
    // Truncated stubs go DOWNWARD (toward older history below the window).
    expect(dashed!.y0).toBe(DOT_Y)
    expect(dashed!.y1).toBe(ROW_HEIGHT)
  })

  it('does not render a through-vertical below a truncated side-parent stub', () => {
    const commits = [
      commit('c3', ['c2']),
      commit('c2', ['c1', 'd']),
      commit('c1', []),
    ]
    const { nodes } = layoutGitGraph(commits)
    const c1Segments = buildRowSegments(nodes[2], ROW_HEIGHT)

    expect(c1Segments.verticals).not.toContainEqual({ lane: 1, y0: 0, y1: ROW_HEIGHT })
  })

  it('keeps a truncated stub on its own lane when an unrelated lane passes through', () => {
    const commits = [
      commit('shared-child-1', ['offscreen-shared']),
      commit('main-tip', ['main-root']),
      commit('shared-child-2', ['offscreen-shared']),
      commit('main-root', []),
    ]
    const { nodes } = layoutGitGraph(commits)
    const segments = buildRowSegments(nodes[2], ROW_HEIGHT)

    expect(segments.verticals).toContainEqual({ lane: 0, y0: 0, y1: ROW_HEIGHT })
    expect(segments.curves).toContainEqual(expect.objectContaining({
      fromLane: 1,
      toLane: 1,
      dashed: true,
    }))
    expect(segments.curves).not.toContainEqual(expect.objectContaining({
      fromLane: 1,
      toLane: 0,
      dashed: true,
    }))
  })

  it('bridges only active incoming lanes across expanded commit details', () => {
    const commits = [
      commit('c3', ['c2']),
      commit('c2', ['c1', 'd']),
      commit('c1', []),
    ]
    const { nodes } = layoutGitGraph(commits)

    expect(buildContinuationVerticals(nodes[2].incomingLanes, 160)).toEqual([
      { lane: 0, y0: 0, y1: 160 },
    ])
  })

  it('produces no outgoing segment for a root commit (no parents)', () => {
    const { nodes } = layoutGitGraph([commit('c1', [])])
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    expect(seg.curves).toEqual([])
    // No lower vertical either (no first-parent edge).
    const lowerVertical = seg.verticals.find((v) => v.y0 === DOT_Y)
    expect(lowerVertical).toBeUndefined()
  })

  it('marks merge commits with isMerge=true', () => {
    const commits = [commit('m', ['a', 'b']), commit('a', []), commit('b', [])]
    const { nodes } = layoutGitGraph(commits)
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    expect(seg.dot.isMerge).toBe(true)
  })

  it('marks single-parent commits as non-merge', () => {
    const commits = [commit('c2', ['c1']), commit('c1', [])]
    const { nodes } = layoutGitGraph(commits)
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    expect(seg.dot.isMerge).toBe(false)
  })

  it('marks a commit with truncated parents as a merge', () => {
    // A merge with both parents outside the window is still a merge.
    const commits = [commit('m', ['a', 'b'])]
    const { nodes } = layoutGitGraph(commits)
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    expect(seg.dot.isMerge).toBe(true)
  })

  it('invariant: every parent edge produces exactly one outgoing segment', () => {
    const commits = [
      commit('C', ['B']),
      commit('B', ['A', 'D']),
      commit('D', ['A']),
      commit('A', []),
    ]
    const { nodes } = layoutGitGraph(commits)
    for (const node of nodes) {
      const seg = buildRowSegments(node, ROW_HEIGHT)
      const outgoingSegments = seg.curves.length +
        seg.verticals.filter((v) => v.y0 === DOT_Y).length
      expect(outgoingSegments).toBe(node.parentEdges.length)
    }
  })

  it('produces distinct dashed curves for a truncated merge', () => {
    const commits = [commit('m', ['a', 'b'])]
    const { nodes } = layoutGitGraph(commits)
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    // Two truncated parents → two dashed curves on distinct lanes.
    const dashed = seg.curves.filter((c) => c.dashed)
    expect(dashed).toHaveLength(2)
    expect(dashed[0].toLane).not.toBe(dashed[1].toLane)
  })

  it('laneX centers each lane in its column', () => {
    expect(laneX(0)).toBe(LANE_WIDTH / 2)
    expect(laneX(1)).toBe(LANE_WIDTH + LANE_WIDTH / 2)
    expect(laneX(2)).toBe(2 * LANE_WIDTH + LANE_WIDTH / 2)
  })
})
