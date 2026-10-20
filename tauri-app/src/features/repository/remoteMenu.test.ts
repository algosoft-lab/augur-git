import { describe, expect, it } from 'vitest';

import {
  planPushCurrent,
  pullTarget,
  remoteBranchItems,
  remoteGroupItems,
  type RemoteMenuOptions
} from './remoteMenu';

const options = (overrides: Partial<RemoteMenuOptions> = {}): RemoteMenuOptions => ({
  networkAvailable: true,
  pullAvailable: true,
  branch: 'main',
  upstream: 'origin/main',
  remotes: ['origin'],
  hasHead: true,
  rebase: false,
  ...overrides
});

const ids = (items: { id: string }[]) => items.map((item) => item.id);

describe('remote group menu', () => {
  it('offers the operations that name one remote', () => {
    const items = remoteGroupItems('origin', options());
    expect(ids(items)).toEqual([
      'fetch-remote',
      'pull-remote',
      'push-remote',
      'edit-remote-url',
      'remove-remote',
      'manage-remotes'
    ]);
  });

  it('offers nothing for the group that is not a remote', () => {
    expect(remoteGroupItems('(other)', options())).toEqual([]);
  });

  it('fetches the remote it names', () => {
    const [fetch] = remoteGroupItems('upstream', options());
    expect(fetch?.command).toEqual({
      kind: 'action',
      action: { action: 'fetchRemote', remote: 'upstream' }
    });
  });

  it('pulls the branch the current branch tracks on that remote', () => {
    const [, pull] = remoteGroupItems(
      'upstream',
      options({ upstream: 'upstream/dev', remotes: ['origin', 'upstream'] })
    );
    expect(pull?.command).toEqual({
      kind: 'pull',
      remote: 'upstream',
      branch: 'dev',
      rebase: false
    });
  });

  it('pulls the branch of the same name when the remote is not the tracked one', () => {
    const [, pull] = remoteGroupItems('upstream', options());
    expect(pull?.command).toEqual({
      kind: 'pull',
      remote: 'upstream',
      branch: 'main',
      rebase: false
    });
  });

  it('honours the pull preference', () => {
    const [, pull] = remoteGroupItems('origin', options({ rebase: true }));
    expect(pull?.command).toEqual({
      kind: 'pull',
      remote: 'origin',
      branch: 'main',
      rebase: true
    });
  });

  it('refuses a pull when there is no branch to pull into', () => {
    const [, pull] = remoteGroupItems('origin', options({ branch: '', upstream: null }));
    expect(pull?.disabled).toBe(true);
  });

  it('refuses a push without a branch to publish', () => {
    const [, , push] = remoteGroupItems('origin', options({ branch: '', upstream: null }));
    expect(push?.disabled).toBe(true);
  });

  it('refuses every network operation while the repository is busy', () => {
    const items = remoteGroupItems('origin', options({ networkAvailable: false }));
    expect(items.slice(0, 3).every((item) => item.disabled)).toBe(true);
    // The settings of the remote are still reachable while a command runs.
    expect(items[3]?.disabled).toBe(false);
  });

  it('refuses a pull while the tree is in conflict', () => {
    const items = remoteGroupItems('origin', options({ pullAvailable: false }));
    expect(items[0]?.disabled).toBe(false);
    expect(items[1]?.disabled).toBe(true);
  });
});

describe('remote branch menu', () => {
  it('names the branch it reads and writes', () => {
    const items = remoteBranchItems('origin', 'main', options());
    expect(ids(items)).toEqual([
      'fetch-remote-branch',
      'pull-remote-branch',
      'pull-remote-branch-rebase',
      'push-remote-branch'
    ]);
    expect(items[1]?.command).toEqual({
      kind: 'pull',
      remote: 'origin',
      branch: 'main',
      rebase: false
    });
    expect(items[2]?.command).toEqual({
      kind: 'pull',
      remote: 'origin',
      branch: 'main',
      rebase: true
    });
    expect(items[3]?.command).toEqual({
      kind: 'action',
      action: { action: 'pushTo', remote: 'origin', branch: 'main', force: false }
    });
  });

  it('offers the way of pulling the preferences ask for first', () => {
    const items = remoteBranchItems('origin', 'main', options({ rebase: true }));
    expect(ids(items)).toEqual([
      'fetch-remote-branch',
      'pull-remote-branch-rebase',
      'pull-remote-branch',
      'push-remote-branch'
    ]);
  });

  it('offers nothing for a branch that belongs to no remote', () => {
    expect(remoteBranchItems('(other)', 'main', options())).toEqual([]);
  });

  it('refuses a push without a commit to publish', () => {
    const items = remoteBranchItems('origin', 'main', options({ hasHead: false }));
    expect(items[3]?.disabled).toBe(true);
  });
});

describe('pull target', () => {
  it('prefers the branch the current branch tracks on that remote', () => {
    expect(
      pullTarget('origin', { branch: 'main', upstream: 'origin/dev', remotes: ['origin'] })
    ).toBe('dev');
  });

  it('falls back to the branch of the same name', () => {
    expect(
      pullTarget('upstream', { branch: 'main', upstream: 'origin/main', remotes: ['origin'] })
    ).toBe('main');
  });

  it('has no target without a branch', () => {
    expect(pullTarget('upstream', { branch: '', upstream: null, remotes: [] })).toBeNull();
  });

  it('reads a remote whose name contains a slash as one name', () => {
    expect(
      pullTarget('foo/bar', {
        branch: 'main',
        upstream: 'foo/bar/dev',
        remotes: ['foo', 'foo/bar']
      })
    ).toBe('dev');
  });
});

describe('push plan', () => {
  it('pushes to the branch the current branch tracks on that remote', () => {
    expect(
      planPushCurrent('origin', { branch: 'main', upstream: 'origin/main', remotes: ['origin'] })
    ).toEqual({
      kind: 'push',
      remote: 'origin',
      branch: 'main'
    });
  });

  it('publishes a branch that does not track that remote', () => {
    expect(
      planPushCurrent('upstream', {
        branch: 'main',
        upstream: 'origin/main',
        remotes: ['origin', 'upstream']
      })
    ).toEqual({
      kind: 'publish',
      remote: 'upstream',
      branch: 'main'
    });
  });

  it('has no plan without a branch', () => {
    expect(planPushCurrent('origin', { branch: '', upstream: null, remotes: [] })).toBeNull();
  });
});
