/**
 * The colour rule for the author initials drawn on a graph node.
 *
 * The threshold and the weighting are the reference's, and a test states the
 * expected answer rather than restating the formula, so a change of either shows
 * up as a failure instead of as a slightly different shade.
 */

import { describe, expect, it } from 'vitest';

import { initialsTextColor, relativeLuminance } from './laneColors';

const FALLBACK = 'var(--foreground)';

describe('relativeLuminance', () => {
  it('reads the three hex forms and the rgb function', () => {
    const expected = relativeLuminance('#2F81F7');
    expect(expected).toBeCloseTo(0.2302, 3);
    expect(relativeLuminance('rgb(47, 129, 247)')).toBeCloseTo(expected!, 6);
    // A short form doubles each digit, so #abc is #aabbcc.
    expect(relativeLuminance('#abc')).toBeCloseTo(relativeLuminance('#aabbcc')!, 6);
  });

  it('anchors pure black at zero and pure white at one', () => {
    expect(relativeLuminance('#000000')).toBe(0);
    expect(relativeLuminance('#ffffff')).toBeCloseTo(1, 6);
  });

  it('is null for a colour it cannot read, so the caller can fall back', () => {
    expect(relativeLuminance('var(--base-blue)')).toBeNull();
    expect(relativeLuminance('')).toBeNull();
  });
});

describe('initialsTextColor', () => {
  it('puts black on a light fill and white on a dark one', () => {
    // A light lane needs dark initials to stay legible.
    expect(initialsTextColor('#e5c890', FALLBACK)).toBe('#000000');
    expect(initialsTextColor('#3FB950', FALLBACK)).toBe('#000000');
    // A dark lane needs light ones.
    expect(initialsTextColor('#2F81F7', FALLBACK)).toBe('#000000');
    expect(initialsTextColor('#1e1e2e', FALLBACK)).toBe('#FFFFFF');
  });

  it('falls back to the theme foreground when the fill is unknown', () => {
    // An unresolvable variable must not be guessed at: unreadable initials are
    // worse than the ordinary foreground.
    expect(initialsTextColor('var(--base-blue)', FALLBACK)).toBe(FALLBACK);
  });
});
