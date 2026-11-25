import { describe, expect, it } from 'vitest';

import {
  mainRemote,
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
  remoteBranches: ['origin/main'],
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
      'set-upstream-remote',
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
    expect(items.slice(0, 4).every((item) => item.disabled)).toBe(true);
    // The settings of the remote are still reachable while a command runs.
    expect(items[4]?.disabled).toBe(false);
  });

  it('refuses a pull while the tree is in conflict', () => {
    const items = remoteGroupItems('origin', options({ pullAvailable: false }));
    expect(items[0]?.disabled).toBe(false);
    expect(items[1]?.disabled).toBe(true);
  });

  it('makes another remote the upstream of the current branch', () => {
    const items = remoteGroupItems(
      'upstream',
      options({
        remotes: ['origin', 'upstream'],
        remoteBranches: ['origin/main', 'upstream/main']
      })
    );
    const item = items.find((entry) => entry.id === 'set-upstream-remote');
    expect(item?.disabled).toBe(false);
    expect(item?.command).toEqual({ kind: 'setUpstream', remote: 'upstream', branch: 'main' });
  });

  it('does not offer an upstream branch the remote does not have', () => {
    const items = remoteGroupItems('upstream', options({ remotes: ['origin', 'upstream'] }));
    const item = items.find((entry) => entry.id === 'set-upstream-remote');
    expect(item?.disabled).toBe(true);
  });

  it('does not offer the upstream the current branch already tracks', () => {
    const item = remoteGroupItems('origin', options()).find(
      (entry) => entry.id === 'set-upstream-remote'
    );
    expect(item?.disabled).toBe(true);
  });
});

describe('remote branch menu', () => {
  it('names the branch it reads and writes', () => {
    const items = remoteBranchItems('origin', 'main', options());
    expect(ids(items)).toEqual([
      'fetch-remote-branch',
      'pull-remote-branch',
      'pull-remote-branch-rebase',
      'push-remote-branch',
      'set-upstream-remote-branch'
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
      'push-remote-branch',
      'set-upstream-remote-branch'
    ]);
  });

  it('offers nothing for a branch that belongs to no remote', () => {
    expect(remoteBranchItems('(other)', 'main', options())).toEqual([]);
  });

  it('refuses a push without a commit to publish', () => {
    const items = remoteBranchItems('origin', 'main', options({ hasHead: false }));
    expect(items[3]?.disabled).toBe(true);
  });

  it('sets a remote branch as the upstream of the current branch', () => {
    const item = remoteBranchItems('origin', 'dev', options()).find(
      (entry) => entry.id === 'set-upstream-remote-branch'
    );
    expect(item?.disabled).toBe(false);
    expect(item?.command).toEqual({ kind: 'setUpstream', remote: 'origin', branch: 'dev' });
  });

  it('does not offer the upstream the current branch already tracks', () => {
    const item = remoteBranchItems('origin', 'main', options()).find(
      (entry) => entry.id === 'set-upstream-remote-branch'
    );
    expect(item?.disabled).toBe(true);
  });

  it('refuses to set an upstream without a checked-out branch', () => {
    const item = remoteBranchItems('origin', 'main', options({ branch: '', upstream: null })).find(
      (entry) => entry.id === 'set-upstream-remote-branch'
    );
    expect(item?.disabled).toBe(true);
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

describe('main remote', () => {
  it('is the remote that owns the upstream', () => {
    expect(mainRemote('origin/main', ['origin', 'upstream'])).toBe('origin');
  });

  it('reads a remote whose name contains a slash as one name', () => {
    expect(mainRemote('foo/bar/dev', ['foo', 'foo/bar'])).toBe('foo/bar');
  });

  it('has no main remote without an upstream', () => {
    expect(mainRemote(null, ['origin'])).toBeNull();
  });

  it('has no main remote when the upstream is a local branch', () => {
    expect(mainRemote('main', ['origin'])).toBeNull();
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
