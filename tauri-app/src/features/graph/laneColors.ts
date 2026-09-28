/**
 * Resolving the graph's lane colours.
 *
 * The lanes are drawn with CSS custom properties so a theme change repaints
 * without a re-render, but the author initials on top of a filled HEAD node need
 * a decision: black on a light lane, white on a dark one. That decision needs the
 * colour's actual value, so the variables are resolved once per theme.
 */

import { useEffect, useState } from 'react';

import { LANE_COLORS } from '../../styles/themes';

/** The custom property each lane colour is written as. */
const LANE_VARIABLES = [
  '--base-blue',
  '--base-green',
  '--warning-background',
  '--base-red',
  null,
  null,
  null,
  null,
  null,
  null
];

/** Parse `#rgb`, `#rrggbb`, or `rgb(r, g, b)` into channels. */
function parseColor(value: string): [number, number, number] | null {
  const hex = value.trim();
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(hex);
  if (short) {
    return [
      parseInt(short[1]! + short[1]!, 16),
      parseInt(short[2]! + short[2]!, 16),
      parseInt(short[3]! + short[3]!, 16)
    ];
  }
  const long = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (long) {
    return [parseInt(long[1]!, 16), parseInt(long[2]!, 16), parseInt(long[3]!, 16)];
  }
  const rgb = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)/i.exec(hex);
  if (rgb) {
    return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3])];
  }
  return null;
}

/**
 * The WCAG relative luminance of a colour, with the sRGB channels
 * gamma-expanded.
 *
 * Returns null for a colour that cannot be parsed, so an unresolvable variable
 * falls through to the theme foreground rather than being guessed at.
 */
export function relativeLuminance(color: string): number | null {
  const channels = parseColor(color);
  if (!channels) {
    return null;
  }
  const linear = (value: number) => {
    const channel = value / 255;
    return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  };
  const [r, g, b] = channels;
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

/**
 * The colour the author initials take on top of a filled lane.
 *
 * The 0.179 threshold is the reference's, and it is what decides the WCAG
 * contrast boundary in practice: below it the fill counts as dark.
 */
export function initialsTextColor(fill: string, fallback: string): string {
  const luminance = relativeLuminance(fill);
  if (luminance === null) {
    return fallback;
  }
  return luminance > 0.179 ? '#000000' : '#FFFFFF';
}

/** The lane colour behind a colour index, resolved to a real colour. */
function resolveLane(index: number): string | null {
  const entry = LANE_COLORS[index % LANE_COLORS.length];
  if (!entry) {
    return null;
  }
  if (!entry.startsWith('var(')) {
    return entry;
  }
  if (typeof document === 'undefined' || typeof getComputedStyle !== 'function') {
    return null;
  }
  const variable = LANE_VARIABLES[index] ?? entry.slice(4, -1).trim();
  const value = getComputedStyle(document.documentElement).getPropertyValue(variable).trim();
  return value || null;
}

/**
 * The resolved colour of every lane, re-read when the theme changes.
 *
 * `null` for a lane whose colour could not be read, which the caller renders
 * with the theme foreground.
 */
export function useResolvedLaneColors(revision: string | null): (string | null)[] {
  const [colors, setColors] = useState<(string | null)[]>(() =>
    LANE_COLORS.map((_, index) => resolveLane(index))
  );
  useEffect(() => {
    setColors(LANE_COLORS.map((_, index) => resolveLane(index)));
  }, [revision]);
  return colors;
}
