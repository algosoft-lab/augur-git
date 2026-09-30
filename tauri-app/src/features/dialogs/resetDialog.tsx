import { useEffect, useMemo, useState } from 'react';

import { Icon } from '../../components/Icon';
import { DialogCard } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import type { ResetMode, ResetPreview, ResetTarget } from '../../bridge/types';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';

type PreviewState =
  | { kind: 'invalid' }
  | { kind: 'loading' }
  | { kind: 'ready'; value: ResetPreview }
  | { kind: 'error'; detail: string };

export function ResetDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [mode, setMode] = useState<ResetMode>('soft');
  const [targetKind, setTargetKind] = useState<'headAncestor' | 'commit'>('headAncestor');
  const [steps, setSteps] = useState('1');
  const [sha, setSha] = useState('');
  const [preview, setPreview] = useState<PreviewState>({ kind: 'invalid' });

  const target = useMemo<ResetTarget | null>(() => {
    if (targetKind === 'headAncestor') {
      if (!/^\d+$/.test(steps)) return null;
      const count = Number(steps);
      return Number.isSafeInteger(count) && count > 0
        ? { kind: 'headAncestor', steps: count }
        : null;
    }
    const value = sha.trim();
    return /^[\da-fA-F]{7,64}$/.test(value) ? { kind: 'commit', sha: value } : null;
  }, [sha, steps, targetKind]);

  useEffect(() => {
    if (!repoId || !target) {
      setPreview({ kind: 'invalid' });
      return;
    }
    let current = true;
    setPreview({ kind: 'loading' });
    void ipc
      .previewReset(repoId, target)
      .then((value) => {
        if (current) setPreview({ kind: 'ready', value });
      })
      .catch((error: unknown) => {
        if (current) setPreview({ kind: 'error', detail: ipc.describeError(error).detail });
      });
    return () => {
      current = false;
    };
  }, [repoId, target]);

  const confirm = () => {
    if (preview.kind !== 'ready' || !repoId) return;
    const value = preview.value;
    closeOverlay();
    void runAction(repoId, {
      action: 'reset',
      mode,
      expectedHead: value.head_oid,
      targetOid: value.target_oid
    });
  };

  const targetInputError =
    targetKind === 'headAncestor'
      ? t(translate, 'reset-invalid-count')
      : t(translate, 'reset-invalid-sha');

  return (
    <DialogCard
      testId="reset-dialog"
      width={560}
      title={t(translate, 'reset-title')}
      icon={<Icon name="undo" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <div className="stack">
          <div className="row">
            <label className="muted" htmlFor="reset-mode">
              {t(translate, 'reset-mode-label')}
            </label>
            <select
              id="reset-mode"
              className="select__trigger"
              value={mode}
              data-testid="reset-mode"
              onChange={(event) => setMode(event.target.value as ResetMode)}
            >
              <option value="soft">{t(translate, 'reset-soft')}</option>
              <option value="hard">{t(translate, 'reset-hard')}</option>
            </select>
          </div>
          <div className="row">
            <label className="muted" htmlFor="reset-target-kind">
              {t(translate, 'reset-target-label')}
            </label>
            <select
              id="reset-target-kind"
              className="select__trigger"
              value={targetKind}
              data-testid="reset-target-kind"
              onChange={(event) => setTargetKind(event.target.value as typeof targetKind)}
            >
              <option value="headAncestor">{t(translate, 'reset-head-ancestor')}</option>
              <option value="commit">{t(translate, 'reset-commit-sha')}</option>
            </select>
            {targetKind === 'headAncestor' ? (
              <input
                className="select__trigger"
                type="number"
                min="1"
                step="1"
                value={steps}
                aria-label={t(translate, 'reset-count-label')}
                data-testid="reset-count"
                onChange={(event) => setSteps(event.target.value)}
              />
            ) : (
              <input
                className="select__trigger mono"
                type="text"
                value={sha}
                placeholder={t(translate, 'reset-sha-placeholder')}
                aria-label={t(translate, 'reset-sha-label')}
                data-testid="reset-sha"
                spellCheck={false}
                onChange={(event) => setSha(event.target.value)}
              />
            )}
          </div>
          <div className="muted" data-testid="reset-mode-description">
            {mode === 'soft'
              ? t(translate, 'reset-soft-description')
              : t(translate, 'reset-hard-warning')}
          </div>
          {preview.kind === 'loading' ? (
            <div className="muted" data-testid="reset-preview-loading">
              {t(translate, 'reset-preview-loading')}
            </div>
          ) : preview.kind === 'ready' ? (
            <div className="stack stack--tight" data-testid="reset-preview">
              <div>
                {ta(translate, 'reset-preview-target', { subject: preview.value.target_subject })}
              </div>
              <div className="mono muted">{preview.value.target_oid}</div>
              <div className="muted">
                {ta(translate, 'reset-preview-commits', { count: preview.value.moved_commits })}
              </div>
            </div>
          ) : preview.kind === 'error' ? (
            <div className="status-conflict" data-testid="reset-preview-error">
              {preview.detail}
            </div>
          ) : targetKind === 'commit' && sha.length > 0 ? (
            <div className="status-conflict" data-testid="reset-input-error">
              {targetInputError}
            </div>
          ) : targetKind === 'headAncestor' && steps.length > 0 ? (
            <div className="status-conflict" data-testid="reset-input-error">
              {targetInputError}
            </div>
          ) : null}
        </div>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="reset-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className={`tool-button ${mode === 'hard' ? 'tool-button--danger' : 'tool-button--primary'}`}
            disabled={preview.kind !== 'ready' || !repo || repo.busy}
            onClick={confirm}
            data-testid="reset-confirm"
          >
            {t(translate, mode === 'hard' ? 'reset-confirm-hard' : 'reset-confirm-soft')}
          </button>
        </>
      }
    />
  );
}
