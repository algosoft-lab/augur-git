/**
 * The network operations a remote offers.
 *
 * The sidebar groups remote-tracking branches under their remote, and both the
 * group header and the branches inside it offer the operations that name one
 * remote: fetch it, pull from it, push to it. The builders here decide what
 * those operations target and when they are available, so the menu stays a
 * description of the repository rather than a sequence of side effects.
 *
 * The builders are pure. The caller runs the action the builder names.
 */

import type { GitAction } from '../../bridge/types';
import type { IconName } from '../../components/Icon';
import { OTHER_REMOTE_GROUP } from './remoteGroups';

/** What a menu entry does when it is chosen. */
export type RemoteMenuCommand =
  /** Run a Git action. */
  | { kind: 'action'; action: GitAction }
  /**
   * Pull one remote branch into the current branch.
   *
   * A pull that rebases runs a preflight probe first, so the command names the
   * target rather than the action.
   */
  | { kind: 'pull'; remote: string; branch: string; rebase: boolean }
  /**
   * Push the current branch to a remote.
   *
   * The target is resolved when the command runs: a branch that already
   * tracks a branch on that remote is pushed there, and one that does not is
   * published with the upstream prompt.
   */
  | { kind: 'pushCurrent'; remote: string }
  /** Open the remote settings on one remote. */
  | { kind: 'editRemote'; remote: string }
  /** Ask before removing one remote. */
  | { kind: 'removeRemote'; remote: string }
  /** Open the remote settings. */
  | { kind: 'manageRemotes' };

export interface RemoteMenuItem {
  id: string;
  /** The catalog key of the label. */
  labelKey: string;
  /** The arguments the label is rendered with. */
  labelArgs?: Record<string, string>;
  icon: IconName;
  disabled: boolean;
  separatorBefore?: boolean;
  command: RemoteMenuCommand;
}

export interface RemoteMenuOptions {
  /** Whether a network command may run: nothing is busy and the state is fresh. */
  networkAvailable: boolean;
  /** Whether the working tree accepts a pull: it holds no unresolved merge. */
  pullAvailable: boolean;
  /** The checked-out branch, empty when there is none. */
  branch: string;
  /** The upstream of the checked-out branch, such as `origin/main`. */
  upstream: string | null;
  /** Every configured remote, so an upstream is split by the longest match. */
  remotes: string[];
  /** Whether the repository has a commit to push. */
  hasHead: boolean;
  /** Whether a pull rebases, from the view preferences. */
  rebase: boolean;
}

/**
 * Split an upstream short name into the remote it belongs to and its branch.
 *
 * The longest matching remote wins, so a remote whose name contains a slash
 * is not mistaken for a shorter one.
 */
export function splitUpstream(
  upstream: string,
  remotes: string[]
): { remote: string; branch: string } | null {
  let best: { remote: string; branch: string } | null = null;
  for (const remote of remotes) {
    if (!upstream.startsWith(`${remote}/`)) {
      continue;
    }
    if (!best || remote.length > best.remote.length) {
      best = { remote, branch: upstream.slice(remote.length + 1) };
    }
  }
  if (best) {
    return best;
  }
  const separator = upstream.indexOf('/');
  return separator > 0
    ? { remote: upstream.slice(0, separator), branch: upstream.slice(separator + 1) }
    : null;
}

/** The branch of a remote that the current branch tracks, if it tracks one. */
function trackedBranch(
  remote: string,
  options: Pick<RemoteMenuOptions, 'upstream' | 'remotes'>
): string | null {
  if (!options.upstream) {
    return null;
  }
  const split = splitUpstream(options.upstream, options.remotes);
  return split && split.remote === remote ? split.branch : null;
}

/**
 * The branch of a remote that a pull of the current branch would read.
 *
 * The branch the current branch tracks on that remote is the one that
 * belongs to it; without one, the branch of the same name is the one the
 * remote is expected to have.
 */
export function pullTarget(
  remote: string,
  options: Pick<RemoteMenuOptions, 'branch' | 'upstream' | 'remotes'>
): string | null {
  return trackedBranch(remote, options) ?? (options.branch.length > 0 ? options.branch : null);
}

/**
 * What a push of the current branch to a remote does.
 *
 * A branch that already tracks a branch on that remote is pushed there. One
 * that does not is published, which the interface confirms first because it
 * also sets the upstream.
 */
