/**
 * The commit-list geometry the viewer computes.
 *
 * The numbers here are deliberately the same as the reference application's, and
 * `relativeTime` mirrors its thresholds exactly, so a test states the expected
 * answer instead of freezing the clock.
 */

import { describe, expect, it } from 'vitest';

import { laneAreaWidth, maxLanes, relativeTime } from './GraphView';
import type { GraphRow } from '../../bridge/types';

/** A translator that renders the key and its arguments, so a test can see both. */
const translate = (key: string, args?: Record<string, string | number>): string =>
  args ? `${key} ${Object.values(args).join(',')}` : key;

function row(laneCount: number): GraphRow {
  return {
    input_lanes: [],
    output_lanes: [],
    parent_lanes: [],
    node_lane: 0,
    lane_count: laneCount,
    is_head: false,
    is_merge: false,
    has_incoming: false,
    node_color: 0,
    node_input_lanes: []
  };
}

describe('lane area width', () => {
  it('is the left pad plus one column per lane, plus the trailing gap', () => {
    // 12 + 24 * lanes + 8, the reference's formula.
    expect(laneAreaWidth(1)).toBe(44);
    expect(laneAreaWidth(3)).toBe(92);
  });

  it('does not reserve a column that has no lane', () => {
    // The widest count already includes the space the last lane needs, so adding
    // one more would push every row 24 pixels to the right of where it belongs.
    expect(laneAreaWidth(2)).toBe(laneAreaWidth(1) + 24);
  });
});

describe('maxLanes', () => {
  it('reports the widest lane count in the layout', () => {
    expect(maxLanes([row(1), row(4), row(2)])).toBe(4);
  });

  it('reports one lane for an empty graph, matching the reference', () => {
    // The reference unwraps to 1 rather than 0, so an empty graph still has a
    // lane area instead of collapsing to nothing.
    expect(maxLanes([])).toBe(1);
  });
});

describe('relativeTime', () => {
  const now = 1_800_000_000;

  it("calls a change from the same minute 'now'", () => {
    expect(relativeTime(now, translate, now)).toBe('rel-now');
  });

  it('counts minutes below an hour', () => {
    expect(relativeTime(now - 5 * 60, translate, now)).toBe('rel-min 5');
  });

  it('counts hours below a day', () => {
    expect(relativeTime(now - 5 * 60 * 60, translate, now)).toBe('rel-hour 5');
  });

  it('counts days below a week', () => {
    expect(relativeTime(now - 3 * 24 * 3600, translate, now)).toBe('rel-day 3');
  });

  it('counts weeks below a month', () => {
    expect(relativeTime(now - 14 * 24 * 3600, translate, now)).toBe('rel-week 2');
  });

  it('counts months below a year', () => {
    expect(relativeTime(now - 90 * 24 * 3600, translate, now)).toBe('rel-month 3');
  });

  it('counts years beyond a year', () => {
    expect(relativeTime(now - 400 * 24 * 3600, translate, now)).toBe('rel-year 1');
  });

  it("treats a timestamp in the future as 'now' rather than a negative age", () => {
    // A clock skewed between commits and display must not read as the future.
    expect(relativeTime(now + 100, translate, now)).toBe('rel-now');
  });
});
