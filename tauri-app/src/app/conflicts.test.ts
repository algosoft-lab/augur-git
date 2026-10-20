import { describe, expect, it } from 'vitest';

import { conflictKind } from './conflicts';

describe('conflict routing', () => {
  it('reads the plain operations', () => {
    expect(conflictKind('merge')).toBe('merge');
    expect(conflictKind('merge --no-ff')).toBe('merge');
    expect(conflictKind('pull')).toBe('merge');
    expect(conflictKind('rebase')).toBe('rebase');
    expect(conflictKind('pull --rebase')).toBe('rebase');
    expect(conflictKind('stash pop')).toBe('stash');
  });

  it('reads the remote-scoped operations, which name their target', () => {
    expect(conflictKind('pull origin main')).toBe('merge');
    expect(conflictKind('pull --rebase origin main')).toBe('rebase');
  });

  it('leaves the operations that cannot conflict alone', () => {
    expect(conflictKind('fetch origin')).toBeNull();
    expect(conflictKind('push origin main')).toBeNull();
    expect(conflictKind('commit')).toBeNull();
  });
});
