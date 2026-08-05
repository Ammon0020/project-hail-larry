/**
 * Pure SVG segment builder for the Git history graph.
 *
 * Given a single {@link GitGraphNode} and layout constants, produces plain-data
 * segment descriptors (verticals, curves, dots) that a React row component maps
 * to `<line>` / `<path>` / `<circle>` elements. All geometry is row-local —
 * nothing spans multiple rows — so the segments work with per-row SVGs that use
 * `overflow: visible` and identical lane `x` coordinates. Verticals in adjacent
 * rows line up to form continuous branch lines.
 *
 * **Edge-driven rendering:** Every parent edge in `node.parentEdges` produces
 * exactly one outgoing segment (line or curve). Lane occupancy
 * (`node.incomingLanes`) is used only for through-verticals — lines passing
 * through the row without a commit dot.
 */

import type { GitGraphLayout, GitGraphNode } from './gitGraphLayout'

export interface GraphVertical {
  lane: number
  y0: number
  y1: number
}

export interface GraphCurve {
  /** Stable edge ID from the layout, for React keys and future hover/selection. */
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
 *  - **Through-lanes**: lanes in `incomingLanes` (excluding the commit's own
 *    lane) get a full-height vertical (y=0 → rowHeight).
 *  - **Commit's lane upper**: if the commit's lane is in `incomingLanes`, draw
 *    a vertical from y=0 to the dot (line coming down to the dot).
 *  - **Parent edges**: for every edge in `node.parentEdges`:
 *    - same lane (first-parent continuation) → solid line from dot to rowHeight;
 *    - different lane → solid Bézier curve from dot to (parentLane, rowHeight);
 *    - `truncated` visibility → same geometry but dashed (going downward).
 *  - **Dot**: drawn at (lane, DOT_Y); larger for merge commits.
 */
export function buildRowSegments(node: GitGraphNode, rowHeight: number): GraphSegments {
  const verticals: GraphVertical[] = []
  const curves: GraphCurve[] = []
  const incomingSet = new Set(node.incomingLanes)

  // Through-lanes: incoming lanes excluding the commit's own lane.
  for (const lane of node.incomingLanes) {
    if (lane === node.lane) continue
    verticals.push({ lane, y0: 0, y1: rowHeight })
  }

  // Commit's lane upper segment (line coming down to the dot).
  if (incomingSet.has(node.lane)) {
    verticals.push({ lane: node.lane, y0: 0, y1: DOT_Y })
  }

  // One outgoing segment per parent edge — the core invariant.
  for (const edge of node.parentEdges) {
    const isTruncated = edge.visibility === 'truncated'
    const targetLane = edge.parentLane

    if (targetLane === node.lane && !isTruncated) {
      // Same-lane visible edge: solid line from dot down (first-parent continuation).
      verticals.push({ lane: node.lane, y0: DOT_Y, y1: rowHeight })
    } else {
      // Cross-lane or truncated: Bézier curve from dot to target lane at row bottom.
      curves.push({
        edgeId: edge.id,
        fromLane: node.lane,
        toLane: targetLane,
        y0: DOT_Y,
        y1: rowHeight,
        dashed: isTruncated,
      })
    }
  }

  // A merge is any commit with more than one parent (including truncated ones).
  const isMerge = node.parentEdges.length > 1
  const dot: GraphDot = {
    lane: node.lane,
    y: DOT_Y,
    isMerge,
  }

  return { verticals, curves, dot }
}

/** Convenience: build segments for all nodes in a layout. */
export function buildAllSegments(layout: GitGraphLayout, rowHeight: number): GraphSegments[] {
  return layout.nodes.map((node) => buildRowSegments(node, rowHeight))
}
