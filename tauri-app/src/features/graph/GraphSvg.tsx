/**
 * Commit-graph lane drawing.
 *
 * The lane layout itself is computed in Rust and arrives as `GraphRow` data, so
 * this module only turns that data into SVG paths. Rows are 36px tall, lanes
 * are 24px wide, and each commit is marked by a small circle on its lane.
 *
 * Edges that touch the commit node are anchored at the node's edge, the way
 * the reference painter draws them. A full-height route is only correct for a
 * lane that genuinely passes through the row; reusing it for parent or
 * converging edges would paint phantom segments above a merge tip or below a
 * root commit, where no connection exists.
 */

/** Row height, matching the row component. */
export const ROW_HEIGHT = 36;
/** Distance between lane centers. */
export const COL_WIDTH = 24;
/** Left padding before the first lane center. */
export const GRAPH_LEFT_PAD = 12;
/** Commit marker radius. */
export const NODE_RADIUS = 7;
/** Maximum control-point offset for rounded graph transitions. */
export const TURN_RADIUS = 9;
const STROKE_WIDTH = 1.5;

export interface GraphMetrics {
  rowHeight: number;
  laneWidth: number;
  leftPad: number;
  nodeRadius: number;
  turnRadius: number;
  strokeWidth: number;
}

export const DESKTOP_GRAPH_METRICS: GraphMetrics = {
  rowHeight: ROW_HEIGHT,
  laneWidth: COL_WIDTH,
  leftPad: GRAPH_LEFT_PAD,
  nodeRadius: NODE_RADIUS,
  turnRadius: TURN_RADIUS,
  strokeWidth: STROKE_WIDTH
};

export const SIDECAR_GRAPH_METRICS: GraphMetrics = {
  rowHeight: 22,
  laneWidth: 11,
  leftPad: 6,
  nodeRadius: 4,
  turnRadius: 5,
  strokeWidth: 1
};

/** One lane entering or leaving a row, identified by the commit it waits for. */
export interface LaneRef {
  oid: string;
  colorIndex: number;
}

export interface LaneGeometry {
  nodeLane: number;
  laneCount: number;
  colorIndex: number;
  isHead: boolean;
  isMerge: boolean;
  hasIncoming: boolean;
  nodeInputLanes: number[];
  parentLanes: number[];
  /** Lanes entering the row, in input order. */
  inputLanes: LaneRef[];
  /** Lanes leaving the row, in output order. */
  outputLanes: LaneRef[];
}

export const laneCenterX = (lane: number, metrics = DESKTOP_GRAPH_METRICS) =>
  metrics.leftPad + lane * metrics.laneWidth + metrics.laneWidth / 2;

export interface GraphSvgProps {
  geometry: LaneGeometry;
  laneColors: readonly string[];
  width: number;
  metrics?: GraphMetrics;
}

/** One drawable lane segment: an SVG path plus the palette index to stroke it with. */
export interface RowPath {
  d: string;
  colorIndex: number;
}

/**
 * Build every lane path for one row, continuations first so the routes created
 * by this commit are painted on top of the pipes they replace.
 */
