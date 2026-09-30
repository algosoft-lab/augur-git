/**
 * The repository toolbar.
 *
 * Availability is derived from repository state rather than tracked separately:
 * an action that would discard or replace an unresolved merge is disabled while
 * conflicts exist, and the branch submenu entries follow the same rule the
 * reference application applies.
 */

import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { Icon } from '../../components/Icon';
import { DialogCard, Menu, ToolButton, type MenuItemSpec } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import { hasLocalBranches, useStore, type RepoState } from '../../app/store';
import { firstLine } from '../../app/repoState';
import { t, ta } from '../../i18n/strings';
import { copyAgentPrompt } from '../agentPrompt/copyAgentPrompt';

/** Shared branch submenu contents for the toolbar and application Edit menu. */
export function createBranchMenuItems(
  repo: RepoState | null,
  translate: (key: string) => string,
  onSelect: (action: string) => void
): MenuItemSpec[] {
  const blocked = repo?.hasConflicts ?? true;
  const branch = repo?.branch ?? '';
  const hasBranches = repo ? hasLocalBranches(repo) : false;
  return [
    {
      id: 'branch-new',
      label: translate('menu-branch-new'),
      icon: <Icon name="git-branch-plus" />,
      disabled: !repo || blocked,
      onSelect: () => onSelect('branch-new')
    },
    {
      id: 'branch-rename',
      label: translate('menu-branch-rename'),
      icon: <Icon name="pencil" />,
      disabled: !repo || branch.length === 0,
      onSelect: () => onSelect('branch-rename')
    },
    {
      id: 'stash',
      label: translate('menu-stash'),
      icon: <Icon name="archive" />,
      disabled: !repo || repo.stashableCount === 0,
      separatorBefore: true,
      onSelect: () => onSelect('stash')
    },
    {
      id: 'stash-pop',
      label: translate('menu-stash-pop'),
      icon: <Icon name="archive-restore" />,
      disabled: !repo || repo.refs.stashes.length === 0 || blocked,
      onSelect: () => onSelect('stash-pop')
    },
    {
      id: 'merge',
      label: translate('menu-merge'),
      icon: <Icon name="git-merge" />,
      disabled: !repo || !hasBranches || blocked,
      separatorBefore: true,
      onSelect: () => onSelect('merge')
    },
    {
      id: 'merge-no-ff',
      label: translate('menu-merge-no-ff'),
      icon: <Icon name="git-merge" />,
      disabled: !repo || !hasBranches || blocked,
      onSelect: () => onSelect('merge-no-ff')
    },
    {
      id: 'rebase',
      label: translate('menu-rebase'),
      icon: <Icon name="git-commit-horizontal" />,
      disabled: !repo || !hasBranches || blocked,
      onSelect: () => onSelect('rebase')
    },
    {
      id: 'apply-patch',
      label: translate('menu-apply-patch'),
      icon: <Icon name="upload" />,
      disabled: !repo || blocked,
      separatorBefore: true,
      onSelect: () => onSelect('apply-patch')
    },
    {
      id: 'apply-patch-ai',
      label: translate('agent-prompt-apply-patch'),
      icon: <Icon name="copy" />,
      disabled: !repo || blocked || repo.busy,
      onSelect: () => onSelect('apply-patch-ai')
    }
  ];
}

