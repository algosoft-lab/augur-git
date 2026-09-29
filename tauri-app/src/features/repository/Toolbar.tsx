/**
 * The repository toolbar.
 *
 * Availability is derived from repository state rather than tracked separately:
 * an action that would discard or replace an unresolved merge is disabled while
 * conflicts exist, and the branch submenu entries follow the same rule the
 * reference application applies.
 */

import { open } from '@tauri-apps/plugin-dialog';

import { Icon } from '../../components/Icon';
import { Menu, ToolButton, type MenuItemSpec } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import { hasLocalBranches, useStore, type RepoState } from '../../app/store';
import { firstLine } from '../../app/repoState';
import { t, ta } from '../../i18n/strings';

export function Toolbar({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const runAction = useStore((state) => state.runAction);
  const openOverlay = useStore((state) => state.openOverlay);
  const refresh = useStore((state) => state.refresh);
  const hasRemote = repo.refs.remotes.length > 0;
  const blocked = repo.hasConflicts;
  const network = hasRemote && !repo.busy;
  const pull = network && !blocked;

  const branchItems: MenuItemSpec[] = [
    {
      id: 'branch-new',
      label: t(translate, 'menu-branch-new'),
      icon: <Icon name="git-branch-plus" />,
      disabled: blocked,
      onSelect: () => openOverlay({ kind: 'newBranch' })
    },
    {
      id: 'branch-rename',
      label: t(translate, 'menu-branch-rename'),
      icon: <Icon name="pencil" />,
      disabled: repo.branch.length === 0,
      onSelect: () => openOverlay({ kind: 'renameBranch', old: repo.branch })
    },
    {
      id: 'stash',
      label: t(translate, 'menu-stash'),
      icon: <Icon name="archive" />,
      disabled: repo.stashableCount === 0,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: 'stash' })
    },
    {
      id: 'stash-pop',
      label: t(translate, 'menu-stash-pop'),
      icon: <Icon name="archive-restore" />,
      disabled: repo.refs.stashes.length === 0 || blocked,
      onSelect: () => {
        void runAction(repo.id, { action: 'stashPop', stashRef: null });
      }
    },
    {
      id: 'merge',
      label: t(translate, 'menu-merge'),
      icon: <Icon name="git-merge" />,
      disabled: !hasLocalBranches(repo) || blocked,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: 'merge', noFf: false })
    },
    {
      id: 'merge-no-ff',
      label: t(translate, 'menu-merge-no-ff'),
      icon: <Icon name="git-merge" />,
      disabled: !hasLocalBranches(repo) || blocked,
      onSelect: () => openOverlay({ kind: 'merge', noFf: true })
    },
    {
      id: 'rebase',
      label: t(translate, 'menu-rebase'),
      icon: <Icon name="git-commit-horizontal" />,
      disabled: !hasLocalBranches(repo) || blocked,
      onSelect: () => openOverlay({ kind: 'rebase' })
    },
    {
      id: 'apply-patch',
      label: t(translate, 'menu-apply-patch'),
      icon: <Icon name="upload" />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => {
        void pickAndApplyPatch(repo.id);
      }
    }
  ];

  return (
    <div className="toolbar" data-testid="toolbar">
      <Menu items={branchItems} testId="branch-menu">
        <ToolButton
          label={t(translate, 'toolbar-branch')}
          icon={<Icon name="git-branch" />}
          tooltip={t(translate, 'toolbar-branch')}
          disabled={repo.busy}
          testId="toolbar-branch"
        />
      </Menu>
      <ToolButton
        label={t(translate, 'toolbar-fetch')}
        icon={<Icon name="download" />}
        disabled={!network}
        testId="toolbar-fetch"
        onClick={() => void runAction(repo.id, { action: 'fetch' })}
      />
      <ToolButton
        label={t(translate, 'toolbar-pull')}
        icon={<Icon name="download" />}
        disabled={!pull}
        testId="toolbar-pull"
        onClick={() => triggerPull(repo)}
      />
      <ToolButton
        label={t(translate, 'toolbar-push')}
        icon={<Icon name="upload" />}
        disabled={!network}
        testId="toolbar-push"
        onClick={() => triggerPush(repo)}
      />
      <ToolButton
        label={t(translate, 'toolbar-push-force')}
        icon={<Icon name="triangle-alert" />}
        disabled={!network}
        testId="toolbar-push-force"
        // Never runs directly: the confirmation comes first.
        onClick={() => openOverlay({ kind: 'forcePush' })}
      />
      <ToolButton
        label={t(translate, 'toolbar-compare')}
        icon={<Icon name="git-branch" />}
        disabled={repo.busy}
        testId="toolbar-compare"
        onClick={() => {
          void ipc.openCompareWindow(repo.id);
        }}
      />
      <span className="count-badge count-badge--ahead" title="ahead">
        <Icon name="chevron-up" size={10} />
        {repo.ahead}
      </span>
      <span className="count-badge count-badge--behind" title="behind">
        <Icon name="chevron-down" size={10} />
        {repo.behind}
      </span>
      <div className="toolbar__spacer" />
      <ToolButton
        label={t(translate, 'toolbar-refresh')}
        icon={<Icon name="refresh-cw" />}
        // Deliberately not disabled while busy, as in the reference: a refresh is
        // a read, and refusing it mid-operation is a worse answer than a
        // snapshot that arrives slightly out of date.
        testId="toolbar-refresh"
        onClick={() => void refresh(repo.id)}
      />
    </div>
  );
}

