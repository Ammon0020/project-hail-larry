import type { GitGraphNode } from './gitGraphLayout'

export interface GraphVertical {
  lane: number
  y0: number
  y1: number
}

export interface GraphCurve {
  edgeId: string
  fromLane: number
  toLane: number
  y0: number
  y1: number
  dashed: boolean
}

export interface GraphDot {
  lane: number
  y: number
  isMerge: boolean
}

export interface GraphSegments {
  verticals: GraphVertical[]
  curves: GraphCurve[]
  dot: GraphDot
}

export const LANE_WIDTH = 18
export const DOT_Y = 22
export const DOT_RADIUS = 3.5
export const HEAD_DOT_RADIUS = 5
export const MERGE_DOT_RADIUS = 4.5

export function laneX(lane: number): number {
  return lane * LANE_WIDTH + LANE_WIDTH / 2
}

export function graphWidth(laneCount: number): number {
  return Math.max(28, laneCount * LANE_WIDTH + 6)
}

export function buildContinuationVerticals(
  incomingLanes: readonly number[],
  rowHeight: number,
): GraphVertical[] {
  return incomingLanes.map((lane) => ({ lane, y0: 0, y1: rowHeight }))
}

export function buildRowSegments(node: GitGraphNode, rowHeight: number): GraphSegments {
  const verticals = node.incomingLanes.map((lane) => ({
    lane,
    y0: 0,
    y1: lane === node.lane ? DOT_Y : rowHeight,
  }))
  const curves: GraphCurve[] = []
  for (const edge of node.parentEdges) {
    if (edge.visibility === 'visible' && edge.parentLane === node.lane) {
      verticals.push({ lane: node.lane, y0: DOT_Y, y1: rowHeight })
    } else {
      curves.push({
        edgeId: edge.id,
        fromLane: node.lane,
        toLane: edge.parentLane,
        y0: DOT_Y,
        y1: rowHeight,
        dashed: edge.visibility === 'truncated',
      })
    }
  }

  return { verticals, curves, dot: { lane: node.lane, y: DOT_Y, isMerge: node.parentEdges.length > 1 } }
}
