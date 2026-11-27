import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import type {
  CredentialEntryView,
  CredentialTest,
  RepoCredentialSnapshot,
  RepoCredentialVerification,
  RepoRemoteCredentialInfo
} from '../../bridge/types';
import { DialogCard, Select, Spinner } from '../../components/controls';
import { Icon } from '../../components/Icon';
import { t, ta } from '../../i18n/strings';
import { draftFromEntry, draftToInput, type CredentialDraft } from '../settings/credentialsForm';
import {
  localDraftFromConfig,
  localDraftToInput,
  vaultDraftFromRemote,
  type RepoLocalDraft
} from './repoCredentialsForm';
import { LocalConfigTab } from './RepoCredentialsLocalTab';
import { VaultTab } from './RepoCredentialsVaultTab';

type ActiveTab = 'vault' | 'local';

export function RepoCredentialsDialog({ repoId }: { repoId: number }) {
  const translate = useStore((state) => state.t);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [snapshot, setSnapshot] = useState<RepoCredentialSnapshot | null>(null);
  const [selectedRemoteName, setSelectedRemoteName] = useState<string>('');
  const [activeTab, setActiveTab] = useState<ActiveTab>('vault');
  const tabInitializedRef = useRef(false);

  const [editing, setEditing] = useState<CredentialDraft | null>(null);
  const [deleting, setDeleting] = useState<CredentialEntryView | null>(null);
  const [vaultTest, setVaultTest] = useState<{ id: string; result: CredentialTest | null } | null>(
    null
  );

  const [localDraft, setLocalDraft] = useState<RepoLocalDraft | null>(null);
  const [localBaseline, setLocalBaseline] = useState<RepoLocalDraft | null>(null);
  const [verification, setVerification] = useState<RepoCredentialVerification | null>(null);
  const [remoteTest, setRemoteTest] = useState<CredentialTest | null>(null);

  const [busy, setBusy] = useState(false);
  const [testingRemote, setTestingRemote] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const applySnapshot = useCallback((next: RepoCredentialSnapshot, resetLocalDraft = true) => {
    setSnapshot(next);
    setSelectedRemoteName((current) => {
      if (current && next.remotes.some((r) => r.name === current)) return current;
      const firstHttp = next.remotes.find((r) => r.isHttp);
      return firstHttp?.name ?? next.remotes[0]?.name ?? '';
    });
    if (resetLocalDraft) {
      const fresh = localDraftFromConfig(next.localConfig);
      setLocalDraft(fresh);
      setLocalBaseline(fresh);
    }
    if (!tabInitializedRef.current) {
      tabInitializedRef.current = true;
      if (next.isWsl) {
        setActiveTab('local');
      }
    }
  }, []);

  useEffect(() => {
    let alive = true;
    void ipc
      .repoCredentialSnapshot(repoId)
      .then((next) => {
        if (alive) applySnapshot(next, true);
      })
      .catch((cause) => {
        if (alive) setError(ipc.describeError(cause).detail);
      });
    return () => {
      alive = false;
    };
  }, [applySnapshot, repoId]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (deleting) {
        setDeleting(null);
      } else if (editing) {
        setEditing(null);
      } else if (!busy) {
        closeOverlay();
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [busy, closeOverlay, deleting, editing]);

  const httpRemotes = useMemo(
    () => (snapshot?.remotes ?? []).filter((remote) => remote.isHttp),
    [snapshot?.remotes]
  );

  const selectedRemote = useMemo(
    () =>
      httpRemotes.find((remote) => remote.name === selectedRemoteName) ?? httpRemotes[0] ?? null,
    [httpRemotes, selectedRemoteName]
  );

  const refreshSnapshot = async (resetLocalDraft: boolean) => {
    const next = await ipc.repoCredentialSnapshot(repoId);
    applySnapshot(next, resetLocalDraft);
  };

  const saveVaultEntry = (draft: CredentialDraft) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    void ipc
      .saveCredential(draftToInput(draft))
      .then(() => refreshSnapshot(false))
      .then(() => setEditing(null))
      .catch((cause) => setError(ipc.describeError(cause).detail))
      .finally(() => setBusy(false));
  };

  const removeVaultEntry = (entry: CredentialEntryView) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    void ipc
      .deleteCredential(entry.id)
      .then(() => refreshSnapshot(false))
      .then(() => setDeleting(null))
      .catch((cause) => setError(ipc.describeError(cause).detail))
      .finally(() => setBusy(false));
  };

  const testVaultEntry = (entry: CredentialEntryView) => {
    setVaultTest({ id: entry.id, result: null });
    void ipc
      .testCredential(entry.id)
      .then((result) => setVaultTest({ id: entry.id, result }))
      .catch((cause) =>
        setVaultTest({
          id: entry.id,
          result: {
            ok: false,
            key: 'credential-test-failed',
            detail: ipc.describeError(cause).detail
          }
        })
      );
  };

  const saveLocalConfig = () => {
    if (!localDraft) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    void ipc
      .saveRepoCredentialConfig(repoId, localDraftToInput(localDraft, selectedRemote))
      .then((next) => {
        applySnapshot(next, true);
        setVerification(null);
        setRemoteTest(null);
        setNotice(t(translate, 'repo-creds-local-saved'));
      })
      .catch((cause) => setError(ipc.describeError(cause).detail))
      .finally(() => setBusy(false));
  };

  const runVerify = () => {
    if (!selectedRemote) return;
    setVerifying(true);
    setError(null);
    void ipc
      .verifyRepoCredential(repoId, selectedRemote.name)
      .then((result) => setVerification(result))
      .catch((cause) => setError(ipc.describeError(cause).detail))
      .finally(() => setVerifying(false));
  };

  const runRemoteTest = () => {
    if (!selectedRemote) return;
    setTestingRemote(true);
    setRemoteTest(null);
    setError(null);
    void ipc
      .testRepoRemoteCredential(repoId, selectedRemote.name)
      .then((result) => setRemoteTest(result))
      .catch((cause) =>
        setRemoteTest({
          ok: false,
          key: 'credential-test-failed',
          detail: ipc.describeError(cause).detail
        })
      )
      .finally(() => setTestingRemote(false));
  };

  const statusBadge = deriveStatusBadge(snapshot, selectedRemote, translate);
  const showConflictBanner =
    Boolean(selectedRemote?.hasUrlHelperConflict) &&
    snapshot !== null &&
    selectedRemote?.matchedVaultScope !== 'repository' &&
    snapshot.localConfig.mode !== 'isolated-store' &&
    snapshot.localConfig.mode !== 'isolated-prompt' &&
    !snapshot.localConfig.localHelpers.includes('');

  return (
    <DialogCard
      testId="repo-credentials-dialog"
      width={720}
      title={t(translate, 'repo-creds-title')}
      subtitle={snapshot?.repoPath}
      icon={<Icon name="key" size={16} />}
      onBackdrop={() => {
        if (!busy) closeOverlay();
      }}
      body={
        <div className="repo-creds stack">
          {snapshot ? (
            <>
              <div className="repo-creds__summary">
                <div className="repo-creds__remote">
                  <span className="settings__label">{t(translate, 'repo-creds-remote-label')}</span>
                  {httpRemotes.length === 0 ? (
                    <span className="settings__hint" data-testid="repo-creds-no-http-remotes">
                      {t(translate, 'repo-creds-no-http-remotes')}
                    </span>
                  ) : httpRemotes.length === 1 ? (
                    <span className="mono repo-creds__remote-url" data-testid="repo-creds-remote-url">
                      {selectedRemote?.name} · {selectedRemote?.url}
                    </span>
                  ) : (
                    <Select<string>
                      value={selectedRemote?.name ?? ''}
                      options={httpRemotes.map((remote) => ({
                        value: remote.name,
                        label: `${remote.name} · ${remote.url}`
                      }))}
                      onChange={(name) => {
                        setSelectedRemoteName(name);
                        setVerification(null);
                        setRemoteTest(null);
                      }}
                      testId="repo-creds-remote-select"
                    />
                  )}
                </div>
                <span
                  className={`repo-creds__badge repo-creds__badge--${statusBadge.tone}`}
                  data-testid="repo-creds-status-badge"
                >
                  {statusBadge.label}
                </span>
              </div>

              {showConflictBanner ? (
                <div className="repo-creds__warning-banner" data-testid="repo-creds-conflict-banner">
                  <Icon name="triangle-alert" size={14} />
                  <span>{t(translate, 'repo-creds-conflict-banner')}</span>
                </div>
              ) : null}

              <div className="repo-creds__tabs" role="tablist">
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTab === 'vault'}
                  className={`repo-creds__tab${activeTab === 'vault' ? ' is-active' : ''}`}
                  data-testid="repo-creds-tab-vault"
                  onClick={() => setActiveTab('vault')}
                >
                  {t(translate, 'repo-creds-tab-vault')}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTab === 'local'}
                  className={`repo-creds__tab${activeTab === 'local' ? ' is-active' : ''}`}
                  data-testid="repo-creds-tab-local"
                  onClick={() => setActiveTab('local')}
                >
                  {t(translate, 'repo-creds-tab-local')}
                </button>
              </div>

              {error ? (
                <div className="status-conflict" role="alert" data-testid="repo-creds-error">
                  {error}
                </div>
              ) : null}
              {notice ? (
                <div className="settings__hint" role="status" data-testid="repo-creds-notice">
                  {notice}
                </div>
              ) : null}

              {activeTab === 'vault' ? (
                <VaultTab
                  snapshot={snapshot}
                  selectedRemote={selectedRemote}
                  editing={editing}
                  deleting={deleting}
                  vaultTest={vaultTest}
                  busy={busy}
                  onStartAdd={() =>
                    setEditing(
                      vaultDraftFromRemote(
                        selectedRemote,
                        snapshot.vault.settings.defaultStorage,
                        snapshot.repoPath
                      )
                    )
                  }
                  onEdit={(entry) => setEditing(draftFromEntry(entry))}
                  onChangeDraft={setEditing}
                  onCancelEdit={() => setEditing(null)}
                  onSaveDraft={saveVaultEntry}
                  onAskDelete={setDeleting}
                  onCancelDelete={() => setDeleting(null)}
                  onConfirmDelete={removeVaultEntry}
                  onTestEntry={testVaultEntry}
                />
              ) : localDraft && localBaseline ? (
                <LocalConfigTab
                  snapshot={snapshot}
                  selectedRemote={selectedRemote}
                  draft={localDraft}
                  baseline={localBaseline}
                  busy={busy}
                  verifying={verifying}
                  testingRemote={testingRemote}
                  verification={verification}
                  remoteTest={remoteTest}
                  onChangeDraft={(next) => {
                    setLocalDraft(next);
                    setNotice(null);
                  }}
                  onSave={saveLocalConfig}
                  onVerify={runVerify}
                  onTestRemote={runRemoteTest}
                />
              ) : null}
            </>
          ) : error ? (
            <div className="status-conflict" role="alert" data-testid="repo-creds-error">
              {error}
            </div>
          ) : (
            <div className="settings__row">
              <Spinner size={14} />
              <span className="settings__hint">…</span>
            </div>
          )}
        </div>
      }
      footer={
        <button
          type="button"
          className="tool-button"
          disabled={busy}
          onClick={closeOverlay}
          data-testid="repo-creds-close"
        >
          {t(translate, 'credential-close')}
        </button>
      }
    />
  );
}

function deriveStatusBadge(
  snapshot: RepoCredentialSnapshot | null,
  selectedRemote: RepoRemoteCredentialInfo | null,
  translate: (key: string) => string
): { tone: 'vault' | 'isolated' | 'http-path' | 'inherit'; label: string } {
  if (!snapshot) {
    return { tone: 'inherit', label: '…' };
  }
  if (!snapshot.isWsl && selectedRemote?.matchedVaultScope === 'repository') {
    return {
      tone: 'vault',
      label: ta(translate, 'repo-creds-status-vault', {
        username: selectedRemote.matchedVaultUsername ?? ''
      })
    };
  }
  switch (snapshot.localConfig.mode) {
    case 'isolated-store':
    case 'isolated-prompt':
      return { tone: 'isolated', label: t(translate, 'repo-creds-status-isolated') };
    case 'use-http-path':
      return { tone: 'http-path', label: t(translate, 'repo-creds-status-http-path') };
    case 'custom':
      return { tone: 'http-path', label: t(translate, 'repo-creds-status-custom') };
    case 'inherit':
    default:
      return { tone: 'inherit', label: t(translate, 'repo-creds-status-inherit') };
  }
}
