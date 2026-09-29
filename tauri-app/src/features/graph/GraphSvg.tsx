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
/** How far a lane's curve travels vertically while turning. */
export const TURN_RADIUS = 6;
const STROKE_WIDTH = 1.5;

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

export const laneCenterX = (lane: number) => GRAPH_LEFT_PAD + lane * COL_WIDTH + COL_WIDTH / 2;

export interface GraphSvgProps {
  geometry: LaneGeometry;
  laneColors: readonly string[];
  width: number;
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
export function buildRowPaths(geometry: LaneGeometry): RowPath[] {
  const midY = ROW_HEIGHT / 2;
  const nodeX = laneCenterX(geometry.nodeLane);
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
    const fromX = laneCenterX(fromLane);
    const toX = laneCenterX(toLane);
    paths.push({
      d: fromX === toX ? `M ${fromX} 0 L ${fromX} ${ROW_HEIGHT}` : throughRoute(fromX, toX, midY),
      colorIndex: input.colorIndex
    });
  }

  // The stub entering the node from above, when the lane reaches this commit
  // from the row above rather than from the left. Without it the node floats
  // free of the pipe that leads to it.
  if (geometry.hasIncoming) {
    paths.push({
      d: `M ${nodeX} 0 L ${nodeX} ${midY - NODE_RADIUS}`,
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
      d: routeToNode(laneCenterX(lane), nodeX, midY),
      colorIndex: geometry.colorIndex
    });
  }

  // Lanes created by this commit's parents.
  for (const lane of geometry.parentLanes) {
    const laneColor = geometry.outputLanes[lane]?.colorIndex ?? geometry.colorIndex;
    if (lane === geometry.nodeLane) {
      // The stub leaving the node downwards. The node's own stroke would stop
      // short of the row below, leaving a visible gap.
      paths.push({
        d: `M ${nodeX} ${midY + NODE_RADIUS} L ${nodeX} ${ROW_HEIGHT}`,
        colorIndex: laneColor
      });
    } else {
      paths.push({
        d: routeFromNode(nodeX, laneCenterX(lane), midY),
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
export function GraphSvg({ geometry, laneColors, width }: GraphSvgProps) {
  const nodeX = laneCenterX(geometry.nodeLane);
  const color = laneColors[geometry.colorIndex % laneColors.length] ?? 'currentColor';

  return (
    <svg
      className="graph-row__lanes"
      width={width}
      height={ROW_HEIGHT}
      aria-hidden="true"
      focusable="false"
    >
      {buildRowPaths(geometry).map((path, index) => (
        <path
          key={index}
          d={path.d}
          fill="none"
          stroke={laneColors[path.colorIndex % laneColors.length] ?? 'currentColor'}
          strokeWidth={STROKE_WIDTH}
          strokeLinecap="round"
        />
      ))}
      <circle
        cx={nodeX}
        cy={ROW_HEIGHT / 2}
        r={NODE_RADIUS}
        fill={geometry.isHead ? color : 'var(--background)'}
        stroke={color}
        strokeWidth={STROKE_WIDTH}
      />
    </svg>
  );
}

/** A full-height route for a lane that shifts sideways partway down the row. */
function throughRoute(fromX: number, toX: number, midY: number): string {
  const direction = toX > fromX ? 1 : -1;
  const radius = Math.min(TURN_RADIUS, Math.abs(toX - fromX) / 2);
  return [
    `M ${fromX} 0`,
    `L ${fromX} ${midY - radius}`,
    `Q ${fromX} ${midY} ${fromX + radius * direction} ${midY}`,
    `L ${toX - radius * direction} ${midY}`,
    `Q ${toX} ${midY} ${toX} ${midY + radius}`,
    `L ${toX} ${ROW_HEIGHT}`
  ].join(' ');
}

/** A route from the row above that terminates at the node's edge. */
function routeToNode(fromX: number, nodeX: number, midY: number): string {
  const direction = nodeX > fromX ? 1 : -1;
  const radius = Math.min(TURN_RADIUS, Math.abs(nodeX - fromX) / 2);
  return [
    `M ${fromX} 0`,
    `L ${fromX} ${midY - radius}`,
    `Q ${fromX} ${midY} ${fromX + radius * direction} ${midY}`,
    `L ${nodeX - direction * NODE_RADIUS} ${midY}`
  ].join(' ');
}

/** A route starting at the node's edge and continuing to the row below. */
function routeFromNode(nodeX: number, toX: number, midY: number): string {
  const direction = toX > nodeX ? 1 : -1;
  const radius = Math.min(TURN_RADIUS, Math.abs(toX - nodeX) / 2);
  return [
    `M ${nodeX + direction * NODE_RADIUS} ${midY}`,
    `L ${toX - radius * direction} ${midY}`,
    `Q ${toX} ${midY} ${toX} ${midY + radius}`,
    `L ${toX} ${ROW_HEIGHT}`
  ].join(' ');
}