/** Whether a plain push should first offer to publish the branch. */
export function shouldOfferUpstream(repo: RepoState): boolean {
  return (
    repo.branch.length > 0 &&
    !repo.branch.startsWith('HEAD') &&
    repo.upstream === null &&
    repo.refs.remotes.length > 0
  );
}

function defaultPushRemote(remotes: string[]): string {
  return remotes.includes('origin') ? 'origin' : (remotes[0] ?? 'origin');
}

/** Run the toolbar's Pull behavior from a keyboard command. */
export function triggerPull(repo: RepoState): void {
  if (repo.refs.remotes.length === 0 || repo.busy || repo.hasConflicts) {
    return;
  }
  const store = useStore.getState();
  if (store.config.view.pull_action === 'rebase') {
    void preflightRebase(repo, null);
  } else {
    void store.runAction(repo.id, { action: 'pullMerge' });
  }
}

/** Run the toolbar's Push behavior, including the missing-upstream prompt. */
export function triggerPush(repo: RepoState): void {
  if (repo.refs.remotes.length === 0 || repo.busy) {
    return;
  }
  const store = useStore.getState();
  if (shouldOfferUpstream(repo)) {
    store.openOverlay({
      kind: 'pushSetUpstream',
      branch: repo.branch,
      remote: defaultPushRemote(repo.refs.remotes)
    });
    return;
  }
  void store.runAction(repo.id, { action: 'push' });
}

/** Ask for a patch file and apply it. Plain `git apply` is atomic. */
async function pickAndApplyPatch(repoId: number): Promise<void> {
  const selected = await open({
    multiple: false,
    directory: false,
    title: useStore.getState().t('menu-apply-patch-prompt'),
    filters: [{ name: 'Patch', extensions: ['patch', 'diff'] }]
  });
  if (typeof selected !== 'string') {
    return;
  }
  await useStore.getState().runAction(repoId, { action: 'applyPatch', path: selected });
}

/**
 * Check the repository before a rebase.
 *
 * A rebase that starts while a merge, cherry-pick, revert, or bisect is in
 * progress would replace that operation, and a branch rebase over a dirty tree
 * fails inside Git with a message that is hard to act on. Both are refused here
 * with a specific message instead.
 */
export async function preflightRebase(repo: RepoState, source: string | null): Promise<void> {
  const store = useStore.getState();
  let probe;
  try {
    probe = await ipc.probeRebase(repo.id, source);
  } catch (error) {
    // A closed repository has nowhere to report to; anything else is a
    // preflight failure, which the reference names with Git's own first line
    // and a sentence that says the rebase never started.
    if (!useStore.getState().repos[repo.id]) {
      return;
    }
    const failure = ipc.describeError(error);
    store.setMessage(
      repo.id,
      ta(store.t, 'rebase-preflight-failed', {
        error: firstLine(failure.detail)
      }),
      false
    );
    return;
  }
  if (probe.other_operation_in_progress || probe.rebase_in_progress) {
    store.setMessage(repo.id, t(store.t, 'rebase-preflight-operation-in-progress'), false);
    return;
  }
  if (source && probe.has_changes) {
    store.setMessage(repo.id, t(store.t, 'rebase-preflight-dirty'), false);
    return;
  }
  if (source) {
    void store.runAction(repo.id, { action: 'rebase', source });
  } else {
    void store.runAction(repo.id, { action: 'pullRebase' });
  }
}