export type PushCurrentPlan =
  | { kind: 'push'; remote: string; branch: string }
  | { kind: 'publish'; remote: string; branch: string };

export function planPushCurrent(
  remote: string,
  options: Pick<RemoteMenuOptions, 'branch' | 'upstream' | 'remotes'>
): PushCurrentPlan | null {
  if (options.branch.length === 0) {
    return null;
  }
  const tracked = trackedBranch(remote, options);
  if (tracked) {
    return { kind: 'push', remote, branch: tracked };
  }
  return { kind: 'publish', remote, branch: options.branch };
}

/**
 * The entries a remote's group header offers.
 *
 * The group is the remote itself, so the operations name the remote and the
 * current branch rather than a branch of it.
 */
export function remoteGroupItems(remote: string, options: RemoteMenuOptions): RemoteMenuItem[] {
  if (remote === OTHER_REMOTE_GROUP) {
    // The catch-all group is not a remote, so there is nothing to talk to.
    return [];
  }
  const target = pullTarget(remote, options);
  return [
    {
      id: 'fetch-remote',
      labelKey: 'context-fetch-remote',
      labelArgs: { remote },
      icon: 'download',
      disabled: !options.networkAvailable,
      command: { kind: 'action', action: { action: 'fetchRemote', remote } }
    },
    {
      id: 'pull-remote',
      labelKey: 'context-pull-remote',
      labelArgs: { remote },
      icon: 'download',
      disabled: !options.networkAvailable || !options.pullAvailable || target === null,
      command: {
        kind: 'pull',
        remote,
        branch: target ?? '',
        rebase: options.rebase
      }
    },
    {
      id: 'push-remote',
      labelKey: 'context-push-remote',
      labelArgs: { remote },
      icon: 'upload',
      // A detached HEAD has no branch to publish, so the entry is refused
      // rather than sending a command Git would reject.
      disabled: !options.networkAvailable || options.branch.length === 0,
      command: { kind: 'pushCurrent', remote }
    },
    {
      id: 'edit-remote-url',
      labelKey: 'context-edit-remote-url',
      icon: 'pencil',
      disabled: false,
      separatorBefore: true,
      command: { kind: 'editRemote', remote }
    },
    {
      id: 'remove-remote',
      labelKey: 'context-remove-remote',
      icon: 'trash-2',
      disabled: false,
      command: { kind: 'removeRemote', remote }
    },
    {
      id: 'manage-remotes',
      labelKey: 'toolbar-manage-remotes',
      icon: 'globe',
      disabled: false,
      command: { kind: 'manageRemotes' }
    }
  ];
}

/**
 * The network entries one remote branch offers.
 *
 * The branch is named, so the pull reads that branch and the push publishes
 * the current HEAD to it.
 */
export function remoteBranchItems(
  remote: string,
  branch: string,
  options: RemoteMenuOptions
): RemoteMenuItem[] {
  if (remote === OTHER_REMOTE_GROUP) {
    return [];
  }
  const pullItem = (rebase: boolean): RemoteMenuItem => ({
    id: rebase ? 'pull-remote-branch-rebase' : 'pull-remote-branch',
    labelKey: rebase ? 'context-pull-remote-branch-rebase' : 'context-pull-remote-branch',
    labelArgs: { remote, branch },
    icon: 'download',
    disabled: !options.networkAvailable || !options.pullAvailable,
    command: { kind: 'pull', remote, branch, rebase }
  });
  return [
    {
      id: 'fetch-remote-branch',
      labelKey: 'context-fetch-remote',
      labelArgs: { remote },
      icon: 'download',
      disabled: !options.networkAvailable,
      command: { kind: 'action', action: { action: 'fetchRemote', remote } }
    },
    // Both ways of pulling are offered, and the one the preferences ask for
    // comes first.
    options.rebase ? pullItem(true) : pullItem(false),
    options.rebase ? pullItem(false) : pullItem(true),
    {
      id: 'push-remote-branch',
      labelKey: 'context-push-remote-branch',
      labelArgs: { remote, branch },
      icon: 'upload',
      // A repository without a commit has nothing to publish.
      disabled: !options.networkAvailable || !options.hasHead,
      command: {
        kind: 'action',
        action: { action: 'pushTo', remote, branch, force: false }
      }
    }
  ];
}
