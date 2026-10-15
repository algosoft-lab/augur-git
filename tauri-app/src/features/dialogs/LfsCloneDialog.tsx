import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { Icon } from '../../components/Icon';
import { Checkbox, DialogCard, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import type { CloneProgressEvent, CloneResult } from '../../bridge/types';
import { t } from '../../i18n/strings';

export function LfsCloneDialog() {
  const translate = useStore((state) => state.t);
  const config = useStore((state) => state.config);
  const openTab = useStore((state) => state.openTab);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [remote, setRemote] = useState('');
  const [parent, setParent] = useState('');
  const [directoryName, setDirectoryName] = useState('');
  const [downloadLfs, setDownloadLfs] = useState(config.lfs_auto_download);
  const [busy, setBusy] = useState(false);
  const [operationId, setOperationId] = useState<string | null>(null);
  const [progress, setProgress] = useState<CloneProgressEvent | null>(null);
  const [error, setError] = useState('');
  const [result, setResult] = useState<CloneResult | null>(null);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void ipc.onLfsCloneProgress((event) => {
      if (active && event.operationId === operationId) setProgress(event);
    }).then((dispose) => {
      if (!active) dispose();
      else unlisten = dispose;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [operationId]);

  const chooseParent = async () => {
    const selected = await open({ directory: true, multiple: false, title: t(translate, 'clone-parent-title') });
    if (typeof selected === 'string') setParent(selected);
  };

  const updateNameFromRemote = (value: string) => {
    setRemote(value);
    if (!directoryName.trim() || directoryName === suggestedName(remote)) {
      setDirectoryName(suggestedName(value));
    }
  };

  const startClone = async () => {
    if (!remote.trim() || !parent.trim() || !directoryName.trim()) return;
    const id = `clone-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    setBusy(true);
    setError('');
    setProgress(null);
    setResult(null);
    setOperationId(id);
    try {
      const cloned = await ipc.lfsClone(remote.trim(), parent.trim(), directoryName.trim(), downloadLfs, id);
      setResult(cloned);
      if (!cloned.lfsError) {
        closeOverlay();
        await openTab(cloned.path, { kind: 'local' });
      }
    } catch (reason) {
      setError(describeError(reason));
    } finally {
      setBusy(false);
      setOperationId(null);
    }
  };

  const cancel = () => {
    if (!operationId) return;
    void ipc.cancelLfsClone(operationId);
  };

  return (
    <DialogCard
      testId="lfs-clone-dialog"
      title={t(translate, 'clone-title')}
      icon={<Icon name="download" size={16} />}
      onBackdrop={() => { if (!busy) closeOverlay(); }}
      body={
        <div className="stack">
          {result ? (
            <>
              {result.lfsError ? <div className="status-conflict" role="alert">{result.lfsError}</div> : null}
              <div className="settings__hint">{result.path}</div>
              {result.lfsError ? <div className="settings__hint">{t(translate, 'clone-partial-help')}</div> : null}
            </>
          ) : (
            <>
              <label className="settings__label" htmlFor="lfs-clone-remote">{t(translate, 'clone-remote-label')}</label>
              <TextInput
                id="lfs-clone-remote"
                value={remote}
                onChange={updateNameFromRemote}
                placeholder="https://example.com/owner/repository.git"
                monospace
                disabled={busy}
                autoFocus
                testId="lfs-clone-remote"
              />
              <label className="settings__label" htmlFor="lfs-clone-parent">{t(translate, 'clone-parent-label')}</label>
              <div className="settings__row">
                <TextInput id="lfs-clone-parent" value={parent} onChange={setParent} monospace disabled={busy} testId="lfs-clone-parent" />
                <button type="button" className="tool-button" disabled={busy} onClick={() => void chooseParent()}>{t(translate, 'clone-choose-parent')}</button>
              </div>
              <label className="settings__label" htmlFor="lfs-clone-directory">{t(translate, 'clone-directory-label')}</label>
              <TextInput id="lfs-clone-directory" value={directoryName} onChange={setDirectoryName} disabled={busy} testId="lfs-clone-directory" />
              <Checkbox
                checked={downloadLfs}
                onChange={setDownloadLfs}
                disabled={busy}
                label={t(translate, 'clone-download-lfs')}
                testId="lfs-clone-download-lfs"
              />
              {busy ? (
                <div className="stack stack--tight" role="status" data-testid="lfs-clone-progress">
                  <div className="settings__hint">{progress?.message ?? t(translate, 'clone-progress-starting')}</div>
                  {progress?.progress ? <div className="settings__hint">{`${progress.progress.currentFile}/${progress.progress.totalFiles} · ${formatBytes(progress.progress.transferredBytes)} / ${formatBytes(progress.progress.totalBytes)}`}</div> : null}
                </div>
              ) : null}
              {error ? <div className="status-conflict" role="alert">{error}</div> : null}
            </>
          )}
        </div>
      }
      footer={
        result ? (
          <>
            <button type="button" className="tool-button" onClick={closeOverlay}>{t(translate, 'dialog-cancel')}</button>
            {result.lfsError ? <button type="button" className="tool-button tool-button--primary" onClick={() => { closeOverlay(); void openTab(result.path, { kind: 'local' }); }}>{t(translate, 'clone-open-partial')}</button> : null}
          </>
        ) : busy ? (
          <button type="button" className="tool-button tool-button--danger" onClick={cancel}>{t(translate, 'clone-cancel')}</button>
        ) : (
          <>
            <button type="button" className="tool-button" onClick={closeOverlay}>{t(translate, 'dialog-cancel')}</button>
            <button type="button" className="tool-button tool-button--primary" disabled={!remote.trim() || !parent.trim() || !directoryName.trim()} onClick={() => void startClone()} data-testid="lfs-clone-start">{t(translate, 'clone-start')}</button>
          </>
        )
      }
    />
  );
}

function suggestedName(remote: string): string {
  const tail = remote.trim().replace(/[\\/]$/, '').split(/[/:\\]/).pop() ?? '';
  return tail.replace(/\.git$/i, '') || '';
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KiB', 'MiB', 'GiB', 'TiB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

function describeError(reason: unknown): string {
  if (reason && typeof reason === 'object' && 'detail' in reason) {
    return String((reason as { detail: unknown }).detail);
  }
  return String(reason);
}
