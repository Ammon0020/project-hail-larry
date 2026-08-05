/**
 * Pure SVG segment builder for the Git history graph.
 *
 * Given a single {@link GitGraphNode} and layout constants, produces plain-data
 * segment descriptors (verticals, curves, dots) that a React row component maps
 * to `<line>` / `<path>` / `<circle>` elements. All geometry is row-local —
 * nothing spans multiple rows — so the segments work with per-row SVGs that use
 * `overflow: visible` and identical lane `x` coordinates. Verticals in adjacent
 * rows line up to form continuous branch lines.
 */

import type { GitGraphLayout, GitGraphNode } from './gitGraphLayout'

export interface GraphVertical {
  lane: number
  y0: number
  y1: number
}

export interface GraphCurve {
  fromLane: number
  toLane: number
  y0: number
  y1: number
  dashed: boolean
}

export interface GraphDot {
  lane: number
  y: number
  isHead: boolean
  isMerge: boolean
}

export interface GraphSegments {
  verticals: GraphVertical[]
  curves: GraphCurve[]
  dot: GraphDot
}

/** Lane geometry constants shared between the helper and the row renderer. */
export const LANE_WIDTH = 18
export const DOT_Y = 22
export const DOT_RADIUS = 3.5
export const HEAD_DOT_RADIUS = 5
export const MERGE_DOT_RADIUS = 4.5

/** Lane `x` pixel for a given lane index (centered in its column). */
export function laneX(lane: number): number {
  return lane * LANE_WIDTH + LANE_WIDTH / 2
}

/** Total graph column width for a given lane count. */
export function graphWidth(laneCount: number): number {
  return Math.max(28, laneCount * LANE_WIDTH + 6)
}

/**
 * Build the SVG segments for a single graph row.
 *
 * Rendering rules:
 *  - **Through-lanes**: lanes in both `lanesAbove` and `lanesBelow` (excluding
 *    the commit's own lane) get a full-height vertical (y=0 → rowHeight).
 *  - **Commit's lane upper**: if the commit's lane is in `lanesAbove`, draw a
 *    vertical from y=0 to the dot (line coming down to the dot).
 *  - **Commit's lane lower**: if the commit's lane is in `lanesBelow`, draw a
 *    vertical from the dot to rowHeight (first-parent continuation downward).
 *  - **New lanes**: lanes in `lanesBelow` but not `lanesAbove` (excluding the
 *    commit's own lane) are merge second-parent lanes initiated by this commit.
 *    Draw a curve from the dot to (lane, rowHeight).
 *  - **Offscreen edges**: dashed curves from the dot going upward off-graph.
 *  - **Dot**: drawn at (lane, DOT_Y); larger for HEAD and merge commits.
 */
export function buildRowSegments(
  node: GitGraphNode,
  rowHeight: number,
): GraphSegments {
  const verticals: GraphVertical[] = []
  const curves: GraphCurve[] = []

  const aboveSet = new Set(node.lanesAbove)
  const belowSet = new Set(node.lanesBelow)

  // Through-lanes: in both above and below, excluding the commit's own lane.
  for (const lane of node.lanesAbove) {
    if (lane === node.lane) continue
    if (belowSet.has(lane)) {
      verticals.push({ lane, y0: 0, y1: rowHeight })
    }
  }

  // Commit's lane upper segment (line coming down to the dot).
  if (aboveSet.has(node.lane)) {
    verticals.push({ lane: node.lane, y0: 0, y1: DOT_Y })
  }

  // Commit's lane lower segment (first-parent continuation).
  if (belowSet.has(node.lane)) {
    verticals.push({ lane: node.lane, y0: DOT_Y, y1: rowHeight })
  }

  // New lanes (merge second+ parents): curve from dot to lane bottom.
  for (const lane of node.lanesBelow) {
    if (lane === node.lane) continue
    if (!aboveSet.has(lane)) {
      curves.push({
        fromLane: node.lane,
        toLane: lane,
        y0: DOT_Y,
        y1: rowHeight,
        dashed: false,
      })
    }
  }

  // Offscreen parent edges: dashed stubs going upward from the dot.
  for (const edge of node.edges) {
    if (edge.offscreen) {
      curves.push({
        fromLane: node.lane,
        toLane: node.lane,
        y0: DOT_Y,
        y1: 0,
        dashed: true,
      })
    }
  }

  const isMerge = node.edges.filter((e) => !e.offscreen).length > 1
  const dot: GraphDot = {
    lane: node.lane,
    y: DOT_Y,
    isHead: false,
    isMerge,
  }

  return { verticals, curves, dot }
}

/** Convenience: build segments for all nodes in a layout. */
export function buildAllSegments(
  layout: GitGraphLayout,
  rowHeight: number,
): GraphSegments[] {
  return layout.nodes.map((node) => buildRowSegments(node, rowHeight))
}