export function Toolbar({ repo, compact = false }: { repo: RepoState; compact?: boolean }) {
  const translate = useStore((state) => state.t);
  const runAction = useStore((state) => state.runAction);
  const openOverlay = useStore((state) => state.openOverlay);
  const refresh = useStore((state) => state.refresh);
  const hasRemote = repo.refs.remotes.length > 0;
  const blocked = repo.hasConflicts;
  const network = hasRemote && !repo.busy;
  const pull = network && !blocked;
  const [patchInFlight, setPatchInFlight] = useState<{ repoId: number; path: string } | null>(null);
  const [patchFailure, setPatchFailure] = useState<{
    repoId: number;
    path: string;
    detail: string;
  } | null>(null);

  useEffect(() => {
    if (patchInFlight === null || patchInFlight.repoId !== repo.id || repo.busy) {
      return;
    }
    if (repo.message?.ok === false) {
      setPatchFailure({ ...patchInFlight, detail: repo.message.text });
    }
    setPatchInFlight(null);
  }, [patchInFlight, repo.busy, repo.message]);

  useEffect(() => {
    const onPatchAction = (event: Event) => {
      const detail = (event as CustomEvent<{ repoId: number; copyPrompt: boolean }>).detail;
      if (detail?.repoId !== repo.id || repo.busy || repo.hasConflicts) return;
      if (detail.copyPrompt) void pickAndCopyPatchPrompt(repo.id);
      else void pickAndApplyPatch(repo.id, setPatchInFlight);
    };
    window.addEventListener('augur:toolbar-patch-action', onPatchAction);
    return () => window.removeEventListener('augur:toolbar-patch-action', onPatchAction);
  }, [repo.busy, repo.hasConflicts, repo.id]);

  const branchItems = createBranchMenuItems(
    repo,
    (key) => t(translate, key),
    (action) => {
      switch (action) {
        case 'branch-new':
          openOverlay({ kind: 'newBranch' });
          break;
        case 'branch-rename':
          openOverlay({ kind: 'renameBranch', old: repo.branch });
          break;
        case 'stash':
          openOverlay({ kind: 'stash' });
          break;
        case 'stash-pop':
          void runAction(repo.id, { action: 'stashPop', stashRef: null });
          break;
        case 'merge':
          openOverlay({ kind: 'merge', noFf: false });
          break;
        case 'merge-no-ff':
          openOverlay({ kind: 'merge', noFf: true });
          break;
        case 'rebase':
          openOverlay({ kind: 'rebase' });
          break;
        case 'apply-patch':
          void pickAndApplyPatch(repo.id, setPatchInFlight);
          break;
        case 'apply-patch-ai':
          void pickAndCopyPatchPrompt(repo.id);
          break;
      }
    }
  );

  const compactMoreItems: MenuItemSpec[] = [
    ...branchItems,
    {
      id: 'push-force',
      label: t(translate, 'toolbar-push-force'),
      icon: <Icon name="triangle-alert" />,
      disabled: !network,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: 'forcePush' })
    },
    {
      id: 'compare',
      label: t(translate, 'toolbar-compare'),
      icon: <Icon name="git-branch" />,
      disabled: repo.busy,
      onSelect: () => void ipc.openCompareWindow(repo.id)
    }
  ];

  return (
    <div className={`toolbar${compact ? ' toolbar--sidecar' : ''}`} data-testid="toolbar">
      {compact ? (
        <>
          <ToolButton
            label={t(translate, 'toolbar-fetch')}
            icon={<Icon name="download" />}
            compact
            disabled={!network}
            testId="toolbar-fetch"
            onClick={() => void runAction(repo.id, { action: 'fetch' })}
          />
          <ToolButton
            label={t(translate, 'toolbar-pull')}
            icon={<Icon name="download" />}
            compact
            disabled={!pull}
            testId="toolbar-pull"
            onClick={() => triggerPull(repo)}
          />
          <ToolButton
            label={t(translate, 'toolbar-push')}
            icon={<Icon name="upload" />}
            compact
            disabled={!network}
            testId="toolbar-push"
            onClick={() => triggerPush(repo)}
          />
          <ToolButton
            label={t(translate, 'toolbar-refresh')}
            icon={<Icon name="refresh-cw" />}
            compact
            testId="toolbar-refresh"
            onClick={() => void refresh(repo.id)}
          />
          <Menu items={compactMoreItems} testId="sidecar-more-menu" align="end">
            <ToolButton
              label={t(translate, 'menu-more')}
              tooltip={t(translate, 'menu-more')}
              icon={<Icon name="menu" />}
              compact
              testId="sidecar-more"
            />
          </Menu>
          <span className="toolbar__spacer" />
          <span className="count-badge count-badge--ahead" title={t(translate, 'toolbar-ahead')}>
            <Icon name="chevron-up" size={10} /> {repo.ahead}
          </span>
          <span className="count-badge count-badge--behind" title={t(translate, 'toolbar-behind')}>
            <Icon name="chevron-down" size={10} /> {repo.behind}
          </span>
        </>
      ) : (
        <>
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
        </>
      )}
      {patchFailure?.repoId === repo.id ? (
        <DialogCard
          testId="patch-prompt-error"
          title={
            <>
              <Icon name="triangle-alert" size={16} />{' '}
              {t(translate, 'agent-prompt-apply-patch-error-title')}
            </>
          }
          onBackdrop={() => setPatchFailure(null)}
          body={
            <div className="mono muted" data-testid="patch-prompt-error-detail">
              {patchFailure.detail}
            </div>
          }
          footer={
            <>
              <button
                type="button"
                className="tool-button"
                onClick={() => setPatchFailure(null)}
                data-testid="patch-prompt-error-close"
              >
                {t(translate, 'rebase-error-close')}
              </button>
              <button
                type="button"
                className="tool-button tool-button--primary"
                data-testid="patch-prompt-error-copy"
                onClick={() =>
                  void copyAgentPrompt(repo.id, {
                    kind: 'applyPatch',
                    path: patchFailure.path,
                    failure: patchFailure.detail
                  })
                }
              >
                <Icon name="copy" size={12} /> {t(translate, 'agent-prompt-apply-patch')}
              </button>
            </>
          }
        />
      ) : null}
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
async function choosePatchFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    title: useStore.getState().t('menu-apply-patch-prompt'),
    filters: [{ name: 'Patch', extensions: ['patch', 'diff'] }]
  });
  return typeof selected === 'string' ? selected : null;
}

async function pickAndApplyPatch(
  repoId: number,
  onQueued: (action: { repoId: number; path: string }) => void
): Promise<void> {
  const selected = await choosePatchFile();
  if (!selected) {
    return;
  }
  await useStore.getState().runAction(repoId, { action: 'applyPatch', path: selected });
  onQueued({ repoId, path: selected });
}

async function pickAndCopyPatchPrompt(repoId: number): Promise<void> {
  const selected = await choosePatchFile();
  if (selected) {
    await copyAgentPrompt(repoId, { kind: 'applyPatch', path: selected });
  }
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
