import { describe, expect, it } from 'vitest';

import {
  applyRepoEvent,
  codeFor,
  emptyRepo,
  groupFiles,
  isConflicted,
  isStaged,
  isUntracked
} from '../app/repoState';
import type { FileStatus } from '../bridge/types';

function file(index: string, worktree: string, path = 'src/main.rs'): FileStatus {
  return { index, worktree, path, old_path: null };
}

describe('porcelain status classification', () => {
  it('recognizes a staged change', () => {
    expect(isStaged(file('M', ' '))).toBe(true);
    expect(isStaged(file('A', 'M'))).toBe(true);
    expect(isStaged(file(' ', 'M'))).toBe(false);
    expect(isStaged(file('?', '?'))).toBe(false);
  });

  it('recognizes an untracked file', () => {
    expect(isUntracked(file('?', '?'))).toBe(true);
    expect(isUntracked(file('?', 'M'))).toBe(false);
  });

  it('hides untracked files when the preference is disabled', () => {
    const files = [
      file(' ', 'M', 'tracked.rs'),
      file('?', '?', 'new.rs'),
      file('U', 'U', 'merge.rs')
    ];
    expect(groupFiles(files, false).unstaged.map((entry) => entry.path)).toEqual([
      'tracked.rs',
      'merge.rs'
    ]);
    expect(groupFiles(files, true).unstaged.map((entry) => entry.path)).toEqual([
      'tracked.rs',
      'new.rs',
      'merge.rs'
    ]);
  });

  it('recognizes every unmerged spelling', () => {
    for (const [index, worktree] of [
      ['U', ' '],
      [' ', 'U'],
      ['D', 'D'],
      ['A', 'A']
    ] as const) {
      expect(isConflicted(file(index, worktree)), `${index}${worktree}`).toBe(true);
      // A conflicted entry is never staged or unstaged: it belongs to its own
      // group in the changes panel.
      expect(isStaged(file(index, worktree))).toBe(false);
    }
  });

  it('picks the character that applies to the requested side', () => {
    const both = file('M', 'M');
    expect(codeFor(both, true)).toBe('M');
    expect(codeFor(both, false)).toBe('M');

    const stagedOnly = file('A', ' ');
    expect(codeFor(stagedOnly, true)).toBe('A');
    // The unstaged side has nothing, so the aggregate code is used.
    expect(codeFor(stagedOnly, false)).toBe('A');

    const worktreeOnly = file(' ', 'D');
    expect(codeFor(worktreeOnly, false)).toBe('D');
    expect(codeFor(worktreeOnly, true)).toBe('D');
  });
});

describe('repository snapshot events', () => {
  const apply = (
    state: ReturnType<typeof emptyRepo>,
    event: Parameters<typeof applyRepoEvent>[1]
  ) =>
    applyRepoEvent(
      state,
      event,
      (key) => key,
      (_key, detail) => detail
    );

  it('drops statistics that arrive for an older status snapshot', () => {
    let state = emptyRepo(1, '/repo', { kind: 'local' });
    state = apply(state, {
      type: 'status',
      snapshotId: 2,
      branch: 'main',
      head: 'abc',
      upstream: null,
      ahead: 0,
      behind: 0,
      files: [file('?', '?', 'new.txt')],
      branches: []
    });

    const afterLateStats = apply(state, {
      type: 'diffStats',
      snapshotId: 1,
      report: {
        staged: { status: 'ready', stats: { added: 10, deleted: 0 }, reason: null },
        unstaged: { status: 'ready', stats: { added: 20, deleted: 0 }, reason: null },
        untracked: { status: 'ready', stats: { added: 30, deleted: 0 }, reason: null }
      }
    });
    expect(afterLateStats).toBe(state);
    expect(afterLateStats.diffStatsReport.untracked.status).toBe('pending');
  });

  it('keeps history available when the initial working-tree status fails', () => {
    const initial = emptyRepo(1, '/repo', { kind: 'local' });
    const state = apply(initial, {
      type: 'statusError',
      error: { key: 'err-lfs-worktree-unavailable', detail: 'Git LFS is missing' }
    });
    expect(state.status).toBe('ready');
    expect(state.statusStale).toBe(true);
    expect(state.snapshotId).toBeNull();
    expect(state.errorMessage).toBe('Git LFS is missing');

    const recovered = apply(state, {
      type: 'status',
      snapshotId: 1,
      branch: 'main',
      head: 'abc',
      upstream: null,
      ahead: 0,
      behind: 0,
      files: [],
      branches: []
    });
    expect(recovered.statusStale).toBe(false);
    expect(recovered.errorMessage).toBeNull();
  });

  it('keeps the last complete working-tree snapshot when refresh fails', () => {
    const files = [file('?', '?', 'new.txt')];
    let state = emptyRepo(1, '/repo', { kind: 'local' });
    state = apply(state, {
      type: 'status',
      snapshotId: 4,
      branch: 'main',
      head: 'abc',
      upstream: null,
      ahead: 0,
      behind: 0,
      files,
      branches: []
    });
    state = apply(state, { type: 'statusError', error: { key: 'err', detail: 'failed' } });

    expect(state.status).toBe('ready');
    expect(state.statusStale).toBe(true);
    expect(state.files).toBe(files);
    expect(state.errorMessage).toBe('failed');
  });
});
