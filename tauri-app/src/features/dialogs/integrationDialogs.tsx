/**
 * Dialogs that move commits between branches, and the ones that recover from a
 * failed attempt.
 *
 * A merge that is already contained in the target does nothing, and a rebase
 * over a dirty tree or an unfinished operation fails with an unhelpful
 * message from Git. Both are checked first so the person is told which of
 * those it is. A merge or rebase that stopped on conflicts is reported with
 * the way out rather than only the way back.
 */

import { useState } from 'react';
import { Icon } from '../../components/Icon';
import { Checkbox, DialogCard } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import { integrationBlocked, localBranches, useStore } from '../../app/store';
import { preflightRebase } from '../repository/Toolbar';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';

export function MergeDialog({ noFf }: { noFf: boolean }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const openOverlay = useStore((state) => state.openOverlay);
  const [source, setSource] = useState(() => (repo ? (localBranches(repo)[0] ?? '') : ''));
  const [withNoFf, setWithNoFf] = useState(noFf);
  const [busy, setBusy] = useState(false);

  const branches = repo ? localBranches(repo) : [];

  const confirm = async () => {
    if (!repoId || !source) {
      return;
    }
    setBusy(true);
    try {
      // The probe tells an unresolved merge from a clean tree, so a merge that
      // stopped on conflicts is reported as such instead of as a bare failure.
      // A merge that is already contained in the target is not special-cased:
      // the reference lets Git run it and report "Already up to date", which is
      // a success rather than a refusal.
      const probe = await ipc.probeMerge(repoId, source);
      if (integrationBlocked(probe)) {
        closeOverlay();
        if (probe.merge_head) {
          openOverlay({
            kind: 'mergeConflict',
            source,
            detail: ta(useStore.getState().t, 'merge-conflict-warning', { source })
          });
        }
        setBusy(false);
        return;
      }
      closeOverlay();
      void runAction(repoId, { action: 'merge', source, noFf: withNoFf });
    } catch (error) {
      useStore.getState().reportError(repoId, error);
      setBusy(false);
    }
  };

  return (
    <DialogCard
      testId="merge-dialog"
      title={ta(translate, 'merge-title', { branch: repo?.branch ?? '' })}
      icon={<Icon name="git-merge" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="row">
            <span className="muted">{t(translate, 'merge-source-label')}</span>
            <select
              className="select__trigger"
              style={{ flex: 1 }}
              value={source}
              data-testid="merge-source"
              onChange={(event) => setSource(event.target.value)}
            >
              {branches.map((branch) => (
                <option key={branch} value={branch}>
                  {branch}
                </option>
              ))}
            </select>
          </div>
          <Checkbox
            checked={withNoFf}
            onChange={setWithNoFf}
            label={t(translate, 'merge-no-ff-label')}
            testId="merge-no-ff"
          />
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="merge-dialog-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!source || busy}
            onClick={() => void confirm()}
            data-testid="merge-dialog-confirm"
          >
            {t(translate, 'dialog-confirm')}
          </button>
        </>
      }
    />
  );
}

export function RebaseDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [source, setSource] = useState(() => (repo ? (localBranches(repo)[0] ?? '') : ''));

  const branches = repo ? localBranches(repo) : [];

  const confirm = async () => {
    if (!repoId || !source) {
      return;
    }
    closeOverlay();
    if (repo) {
      await preflightRebase(repo, source);
    }
  };

  return (
    <DialogCard
      testId="rebase-dialog"
      title={ta(translate, 'rebase-title', { branch: repo?.branch ?? '' })}
      icon={<Icon name="git-commit-horizontal" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="status-conflict">
            {ta(translate, 'rebase-warning', { branch: repo?.branch ?? '' })}
          </div>
          <div className="row">
            <span className="muted">{t(translate, 'merge-source-label')}</span>
            <select
              className="select__trigger"
              style={{ flex: 1 }}
              value={source}
              data-testid="rebase-source"
              onChange={(event) => setSource(event.target.value)}
            >
              {branches.map((branch) => (
                <option key={branch} value={branch}>
                  {branch}
                </option>
              ))}
            </select>
          </div>
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="rebase-dialog-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!source}
            onClick={() => void confirm()}
            data-testid="rebase-dialog-confirm"
          >
            {t(translate, 'dialog-confirm')}
          </button>
        </>
      }
    />
  );
}

export function MergeConflictDialog({ source, detail }: { source: string; detail: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="merge-conflict-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} /> {t(translate, 'merge-conflict-title')}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">{ta(translate, 'merge-conflict-warning', { source })}</div>
          <pre
            className="status-conflict"
            style={{ maxHeight: 180, overflow: 'auto', margin: 0, fontSize: '0.7em' }}
            data-testid="merge-conflict-detail"
          >
            {detail}
          </pre>
        </>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--danger"
          data-testid="merge-abort"
          onClick={() => {
            if (repoId) {
              closeOverlay();
              void runAction(repoId, { action: 'abortMerge' });
            }
          }}
        >
          {t(translate, 'merge-abort')}
        </button>
      }
    />
  );
}

export function RebaseConflictDialog({ detail, source }: { detail: string; source?: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="rebase-conflict-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} /> {t(translate, 'rebase-conflict-title')}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted" data-testid="rebase-conflict-warning">
            {ta(translate, 'rebase-conflict-warning', {
              // A conflict raised by a pull rebase has no other source to name.
              source: source ?? 'pull --rebase'
            })}
          </div>
          <pre
            className="status-conflict"
            style={{ maxHeight: 180, overflow: 'auto', margin: 0, fontSize: '0.7em' }}
            data-testid="rebase-conflict-detail"
          >
            {detail}
          </pre>
        </>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--danger"
          data-testid="rebase-abort"
          onClick={() => {
            if (repoId) {
              closeOverlay();
              void runAction(repoId, { action: 'abortRebase' });
            }
          }}
        >
          {t(translate, 'rebase-abort')}
        </button>
      }
    />
  );
}
