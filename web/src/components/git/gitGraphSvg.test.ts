import { describe, expect, it } from 'vitest'
import { layoutGitGraph } from '@/components/git/gitGraphLayout'
import { buildRowSegments, DOT_Y, laneX, LANE_WIDTH } from '@/components/git/gitGraphSvg'
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
    // c3 -> c2 -> c1: at the c2 row, lane 0 passes through (above and below).
    const { nodes } = layoutGitGraph([commit('c3', ['c2']), commit('c2', ['c1']), commit('c1', [])])
    const seg = buildRowSegments(nodes[1], ROW_HEIGHT)
    // Lane 0 is in both above and below, so it gets a full-height vertical.
    // But it's the commit's own lane, so it's split: upper (0→DOT_Y) and lower (DOT_Y→44).
    const upper = seg.verticals.find((v) => v.lane === 0 && v.y0 === 0)
    const lower = seg.verticals.find((v) => v.lane === 0 && v.y1 === ROW_HEIGHT)
    expect(upper).toBeDefined()
    expect(lower).toBeDefined()
    expect(upper!.y1).toBe(DOT_Y)
    expect(lower!.y0).toBe(DOT_Y)
  })

  it('produces a curve for a merge second parent', () => {
    // c3 merges c1 and c2'. c2' is on lane 1.
    const commits = [
      commit('c3', ['c1', "c2'"]),
      commit("c2'", ['c1']),
      commit('c1', []),
    ]
    const { nodes } = layoutGitGraph(commits)
    const mergeSeg = buildRowSegments(nodes[0], ROW_HEIGHT)
    // The merge has a curve to lane 1 (the second parent's lane).
    const curve = mergeSeg.curves.find((c) => c.toLane === 1 && !c.dashed)
    expect(curve).toBeDefined()
    expect(curve!.fromLane).toBe(0) // merge is on lane 0
    expect(curve!.y0).toBe(DOT_Y)
    expect(curve!.y1).toBe(ROW_HEIGHT)
  })

  it('produces a dashed curve for offscreen parents', () => {
    const { nodes } = layoutGitGraph([commit('c2', ['c1'])])
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    const dashed = seg.curves.find((c) => c.dashed)
    expect(dashed).toBeDefined()
    expect(dashed!.y0).toBe(DOT_Y)
    expect(dashed!.y1).toBe(0)
  })

  it('marks merge commits with isMerge=true', () => {
    const commits = [commit('m', ['a', 'b']), commit('a', []), commit('b', [])]
    const { nodes } = layoutGitGraph(commits)
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    expect(seg.dot.isMerge).toBe(true)
  })

  it('does not mark single-parent commits as merges', () => {
    const commits = [commit('c2', ['c1']), commit('c1', [])]
    const { nodes } = layoutGitGraph(commits)
    const seg = buildRowSegments(nodes[0], ROW_HEIGHT)
    expect(seg.dot.isMerge).toBe(false)
  })

  it('laneX centers each lane in its column', () => {
    expect(laneX(0)).toBe(LANE_WIDTH / 2)
    expect(laneX(1)).toBe(LANE_WIDTH + LANE_WIDTH / 2)
    expect(laneX(2)).toBe(2 * LANE_WIDTH + LANE_WIDTH / 2)
  })
})
