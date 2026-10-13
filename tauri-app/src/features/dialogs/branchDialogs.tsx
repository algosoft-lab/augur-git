/**
 * Dialogs that name, rename, and delete refs.
 *
 * Creating and renaming a ref both need a name, so they share one dialog
 * with a mode; deleting needs a warning and, for a branch, a force option
 * because a branch that is not fully merged would otherwise be refused.
 *
 * The name is validated as it is typed so the confirm button reflects the
 * input, and the backend re-validates by letting Git reject anything
 * invalid.
 */

import { useMemo, useState } from 'react';
import { Icon } from '../../components/Icon';
import { Checkbox, DialogCard, Select, TextInput } from '../../components/controls';
import { localBranches, useStore } from '../../app/store';
import { validateBranchName } from './branchName';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';

/** Which of the three name-based dialogs is being shown. */
export type NamedMode =
  | { mode: 'newBranch' }
  | { mode: 'renameBranch'; old: string }
  | { mode: 'renameRemoteBranch'; old: string; remote: string };

export function NamedBranchDialog(props: NamedMode) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [name, setName] = useState(props.mode === 'newBranch' ? '' : props.old);
  const existing = useMemo(() => {
    if (!repo) {
      return [];
    }
    const names = localBranches(repo);
    if (repo.branch) {
      names.push(repo.branch);
    }
    return names;
  }, [repo]);

  // A remote branch name only has to satisfy ref syntax; the remote itself
  // rejects a name that already exists, so a duplicate is not checked here.
  const allow = props.mode === 'renameBranch' ? props.old : undefined;
  const duplicateCheck = props.mode === 'renameRemoteBranch' ? [] : existing;
  const error = validateBranchName(name, duplicateCheck, allow);
  const confirmEnabled = error === null;

  const title =
    props.mode === 'newBranch'
      ? t(translate, 'branch-new-title')
      : props.mode === 'renameBranch'
        ? t(translate, 'branch-rename-title')
        : t(translate, 'rename-remote-branch-title');

  const icon =
    props.mode === 'newBranch' ? (
      <Icon name="git-branch-plus" size={16} />
    ) : (
      <Icon name="pencil" size={16} />
    );

  const hint =
    props.mode === 'newBranch'
      ? ta(translate, 'branch-new-hint', { branch: repo?.branch || 'HEAD' })
      : props.mode === 'renameBranch'
        ? ta(translate, 'branch-rename-hint', { branch: props.old })
        : ta(translate, 'rename-remote-branch-hint', {
            remote: props.remote,
            branch: props.old
          });

  const confirm = () => {
    if (!repoId || !confirmEnabled) {
      return;
    }
    closeOverlay();
    if (props.mode === 'newBranch') {
      void runAction(repoId, { action: 'createBranch', name });
    } else if (props.mode === 'renameBranch') {
      void runAction(repoId, { action: 'renameBranch', old: props.old, new: name });
    } else {
      void runAction(repoId, {
        action: 'pushRenameRemote',
        remote: props.remote,
        old: props.old,
        new: name
      });
    }
  };

  return (
    <DialogCard
      testId="branch-dialog"
      title={title}
      icon={icon}
      onBackdrop={closeOverlay}
      body={
        <>
          <label className="settings__label" htmlFor="branch-name-input">
            {t(translate, 'branch-name-label')}
          </label>
          <TextInput
            value={name}
            onChange={setName}
            autoFocus
            monospace
            testId="branch-name-input"
            onSubmit={confirm}
            onEscape={closeOverlay}
          />
          <div className="settings__hint">{hint}</div>
          {error && error !== 'empty' ? (
            <div className="status-conflict" data-testid="branch-name-error">
              {error === 'exists'
                ? ta(translate, 'branch-name-exists', { name })
                : t(translate, 'branch-name-invalid')}
            </div>
          ) : null}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="branch-dialog-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!confirmEnabled}
            onClick={confirm}
            data-testid="branch-dialog-confirm"
          >
            {t(translate, 'dialog-confirm')}
          </button>
        </>
      }
    />
  );
}

