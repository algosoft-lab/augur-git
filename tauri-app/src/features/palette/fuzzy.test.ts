import { describe, expect, it } from 'vitest';

import { fuzzyMatch } from './fuzzy';

describe('fuzzyMatch', () => {
  it('matches case-insensitive subsequences and reports matched positions', () => {
    expect(fuzzyMatch('gtc', 'Git Checkout')).toEqual({
      score: expect.any(Number),
      indices: [0, 2, 4]
    });
  });

  it('prefers contiguous prefixes to scattered matches', () => {
    expect(fuzzyMatch('pull', 'Pull')!.score).toBeGreaterThan(
      fuzzyMatch('pull', 'Perpetually lax')!.score
    );
  });

  it('returns an empty match for an empty query', () => {
    expect(fuzzyMatch('  ', 'anything')).toEqual({ score: 0, indices: [] });
  });

  it('rejects a query that is not a subsequence', () => {
    expect(fuzzyMatch('xyz', 'Repository')).toBeNull();
  });
});
