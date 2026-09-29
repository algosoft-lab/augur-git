/**
 * Lane path construction for one graph row.
 *
 * The expectations pin the phantom-edge cases: no path may reach past a node
 * toward the top of the graph when the commit has no incoming lane, and
 * nothing may continue below a node that has no parent beneath it.
 */

import { describe, expect, it } from 'vitest';

import {
  buildRowPaths,
  laneCenterX,
  NODE_RADIUS,
  ROW_HEIGHT,
  TURN_RADIUS,
  type LaneGeometry,
  type LaneRef
} from './GraphSvg';

const lane = (oid: string, colorIndex = 0): LaneRef => ({ oid, colorIndex });
const nodeX = laneCenterX(0);
const sideX = laneCenterX(1);
const midY = ROW_HEIGHT / 2;

function geometry(overrides: Partial<LaneGeometry>): LaneGeometry {
  return {
    nodeLane: 0,
    laneCount: 2,
    colorIndex: 0,
    isHead: false,
    isMerge: false,
    hasIncoming: false,
    nodeInputLanes: [],
    parentLanes: [],
    inputLanes: [],
    outputLanes: [],
    ...overrides
  };
}

describe('buildRowPaths', () => {
  it('a top-row merge without an incoming lane draws no edge above the node', () => {
    const paths = buildRowPaths(
      geometry({
        isMerge: true,
        isHead: true,
        parentLanes: [0, 1],
        outputLanes: [lane('p1'), lane('p2', 1)]
      })
    );

    expect(paths.filter((path) => path.d.startsWith(`M ${nodeX} 0`))).toEqual([]);
    expect(paths).toContainEqual({
      d: `M ${nodeX + NODE_RADIUS} ${midY} C ${nodeX + NODE_RADIUS + TURN_RADIUS} ${midY} ${sideX} ${
        ROW_HEIGHT - TURN_RADIUS
      } ${sideX} ${ROW_HEIGHT}`,
      colorIndex: 1
    });
  });

  it('a root commit with converging lanes draws no edge below the node', () => {
    const paths = buildRowPaths(
      geometry({
        hasIncoming: true,
        nodeInputLanes: [0, 1],
        inputLanes: [lane('root'), lane('root', 1)]
      })
    );

    expect(paths.filter((path) => path.d.endsWith(`L ${nodeX} ${ROW_HEIGHT}`))).toEqual([]);
    expect(paths).toContainEqual({
      d: `M ${sideX} 0 C ${sideX} ${TURN_RADIUS} ${nodeX + NODE_RADIUS + TURN_RADIUS} ${midY} ${
        nodeX + NODE_RADIUS
      } ${midY}`,
      colorIndex: 1
    });
  });

  it('a mid-graph merge keeps its incoming and outgoing connections', () => {
    const paths = buildRowPaths(
      geometry({
        hasIncoming: true,
        nodeInputLanes: [0],
        parentLanes: [0, 1],
        inputLanes: [lane('m')],
        outputLanes: [lane('p1'), lane('p2', 1)]
      })
    );

    expect(paths).toEqual([
      { d: `M ${nodeX} 0 L ${nodeX} ${midY - NODE_RADIUS}`, colorIndex: 0 },
      { d: `M ${nodeX} ${midY + NODE_RADIUS} L ${nodeX} ${ROW_HEIGHT}`, colorIndex: 0 },
      {
        d: `M ${nodeX + NODE_RADIUS} ${midY} C ${nodeX + NODE_RADIUS + TURN_RADIUS} ${midY} ${sideX} ${
          ROW_HEIGHT - TURN_RADIUS
        } ${sideX} ${ROW_HEIGHT}`,
        colorIndex: 1
      }
    ]);
  });

  it('a pass-through lane stays a straight pipe when its column is unchanged', () => {
    const paths = buildRowPaths(
      geometry({
        nodeLane: 1,
        nodeInputLanes: [1],
        parentLanes: [1],
        inputLanes: [lane('x', 3), lane('n')],
        outputLanes: [lane('x', 3), lane('p')]
      })
    );

    expect(paths).toContainEqual({ d: `M ${nodeX} 0 L ${nodeX} ${ROW_HEIGHT}`, colorIndex: 3 });
  });

  it('a lane shifted by compacting duplicates stays connected through the row', () => {
    const paths = buildRowPaths(
      geometry({
        laneCount: 3,
        nodeInputLanes: [0, 1],
        parentLanes: [0],
        inputLanes: [lane('base'), lane('base', 1), lane('child', 2)],
        outputLanes: [lane('p'), lane('child', 2)]
      })
    );

    const farX = laneCenterX(2);
    expect(paths).toContainEqual({
      d: [`M ${farX} 0`, `C ${farX} ${midY} ${sideX} ${midY} ${sideX} ${ROW_HEIGHT}`].join(' '),
      colorIndex: 2
    });
  });

  it('parent edges carry the parent lane color, not the node color', () => {
    const paths = buildRowPaths(
      geometry({
        isMerge: true,
        parentLanes: [0, 1],
        outputLanes: [lane('p1'), lane('p2', 4)]
      })
    );

    const secondParent = paths.find((path) =>
      path.d.startsWith(`M ${nodeX + NODE_RADIUS} ${midY}`)
    );
    expect(secondParent?.colorIndex).toBe(4);
  });

  it('curves merge inputs and parent routes on the left side of the node', () => {
    const leftNodeX = laneCenterX(1);
    const paths = buildRowPaths(
      geometry({
        nodeLane: 1,
        hasIncoming: true,
        nodeInputLanes: [0, 1],
        parentLanes: [0],
        inputLanes: [lane('merge', 3), lane('merge')],
        outputLanes: [lane('parent', 4)]
      })
    );

    expect(paths).toContainEqual({
      d: `M ${nodeX} 0 C ${nodeX} ${TURN_RADIUS} ${leftNodeX - NODE_RADIUS - TURN_RADIUS} ${midY} ${
        leftNodeX - NODE_RADIUS
      } ${midY}`,
      colorIndex: 3
    });
    expect(paths).toContainEqual({
      d: `M ${leftNodeX - NODE_RADIUS} ${midY} C ${leftNodeX - NODE_RADIUS - TURN_RADIUS} ${midY} ${
        nodeX
      } ${ROW_HEIGHT - TURN_RADIUS} ${nodeX} ${ROW_HEIGHT}`,
      colorIndex: 4
    });
  });
});