export function DeleteRefDialog({ name, isTag }: { name: string; isTag: boolean }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [force, setForce] = useState(false);

  return (
    <DialogCard
      testId="delete-ref-dialog"
      title={t(translate, isTag ? 'delete-tag-title' : 'delete-branch-title')}
      icon={<Icon name="trash-2" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">
            {ta(translate, isTag ? 'delete-tag-warning' : 'delete-branch-warning', { name })}
          </div>
          {isTag ? null : (
            <Checkbox
              checked={force}
              onChange={setForce}
              label={t(translate, 'delete-force-label')}
              testId="delete-force"
            />
          )}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="delete-ref-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="delete-ref-confirm"
            onClick={() => {
              if (!repoId) {
                return;
              }
              closeOverlay();
              void runAction(
                repoId,
                isTag ? { action: 'deleteTag', name } : { action: 'deleteBranch', name, force }
              );
            }}
          >
            {t(translate, 'dialog-confirm')}
          </button>
        </>
      }
    />
  );
}

/**
 * Remote tag dialogs.
 *
 * The sidebar tag list is flat (the local mirror), so these dialogs choose the
 * remote to operate on. `origin` is the default when it exists, otherwise the
 * first configured remote.
 */
function defaultRemote(remotes: string[]): string {
  return remotes.includes('origin') ? 'origin' : (remotes[0] ?? '');
}

export function DeleteRemoteTagDialog({ name }: { name: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const remotes = repo?.refs.remotes ?? [];
  const [remote, setRemote] = useState(() => defaultRemote(remotes));

  return (
    <DialogCard
      testId="delete-remote-tag-dialog"
      title={t(translate, 'delete-remote-tag-title')}
      icon={<Icon name="trash-2" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">
            {ta(translate, 'delete-remote-tag-warning', { name, remote })}
          </div>
          <span className="settings__label">{t(translate, 'remote-label')}</span>
          <Select
            testId="delete-remote-tag-remote"
            value={remote}
            options={remotes.map((entry) => ({ value: entry, label: entry }))}
            onChange={setRemote}
          />
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="delete-remote-tag-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="delete-remote-tag-confirm"
            disabled={!remote}
            onClick={() => {
              if (!repoId || !remote) {
                return;
              }
              closeOverlay();
              void runAction(repoId, { action: 'pushDeleteTag', remote, name });
            }}
          >
            {t(translate, 'dialog-confirm')}
          </button>
        </>
      }
    />
  );
}

export function RenameRemoteTagDialog({ old }: { old: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const remotes = repo?.refs.remotes ?? [];
  const [remote, setRemote] = useState(() => defaultRemote(remotes));
  const [name, setName] = useState(old);
  // Only the ref syntax is enforced; the remote rejects a name that already
  // exists, so a duplicate is not checked here (same as a remote branch).
  const error = validateBranchName(name, [], old);
  const confirmEnabled = error === null && remote !== '';

  const confirm = () => {
    if (!repoId || !confirmEnabled) {
      return;
    }
    closeOverlay();
    void runAction(repoId, { action: 'pushRenameTag', remote, old, new: name });
  };

  return (
    <DialogCard
      testId="rename-remote-tag-dialog"
      title={t(translate, 'rename-remote-tag-title')}
      icon={<Icon name="pencil" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <label className="settings__label" htmlFor="rename-remote-tag-input">
            {t(translate, 'tag-name-label')}
          </label>
          <TextInput
            value={name}
            onChange={setName}
            autoFocus
            monospace
            testId="rename-remote-tag-input"
            onSubmit={confirm}
            onEscape={closeOverlay}
          />
          <div className="settings__hint">
            {ta(translate, 'rename-remote-tag-hint', { remote, tag: old })}
          </div>
          <span className="settings__label">{t(translate, 'remote-label')}</span>
          <Select
            testId="rename-remote-tag-remote"
            value={remote}
            options={remotes.map((entry) => ({ value: entry, label: entry }))}
            onChange={setRemote}
          />
          {error && error !== 'empty' ? (
            <div className="status-conflict" data-testid="rename-remote-tag-error">
              {t(translate, 'tag-name-invalid')}
            </div>
          ) : null}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="rename-remote-tag-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!confirmEnabled}
            onClick={confirm}
            data-testid="rename-remote-tag-confirm"
          >
            {t(translate, 'dialog-confirm')}
          </button>
        </>
      }
    />
  );
}

export function DeleteRemoteBranchDialog({ remote, branch }: { remote: string; branch: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="delete-remote-dialog"
      title={t(translate, 'delete-remote-branch-title')}
      icon={<Icon name="trash-2" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <div className="muted">
          {ta(translate, 'delete-remote-branch-warning', { remote, branch })}
        </div>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="delete-remote-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="delete-remote-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, {
                  action: 'pushDeleteRemote',
                  remote,
                  branch
                });
              }
            }}
          >
            {t(translate, 'dialog-confirm')}
          </button>
        </>
      }
    />
  );
}
