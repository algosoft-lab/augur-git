/**
 * Commit-graph lane drawing.
 *
 * The lane layout itself is computed in Rust and arrives as `GraphRow` data, so
 * this module only turns that data into SVG paths. Geometry follows the
 * reference application: 36px rows, 24px lanes, a 12px left pad, a hollow node
 * for an ordinary commit and a filled one for HEAD, and rounded orthogonal
 * turns between lanes.
 */

/** Row height, matching the row component. */
export const ROW_HEIGHT = 36;
/** Distance between lane centers. */
export const COL_WIDTH = 24;
/** Left padding before the first lane center. */
export const GRAPH_LEFT_PAD = 12;
/** Node diameter. */
export const NODE_RADIUS = 12;
const STROKE_WIDTH = 1.5;
const TURN_RADIUS = 6;

export interface LaneGeometry {
  nodeLane: number;
  laneCount: number;
  colorIndex: number;
  isHead: boolean;
  isMerge: boolean;
  hasIncoming: boolean;
  nodeInputLanes: number[];
  parentLanes: number[];
  /** Color index of each lane entering the row. */
  inputColors: number[];
  /** Color index of each lane leaving the row. */
  outputColors: number[];
}

const LANE_X = (lane: number) => GRAPH_LEFT_PAD + lane * COL_WIDTH + COL_WIDTH / 2;

export interface GraphSvgProps {
  geometry: LaneGeometry;
  laneColors: readonly string[];
  width: number;
}

/**
 * Draw one row.
 *
 * Continuation lanes are painted first so the routes created by this commit
 * appear on top of the pipes they replace.
 */
export function GraphSvg({ geometry, laneColors, width }: GraphSvgProps) {
  const { nodeLane, inputColors, outputColors, nodeInputLanes, parentLanes } = geometry;
  const midY = ROW_HEIGHT / 2;
  const nodeX = LANE_X(nodeLane);
  const color = laneColors[geometry.colorIndex % laneColors.length] ?? "currentColor";

  const paths: { d: string; color: string; width: number }[] = [];

  // Pipes that continue through the row untouched.
  for (let lane = 0; lane < inputColors.length; lane += 1) {
    if (nodeInputLanes.includes(lane)) {
      continue;
    }
    if (outputColors.length <= lane) {
      continue;
    }
    const x = LANE_X(lane);
    paths.push({
      d: `M ${x} 0 L ${x} ${ROW_HEIGHT}`,
      color: laneColors[outputColors[lane]! % laneColors.length] ?? "currentColor",
      width: STROKE_WIDTH,
    });
  }

  // Lanes that converge on this commit's node.
  for (const lane of nodeInputLanes) {
    if (lane === nodeLane) {
      continue;
    }
    const x = LANE_X(lane);
    paths.push({ d: route(x, nodeX, midY), color, width: STROKE_WIDTH });
  }

  // Lanes created by this commit's parents.
  for (const lane of parentLanes) {
    const x = LANE_X(lane);
    const laneColor =
      laneColors[(outputColors[lane] ?? geometry.colorIndex) % laneColors.length] ??
      "currentColor";
    if (lane === nodeLane) {
      paths.push({ d: `M ${nodeX} ${midY} L ${nodeX} ${ROW_HEIGHT}`, color: laneColor, width: STROKE_WIDTH });
    } else {
      paths.push({ d: route(nodeX, x, midY), color: laneColor, width: STROKE_WIDTH });
    }
  }

  return (
    <svg
      className="graph-row__lanes"
      width={width}
      height={ROW_HEIGHT}
      aria-hidden="true"
      focusable="false"
    >
      {paths.map((path, index) => (
        <path
          key={index}
          d={path.d}
          fill="none"
          stroke={path.color}
          strokeWidth={path.width}
          strokeLinecap="round"
        />
      ))}
      {geometry.hasIncoming ? null : null}
      <circle
        cx={nodeX}
        cy={midY}
        r={NODE_RADIUS - STROKE_WIDTH}
        fill="var(--background)"
        stroke={color}
        strokeWidth={STROKE_WIDTH}
      />
      {geometry.isHead ? <circle cx={nodeX} cy={midY} r={4.5} fill={color} /> : null}
    </svg>
  );
}

/** A rounded orthogonal route from `fromX` to `toX` at `midY`. */
function route(fromX: number, toX: number, midY: number): string {
  if (fromX === toX) {
    return `M ${fromX} 0 L ${fromX} ${ROW_HEIGHT}`;
  }
  const direction = toX > fromX ? 1 : -1;
  const startY = midY - TURN_RADIUS * direction * -1;
  const endY = midY + TURN_RADIUS * direction;
  const radius = Math.min(TURN_RADIUS, Math.abs(toX - fromX) / 2);
  return [
    `M ${fromX} 0`,
    `L ${fromX} ${startY}`,
    `Q ${fromX} ${midY} ${fromX + radius * direction} ${midY}`,
    `L ${toX - radius * direction} ${midY}`,
    `Q ${toX} ${midY} ${toX} ${endY}`,
    `L ${toX} ${ROW_HEIGHT}`,
  ].join(" ");
}
