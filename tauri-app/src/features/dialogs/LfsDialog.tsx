import { useEffect, useMemo, useState } from 'react';

import { Icon } from '../../components/Icon';
import { Checkbox, DialogCard, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import type { LfsCachePreview, LfsLock, LfsProgressEvent } from '../../bridge/types';
import { t, ta } from '../../i18n/strings';

export function LfsDialog({ repoId }: { repoId: number }) {
  const translate = useStore((state) => state.t);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [snapshot, setSnapshot] = useState<Awaited<ReturnType<typeof ipc.lfsSnapshot>> | null>(null);
  const [locks, setLocks] = useState<LfsLock[]>([]);
  const [cache, setCache] = useState<LfsCachePreview | null>(null);
  const [diagnosticOutput, setDiagnosticOutput] = useState('');
  const [filter, setFilter] = useState('');
  const [pattern, setPattern] = useState('');
  const [lockPath, setLockPath] = useState('');
  const [lockable, setLockable] = useState(false);
  const [include, setInclude] = useState('');
  const [exclude, setExclude] = useState('');
  const [endpoint, setEndpoint] = useState('');
  const [fetchRemote, setFetchRemote] = useState('origin');
  const [fetchReference, setFetchReference] = useState('');
  const [concurrentTransfers, setConcurrentTransfers] = useState('');
  const [transferRetries, setTransferRetries] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [progress, setProgress] = useState<LfsProgressEvent['progress'] | null>(null);
  const [operationId, setOperationId] = useState<string | null>(null);
  const [confirmPrune, setConfirmPrune] = useState(false);
  const [forceUnlockId, setForceUnlockId] = useState<string | null>(null);

  const refresh = async () => {
    const next = await ipc.lfsSnapshot(repoId);
    setSnapshot(next);
    setEndpoint(next.repository.endpoint ?? '');
    setInclude(next.repository.fetchInclude ?? '');
    setExclude(next.repository.fetchExclude ?? '');
    setConcurrentTransfers(next.repository.concurrentTransfers?.toString() ?? '');
    setTransferRetries(next.repository.transferRetries?.toString() ?? '');
  };

  useEffect(() => {
    let alive = true;
    void ipc.lfsSnapshot(repoId).then((next) => {
      if (alive) {
        setSnapshot(next);
        setEndpoint(next.repository.endpoint ?? '');
        setInclude(next.repository.fetchInclude ?? '');
        setExclude(next.repository.fetchExclude ?? '');
        setConcurrentTransfers(next.repository.concurrentTransfers?.toString() ?? '');
        setTransferRetries(next.repository.transferRetries?.toString() ?? '');
      }
    }).catch((reason: unknown) => {
      if (alive) setError(describeError(reason));
    });
    return () => { alive = false; };
  }, [repoId]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void ipc.onLfsProgress((event) => {
      if (event.repoId === repoId && event.operationId === operationId) setProgress(event.progress);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [operationId, repoId]);

  const files = useMemo(() => {
    const query = filter.trim().toLocaleLowerCase();
    return (snapshot?.files ?? []).filter((file) => !query || file.path.toLocaleLowerCase().includes(query));
  }, [filter, snapshot?.files]);

  const perform = async (operation: () => Promise<unknown>, completed?: string) => {
    setBusy(true);
    setError('');
    setNotice('');
    try {
      await operation();
      if (completed) setNotice(t(translate, completed));
      await refresh();
    } catch (reason) {
      setError(describeError(reason));
    } finally {
      setBusy(false);
    }
  };

  const download = async (allHistory = false) => {
    const id = `lfs-${repoId}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    setOperationId(id);
    setProgress(null);
    await perform(
      () => ipc.lfsFetch(repoId, id, allHistory),
      allHistory ? 'lfs-download-history-complete' : 'lfs-download-complete'
    );
    setOperationId(null);
    setProgress(null);
  };

  const downloadReference = async () => {
    const id = `lfs-${repoId}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    setOperationId(id);
    setProgress(null);
    await perform(
      () => ipc.lfsFetch(repoId, id, false, fetchRemote.trim(), fetchReference.trim()),
      'lfs-download-reference-complete'
    );
    setOperationId(null);
    setProgress(null);
  };

  const repository = snapshot?.repository;
  const environment = snapshot?.environment;
  const storageCleanupDisabled = Boolean(cache?.output.includes('cleanup is disabled'));

  return (
    <DialogCard
      testId="lfs-dialog"
      width={760}
      title={t(translate, 'lfs-manage-title')}
      icon={<Icon name="download" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <div className="lfs-manager stack">
          <section className="stack stack--tight">
            <strong>{t(translate, 'lfs-environment-title')}</strong>
            <div className="settings__hint">
              {!environment ? t(translate, 'lfs-environment-checking') : environment.installed
                ? `${environment.version ?? ''}${environment.path ? ` · ${environment.path}` : ''}`
                : t(translate, 'lfs-environment-missing')}
            </div>
            {repository?.detail ? <div className="settings__hint">{repository.detail}</div> : null}
            {repository?.hookConflict ? <div className="settings__hint">{t(translate, 'lfs-hook-manual-instructions')}</div> : null}
            <button
              type="button"
              className="tool-button"
              disabled={busy}
              onClick={() => {
                setError('');
                void refresh().catch((reason: unknown) => setError(describeError(reason)));
              }}
            >
              {t(translate, 'lfs-environment-recheck')}
            </button>
            {repository?.usesLfs && environment?.installed && !repository.initialized ? (
              <button type="button" className="tool-button tool-button--primary" disabled={busy} onClick={() => void perform(() => ipc.lfsInitialize(repoId), 'lfs-initialize-complete')}>
                {t(translate, 'lfs-initialize')}
              </button>
            ) : null}
          </section>

          {repository?.usesLfs ? (
            <>
              <Checkbox
                checked={repository.automaticDownload}
                onChange={(enabled) => void perform(() => ipc.lfsUpdatePreferences(repoId, { automaticDownload: enabled }), 'lfs-preferences-saved')}
                label={t(translate, 'lfs-repository-auto-download')}
                testId="lfs-repository-auto-download"
              />
              <section className="stack stack--tight">
                <div className="settings__row">
                  <strong>{t(translate, 'lfs-files-title')}</strong>
                  <button type="button" className="tool-button" disabled={busy || !environment?.installed || !repository.initialized} onClick={() => void download()}>
                    <Icon name="download" size={13} /> {t(translate, 'lfs-download-all')}
                  </button>
                </div>
                <div className="settings__row">
                  <span className="settings__hint">{t(translate, 'lfs-history-download-warning')}</span>
                  <button type="button" className="tool-button" disabled={busy || !environment?.installed || !repository.initialized} onClick={() => void download(true)}>
                    {t(translate, 'lfs-download-history')}
                  </button>
                </div>
                <div className="settings__row">
                  <TextInput value={fetchRemote} onChange={setFetchRemote} placeholder={t(translate, 'lfs-reference-remote')} ariaLabel={t(translate, 'lfs-reference-remote')} />
                  <TextInput value={fetchReference} onChange={setFetchReference} placeholder={t(translate, 'lfs-reference-name')} ariaLabel={t(translate, 'lfs-reference-name')} />
                  <button type="button" className="tool-button" disabled={busy || !environment?.installed || !repository.initialized || !fetchRemote.trim() || !fetchReference.trim()} onClick={() => void downloadReference()}>
                    {t(translate, 'lfs-download-reference')}
                  </button>
                </div>
                <TextInput value={filter} onChange={setFilter} placeholder={t(translate, 'lfs-search-files')} ariaLabel={t(translate, 'lfs-search-files')} testId="lfs-file-search" />
                <div className="lfs-manager__files" data-testid="lfs-files-list">
                  {files.length === 0 ? <div className="settings__hint">{t(translate, 'lfs-files-empty')}</div> : files.map((file) => (
                    <div className="lfs-manager__file" key={`${file.path}:${file.oid}`}>
                      <div className="lfs-manager__file-detail">
                        <span className="mono" title={file.path}>{file.path}</span>
                        <span className="settings__hint">{file.size === null ? '' : formatBytes(file.size)} · {file.oid.slice(0, 16)}</span>
                      </div>
                      {file.present ? <span className="settings__hint">{t(translate, 'lfs-file-available')}</span> : (
                        <button type="button" className="tool-button" disabled={busy || !repository.initialized} onClick={() => void perform(() => ipc.lfsDownloadFile(repoId, file.path))}>
                          {t(translate, 'lfs-download-file')}
                        </button>
                      )}
                    </div>
                  ))}
                </div>
                {operationId ? <div className="settings__row" data-testid="lfs-transfer-progress">
                  <div className="settings__hint">
                    {progress
                      ? `${progress.direction} ${progress.currentFile}/${progress.totalFiles} · ${formatBytes(progress.transferredBytes)} / ${formatBytes(progress.totalBytes)} · ${progress.path}`
                      : t(translate, 'lfs-transfer-connecting')}
                  </div>
                  <button type="button" className="tool-button" onClick={() => void ipc.lfsCancel(operationId)}>{t(translate, 'lfs-cancel-transfer')}</button>
                </div> : null}
              </section>

              <section className="stack stack--tight">
                <strong>{t(translate, 'lfs-tracking-title')}</strong>
                <TextInput value={pattern} onChange={setPattern} placeholder={t(translate, 'lfs-pattern-placeholder')} testId="lfs-pattern" />
                <Checkbox checked={lockable} onChange={setLockable} label={t(translate, 'lfs-lockable')} testId="lfs-lockable" />
                <div className="settings__row">
                  <button type="button" className="tool-button" disabled={busy || !pattern.trim()} onClick={() => void perform(() => ipc.lfsTrack(repoId, pattern.trim(), lockable), 'lfs-track-complete')}>
                    {t(translate, 'lfs-track')}
                  </button>
                  <button type="button" className="tool-button" disabled={busy || !pattern.trim()} onClick={() => void perform(() => ipc.lfsUntrack(repoId, pattern.trim()), 'lfs-untrack-complete')}>
                    {t(translate, 'lfs-untrack')}
                  </button>
                </div>
              </section>

              <section className="stack stack--tight">
                <div className="settings__row">
                  <strong>{t(translate, 'lfs-locks-title')}</strong>
                  <button type="button" className="tool-button" disabled={busy} onClick={() => void perform(async () => setLocks(await ipc.lfsLocks(repoId)))}>{t(translate, 'lfs-refresh-locks')}</button>
                </div>
                <div className="settings__row">
                  <TextInput value={lockPath} onChange={setLockPath} placeholder={t(translate, 'lfs-lock-path-placeholder')} testId="lfs-lock-path" />
                  <button type="button" className="tool-button" disabled={busy || !lockPath.trim()} onClick={() => void perform(() => ipc.lfsLock(repoId, lockPath.trim()), 'lfs-lock-complete')}>{t(translate, 'lfs-lock')}</button>
                </div>
                {locks.map((lock) => <div className="lfs-manager__file" key={lock.id}>
                  <span className="mono">{lock.path} · {lock.owner}</span>
                  {forceUnlockId === lock.id ? (
                    <button type="button" className="tool-button tool-button--danger" disabled={busy} onClick={() => { setForceUnlockId(null); void perform(() => ipc.lfsUnlock(repoId, lock.id, true), 'lfs-unlock-complete'); }}>{t(translate, 'lfs-force-unlock-confirm')}</button>
                  ) : <div className="settings__row">
                    <button type="button" className="tool-button" disabled={busy} onClick={() => void perform(() => ipc.lfsUnlock(repoId, lock.id, false), 'lfs-unlock-complete')}>{t(translate, 'lfs-unlock')}</button>
                    <button type="button" className="tool-button tool-button--danger" disabled={busy} onClick={() => setForceUnlockId(lock.id)}>{t(translate, 'lfs-force-unlock')}</button>
                  </div>}
                </div>)}
              </section>

              <section className="stack stack--tight">
                <strong>{t(translate, 'lfs-advanced-title')}</strong>
                <label className="settings__label">{t(translate, 'lfs-include')}</label>
                <TextInput value={include} onChange={setInclude} placeholder="*.psd,assets/**" />
                <label className="settings__label">{t(translate, 'lfs-exclude')}</label>
                <TextInput value={exclude} onChange={setExclude} placeholder="*.tmp" />
                <label className="settings__label">{t(translate, 'lfs-endpoint')}</label>
                <TextInput value={endpoint} onChange={setEndpoint} placeholder={t(translate, 'lfs-endpoint-inherited')} monospace />
                {repository.endpointSource ? <div className="settings__hint">{ta(translate, 'lfs-endpoint-source', { source: repository.endpointSource })}</div> : null}
                <label className="settings__label">{t(translate, 'lfs-concurrent-transfers')}</label>
                <TextInput value={concurrentTransfers} onChange={setConcurrentTransfers} placeholder={repository.concurrentTransfers?.toString() ?? '8'} />
                <label className="settings__label">{t(translate, 'lfs-transfer-retries')}</label>
                <TextInput value={transferRetries} onChange={setTransferRetries} placeholder={repository.transferRetries?.toString() ?? '8'} />
                <Checkbox
                  checked={repository.locksVerify !== 'false'}
                  onChange={(enabled) => void perform(() => ipc.lfsUpdatePreferences(repoId, {
                    automaticDownload: repository.automaticDownload,
                    locksVerify: enabled ? 'true' : 'false'
                  }), 'lfs-preferences-saved')}
                  label={t(translate, 'lfs-locks-verify')}
                />
                <button type="button" className="tool-button" disabled={busy} onClick={() => void perform(() => ipc.lfsUpdatePreferences(repoId, {
                  automaticDownload: repository.automaticDownload,
                  fetchInclude: include,
                  fetchExclude: exclude,
                  endpoint,
                  concurrentTransfers: parseLfsNumberSetting(concurrentTransfers),
                  transferRetries: parseLfsNumberSetting(transferRetries)
                }), 'lfs-preferences-saved')}>
                  {t(translate, 'lfs-save-preferences')}
                </button>
              </section>

              <section className="stack stack--tight">
                <div className="settings__row">
                  <strong>{t(translate, 'lfs-cache-title')}</strong>
                  <button type="button" className="tool-button" disabled={busy} onClick={() => void perform(async () => setCache(await ipc.lfsCachePreview(repoId)))}>{t(translate, 'lfs-cache-preview')}</button>
                </div>
                {cache ? <>
                  <div className="settings__hint">{ta(translate, 'lfs-cache-size', { objects: cache.objects, size: formatBytes(cache.bytes) })}</div>
                  {cache.output ? <pre className="lfs-manager__output">{cache.output}</pre> : null}
                  {storageCleanupDisabled ? null : confirmPrune ? (
                    <div className="settings__row">
                      <span className="settings__hint">{t(translate, 'lfs-prune-confirmation')}</span>
                      <button type="button" className="tool-button tool-button--danger" disabled={busy} onClick={() => { setConfirmPrune(false); void perform(() => ipc.lfsPrune(repoId), 'lfs-prune-complete'); }}>{t(translate, 'lfs-prune-confirm')}</button>
                    </div>
                  ) : <button type="button" className="tool-button tool-button--danger" disabled={busy || cache.candidates === 0} onClick={() => setConfirmPrune(true)}>{t(translate, 'lfs-prune-preview')}</button>}
                </> : null}
              </section>

              <section className="stack stack--tight">
                <strong>{t(translate, 'lfs-diagnostics-title')}</strong>
                <div className="settings__hint">{t(translate, 'lfs-fsck-warning')}</div>
                <button
                  type="button"
                  className="tool-button"
                  disabled={busy || !environment?.installed}
                  onClick={() => void perform(async () => {
                    setDiagnosticOutput(await ipc.lfsFsck(repoId));
                  }, 'lfs-fsck-complete')}
                >
                  {t(translate, 'lfs-fsck-run')}
                </button>
                {diagnosticOutput ? <pre className="lfs-manager__output">{diagnosticOutput}</pre> : null}
              </section>
            </>
          ) : <div className="settings__hint">{t(translate, 'lfs-repository-unused')}</div>}
          {!repository?.usesLfs && environment?.installed ? (
            <section className="stack stack--tight">
              <strong>{t(translate, 'lfs-tracking-title')}</strong>
              <TextInput value={pattern} onChange={setPattern} placeholder={t(translate, 'lfs-pattern-placeholder')} testId="lfs-pattern" />
              <Checkbox checked={lockable} onChange={setLockable} label={t(translate, 'lfs-lockable')} testId="lfs-lockable" />
              <div className="settings__row">
                <button type="button" className="tool-button" disabled={busy || !pattern.trim()} onClick={() => void perform(() => ipc.lfsTrack(repoId, pattern.trim(), lockable), 'lfs-track-complete')}>
                  {t(translate, 'lfs-track')}
                </button>
                <button type="button" className="tool-button" disabled={busy || !pattern.trim()} onClick={() => void perform(() => ipc.lfsUntrack(repoId, pattern.trim()), 'lfs-untrack-complete')}>
                  {t(translate, 'lfs-untrack')}
                </button>
              </div>
            </section>
          ) : null}
          {error ? <div className="status-conflict" role="alert">{error}</div> : null}
          {notice ? <div className="settings__hint" role="status">{notice}</div> : null}
        </div>
      }
      footer={
        <button type="button" className="tool-button" onClick={closeOverlay}>
          {t(translate, 'dialog-cancel')}
        </button>
      }
    />
  );
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

function parseLfsNumberSetting(value: string): number {
  if (!value.trim()) return 0;
  const parsed = Number(value);
  return Number.isInteger(parsed) ? parsed : 255;
}

function describeError(reason: unknown): string {
  if (reason && typeof reason === 'object' && 'detail' in reason) {
    return String((reason as { detail: unknown }).detail);
  }
  return String(reason);
}
