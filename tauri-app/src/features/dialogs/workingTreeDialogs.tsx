/**
 * Dialogs that move or destroy uncommitted work.
 *
 * Stashing names the change so it can be found later, dropping a stash names
 * the reference because the two are otherwise indistinguishable, and
 * discarding lists the files because it cannot be undone.
 */

import { useMemo, useState } from 'react';
import { Icon } from '../../components/Icon';
import { DialogCard, TextInput } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import { Overlay, useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';

export function StashDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [message, setMessage] = useState('');

  return (
    <DialogCard
      testId="stash-dialog"
      title={t(translate, 'stash-title')}
      icon={<Icon name="archive" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <label className="settings__label" htmlFor="stash-message">
            {t(translate, 'stash-message-label')}
          </label>
          <TextInput
            value={message}
            onChange={setMessage}
            autoFocus
            testId="stash-message"
            onSubmit={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: 'stash', message });
              }
            }}
            onEscape={closeOverlay}
          />
          <div className="settings__hint">
            {ta(translate, 'stash-hint', { count: repo?.stashableCount ?? 0 })}
          </div>
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="stash-dialog-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            data-testid="stash-dialog-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: 'stash', message });
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

export function StashDropDialog({ reference }: { reference: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="stash-drop-dialog"
      title={t(translate, 'stash-drop-title')}
      icon={<Icon name="trash-2" size={16} />}
      onBackdrop={closeOverlay}
      body={<div className="muted">{ta(translate, 'stash-drop-warning', { reference })}</div>}
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="stash-drop-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="stash-drop-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: 'stashDrop', stashRef: reference });
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

export function DiscardDialog({ overlay }: { overlay: Extract<Overlay, { kind: 'discard' }> }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [busy, setBusy] = useState(false);

  const files = useMemo(() => {
    if (!repo) {
      return [];
    }
    const staged = overlay.scope.staged;
    return repo.files.filter((file) => (staged ? file.index !== ' ' : file.worktree !== ' '));
  }, [repo, overlay.scope.staged]);

  // Each warning names what is about to be destroyed, because "discard" alone
  // does not say whether one file or the whole tree is going.
  const warning =
    overlay.scope.all && files.length === 1
      ? ta(translate, 'discard-file-warning', { path: files[0]?.path ?? '' })
      : overlay.trackedCount > 0
        ? ta(translate, 'discard-all-warning', {
            tracked: overlay.trackedCount,
            untracked: overlay.untrackedCount
          })
        : ta(translate, 'discard-untracked-file-warning', {
            path: files[0]?.path ?? ''
          });

  return (
    <DialogCard
      testId="discard-dialog"
      title={t(translate, 'discard-title')}
      icon={<Icon name="undo" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">{warning}</div>
          {files.length > 0 && files.length <= 12 ? (
            <ul className="stack stack--tight" style={{ margin: 0, paddingLeft: 18 }}>
              {files.map((file) => (
                <li key={file.path} className="mono">
                  {file.path}
                </li>
              ))}
            </ul>
          ) : null}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="discard-cancel"
          >
            {t(translate, 'discard-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            disabled={busy}
            data-testid="discard-confirm"
            onClick={async () => {
              if (!repoId) {
                return;
              }
              setBusy(true);
              try {
                await ipc.workingTreeOperation(repoId, 'discard', files, overlay.scope.all);
                useStore.getState().setBusy(repoId, true);
                closeOverlay();
              } catch (error) {
                useStore.getState().reportError(repoId, error);
                setBusy(false);
              }
            }}
          >
            {t(translate, 'discard-confirm')}
          </button>
        </>
      }
    />
  );
}