export function buildRowPaths(geometry: LaneGeometry, metrics = DESKTOP_GRAPH_METRICS): RowPath[] {
  const midY = metrics.rowHeight / 2;
  const nodeX = laneCenterX(geometry.nodeLane, metrics);
  const paths: RowPath[] = [];

  const nodeInputLanes = new Set(geometry.nodeInputLanes);
  const parentLanes = new Set(geometry.parentLanes);
  const usedOutputs = new Set<number>();

  // Pipes that pass through the row. Input lanes are matched to output lanes
  // by commit rather than by index, so a lane that shifts when duplicate
  // lanes compact stays connected instead of dropping out for a row.
  for (const [fromLane, input] of geometry.inputLanes.entries()) {
    if (nodeInputLanes.has(fromLane)) {
      continue;
    }
    const toLane = geometry.outputLanes.findIndex(
      (output, lane) => !usedOutputs.has(lane) && !parentLanes.has(lane) && output.oid === input.oid
    );
    if (toLane < 0) {
      continue;
    }
    usedOutputs.add(toLane);
    const fromX = laneCenterX(fromLane, metrics);
    const toX = laneCenterX(toLane, metrics);
    paths.push({
      d:
        fromX === toX
          ? `M ${fromX} 0 L ${fromX} ${metrics.rowHeight}`
          : throughRoute(fromX, toX, midY, metrics),
      colorIndex: input.colorIndex
    });
  }

  // The stub entering the node from above, when the lane reaches this commit
  // from the row above rather than from the left. Without it the node floats
  // free of the pipe that leads to it.
  if (geometry.hasIncoming) {
    paths.push({
      d: `M ${nodeX} 0 L ${nodeX} ${midY - metrics.nodeRadius}`,
      colorIndex: geometry.colorIndex
    });
  }

  // Lanes that converge on this commit's node. They stop at the node: below
  // it only a real parent edge may continue.
  for (const lane of geometry.nodeInputLanes) {
    if (lane === geometry.nodeLane) {
      continue;
    }
    paths.push({
      d: routeToNode(laneCenterX(lane, metrics), nodeX, midY, metrics),
      colorIndex: geometry.inputLanes[lane]?.colorIndex ?? geometry.colorIndex
    });
  }

  // Lanes created by this commit's parents.
  for (const lane of geometry.parentLanes) {
    const laneColor = geometry.outputLanes[lane]?.colorIndex ?? geometry.colorIndex;
    if (lane === geometry.nodeLane) {
      // The stub leaving the node downwards. The node's own stroke would stop
      // short of the row below, leaving a visible gap.
      paths.push({
        d: `M ${nodeX} ${midY + metrics.nodeRadius} L ${nodeX} ${metrics.rowHeight}`,
        colorIndex: laneColor
      });
    } else {
      paths.push({
        d: routeFromNode(nodeX, laneCenterX(lane, metrics), midY, metrics),
        colorIndex: laneColor
      });
    }
  }

  return paths;
}

/**
 * Draw one row.
 *
 * HEAD is a filled disc and every other commit a ring, so the tip of the
 * history is findable in a column of nodes without reading a single label.
 */
export function GraphSvg({
  geometry,
  laneColors,
  width,
  metrics = DESKTOP_GRAPH_METRICS
}: GraphSvgProps) {
  const nodeX = laneCenterX(geometry.nodeLane, metrics);
  const color = laneColors[geometry.colorIndex % laneColors.length] ?? 'currentColor';

  return (
    <svg
      className="graph-row__lanes"
      width={width}
      height={metrics.rowHeight}
      aria-hidden="true"
      focusable="false"
    >
      {buildRowPaths(geometry, metrics).map((path, index) => (
        <path
          key={index}
          d={path.d}
          fill="none"
          stroke={laneColors[path.colorIndex % laneColors.length] ?? 'currentColor'}
          strokeWidth={metrics.strokeWidth}
          strokeLinecap="round"
        />
      ))}
      <circle
        cx={nodeX}
        cy={metrics.rowHeight / 2}
        r={metrics.nodeRadius}
        fill={geometry.isHead ? color : 'var(--background)'}
        stroke={color}
        strokeWidth={metrics.strokeWidth}
      />
    </svg>
  );
}

/** A full-height curved route for a lane that shifts sideways through the row. */
function throughRoute(fromX: number, toX: number, midY: number, metrics: GraphMetrics): string {
  return [`M ${fromX} 0`, `C ${fromX} ${midY} ${toX} ${midY} ${toX} ${metrics.rowHeight}`].join(
    ' '
  );
}

/** A route from the row above that terminates at the node's edge. */
function routeToNode(fromX: number, nodeX: number, midY: number, metrics: GraphMetrics): string {
  const direction = nodeX > fromX ? 1 : -1;
  const edgeX = nodeX - direction * metrics.nodeRadius;
  const handle = Math.min(metrics.turnRadius, Math.abs(edgeX - fromX), midY);
  return [
    `M ${fromX} 0`,
    `C ${fromX} ${handle} ${edgeX - direction * handle} ${midY} ${edgeX} ${midY}`
  ].join(' ');
}

/** A route starting at the node's edge and continuing to the row below. */
function routeFromNode(nodeX: number, toX: number, midY: number, metrics: GraphMetrics): string {
  const direction = toX > nodeX ? 1 : -1;
  const edgeX = nodeX + direction * metrics.nodeRadius;
  const handle = Math.min(metrics.turnRadius, Math.abs(toX - edgeX), metrics.rowHeight - midY);
  return [
    `M ${edgeX} ${midY}`,
    `C ${edgeX + direction * handle} ${midY} ${toX} ${metrics.rowHeight - handle} ${toX} ${metrics.rowHeight}`
  ].join(' ');
}
