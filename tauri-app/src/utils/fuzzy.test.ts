import { describe, expect, it } from 'vitest';

import { fuzzyScore, normalize, subsequence } from './fuzzy';

describe('fuzzy matching', () => {
  it('normalizes case, whitespace, underscores, and dashes', () => {
    expect(normalize('Fix_Login - NOW')).toBe('fixloginnow');
  });

  it('matches query characters in order', () => {
    expect(subsequence(normalize('Fix Login'), normalize('fxlgn'))).toBe(true);
    expect(subsequence(normalize('Fix Login'), normalize('ngolxfi'))).toBe(false);
  });

  it('ranks prefixes and consecutive matches above looser subsequences', () => {
    expect(fuzzyScore('font', 'Font size')).toBeGreaterThan(fuzzyScore('font', 'UI font size')!);
    expect(fuzzyScore('fxlgn', 'Fix Login')).toBeGreaterThan(0);
  });

  it('returns null for missing or separator-only queries', () => {
    expect(fuzzyScore('fontx', 'Font size')).toBeNull();
    expect(fuzzyScore(' _- ', 'Font size')).toBeNull();
  });
});
