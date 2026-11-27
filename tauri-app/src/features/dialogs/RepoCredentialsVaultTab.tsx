import { useMemo } from 'react';

import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import type {
  CredentialEntryView,
  CredentialStorage,
  CredentialTest,
  RepoCredentialSnapshot,
  RepoRemoteCredentialInfo
} from '../../bridge/types';
import { EmptyState, IconButton, Select, TextInput } from '../../components/controls';
import { Icon } from '../../components/Icon';
import { t, ta } from '../../i18n/strings';
import { hasNativeWindow } from '../../platform';
import { isDraftComplete, storageChoices, type CredentialDraft } from '../settings/credentialsForm';

export function VaultTab({
  snapshot,
  selectedRemote,
  editing,
  deleting,
  vaultTest,
  busy,
  onStartAdd,
  onEdit,
  onChangeDraft,
  onCancelEdit,
  onSaveDraft,
  onAskDelete,
  onCancelDelete,
  onConfirmDelete,
  onTestEntry
}: {
  snapshot: RepoCredentialSnapshot;
  selectedRemote: RepoRemoteCredentialInfo | null;
  editing: CredentialDraft | null;
  deleting: CredentialEntryView | null;
  vaultTest: { id: string; result: CredentialTest | null } | null;
  busy: boolean;
  onStartAdd: () => void;
  onEdit: (entry: CredentialEntryView) => void;
  onChangeDraft: (draft: CredentialDraft) => void;
  onCancelEdit: () => void;
  onSaveDraft: (draft: CredentialDraft) => void;
  onAskDelete: (entry: CredentialEntryView) => void;
  onCancelDelete: () => void;
  onConfirmDelete: (entry: CredentialEntryView) => void;
  onTestEntry: (entry: CredentialEntryView) => void;
}) {
  const translate = useStore((state) => state.t);

  if (snapshot.isWsl) {
    return (
      <div className="stack">
        <div className="settings__hint" data-testid="repo-creds-wsl-note">
          {t(translate, 'repo-creds-vault-wsl-unsupported')}
        </div>
      </div>
    );
  }

  return (
    <div className="stack">
      <p className="settings__hint">{t(translate, 'repo-creds-vault-description')}</p>

      {snapshot.vaultEntries.length === 0 ? (
        <EmptyState
          icon={<Icon name="key" size={18} />}
          message={t(translate, 'repo-creds-vault-empty')}
          testId="repo-creds-vault-empty"
        />
      ) : (
        <div className="credentials__table" data-testid="repo-creds-vault-list">
          <div className="credentials__row credentials__row--head" role="row">
            <span>{t(translate, 'credentials-column-host')}</span>
            <span>{t(translate, 'credentials-column-username')}</span>
            <span>{t(translate, 'credentials-column-storage')}</span>
            <span>{t(translate, 'credentials-column-status')}</span>
            <span aria-hidden="true" />
          </div>
          {snapshot.vaultEntries.map((entry) => {
            const result = vaultTest?.id === entry.id ? vaultTest.result : null;
            return (
              <div
                className="credentials__row"
                key={entry.id}
                data-testid={`repo-creds-vault-entry-${entry.id}`}
              >
                <span className="credentials__host" title={`${entry.protocol}://${entry.host}`}>
                  <span className="credentials__address">
                    {entry.host}
                    {entry.pathPrefix ? (
                      <span className="credentials__path">/{entry.pathPrefix}</span>
                    ) : null}
                  </span>
                </span>
                <span className="credentials__username">{entry.username}</span>
                <span>{storageLabel(entry.storage, translate)}</span>
                <span className={entry.failing || !entry.secretPresent ? 'credentials__bad' : ''}>
                  {statusLabel(entry, translate)}
                </span>
                <span className="credentials__actions">
                  {entry.pathPrefix ? (
                    <IconButton
                      icon={<Icon name="refresh-cw" size={14} />}
                      tooltip={t(translate, 'credentials-action-test')}
                      testId={`repo-creds-vault-test-${entry.id}`}
                      onClick={() => onTestEntry(entry)}
                    />
                  ) : null}
                  <IconButton
                    icon={<Icon name="pencil" size={14} />}
                    tooltip={t(translate, 'credentials-action-edit')}
                    testId={`repo-creds-vault-edit-${entry.id}`}
                    onClick={() => onEdit(entry)}
                  />
                  <IconButton
                    icon={<Icon name="trash-2" size={14} />}
                    tooltip={t(translate, 'credentials-action-delete')}
                    testId={`repo-creds-vault-delete-${entry.id}`}
                    onClick={() => onAskDelete(entry)}
                  />
                </span>
                {result ? (
                  <div className="credentials__test-result" data-testid="repo-creds-vault-test-result">
                    {result.ok
                      ? t(translate, result.key)
                      : `${t(translate, result.key)}${result.detail ? `: ${result.detail}` : ''}`}
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      )}

      {deleting ? (
        <div className="repo-creds__inline-card" data-testid="repo-creds-vault-delete-confirm-card">
          <strong>{t(translate, 'credentials-delete-title')}</strong>
          <div className="settings__hint">
            {deleting.username}@{deleting.host} — {t(translate, 'credentials-delete-body')}
          </div>
          <div className="settings__row">
            <button type="button" className="tool-button" disabled={busy} onClick={onCancelDelete}>
              {t(translate, 'dialog-cancel')}
            </button>
            <button
              type="button"
              className="tool-button tool-button--danger"
              disabled={busy}
              data-testid="repo-creds-vault-delete-confirm"
              onClick={() => onConfirmDelete(deleting)}
            >
              {t(translate, 'credentials-delete-confirm')}
            </button>
          </div>
        </div>
      ) : null}

      {editing ? (
        <InlineVaultForm
          draft={editing}
          snapshot={snapshot}
          busy={busy}
          onChange={onChangeDraft}
          onCancel={onCancelEdit}
          onSave={onSaveDraft}
        />
      ) : (
        <div className="credentials__footer">
          <button
            type="button"
            className="credentials__action credentials__action--primary"
            data-testid="repo-creds-vault-add"
            disabled={busy}
            onClick={onStartAdd}
          >
            <Icon name="plus" size={14} />
            {t(translate, 'repo-creds-vault-add')}
          </button>
          {hasNativeWindow ? (
            <button
              type="button"
              className="tool-button"
              data-testid="repo-creds-open-global"
              onClick={() => void ipc.openSettingsWindow('credentials')}
            >
              {t(translate, 'repo-creds-vault-open-global')}
            </button>
          ) : null}
        </div>
      )}

      {snapshot.fallbackVaultEntries.length > 0 ? (
        <div className="repo-creds__fallback stack stack--tight" data-testid="repo-creds-fallback-list">
          <span className="settings__label">{t(translate, 'repo-creds-vault-fallback-title')}</span>
          <span className="settings__hint">
            {selectedRemote
              ? ta(translate, 'repo-creds-vault-fallback-hint', { remote: selectedRemote.name })
              : null}
          </span>
          {snapshot.fallbackVaultEntries.map((entry) => (
            <div className="repo-creds__fallback-row" key={entry.id}>
              <span className="mono">
                {entry.protocol}://{entry.host}
                {entry.pathPrefix ? `/${entry.pathPrefix}` : ''}
              </span>
              <span>{entry.username}</span>
              <span className="settings__hint">{storageLabel(entry.storage, translate)}</span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function InlineVaultForm({
  draft,
  snapshot,
  busy,
  onChange,
  onCancel,
  onSave
}: {
  draft: CredentialDraft;
  snapshot: RepoCredentialSnapshot;
  busy: boolean;
  onChange: (draft: CredentialDraft) => void;
  onCancel: () => void;
  onSave: (draft: CredentialDraft) => void;
}) {
  const translate = useStore((state) => state.t);
  const choices = useMemo(
    () => storageChoices(snapshot.vault.keychainAvailable, snapshot.vault.plaintextAvailable),
    [snapshot.vault.keychainAvailable, snapshot.vault.plaintextAvailable]
  );
  const set = (patch: Partial<CredentialDraft>) => onChange({ ...draft, ...patch });
  const isEdit = draft.id !== null;
  const effectiveStorage = choices.includes(draft.storage)
    ? draft.storage
    : (choices[0] ?? 'session');

  return (
    <div className="repo-creds__inline-card stack" data-testid="repo-creds-vault-form">
      <strong>
        {t(translate, isEdit ? 'credentials-form-title-edit' : 'credentials-form-title-add')}
      </strong>
      <div className="repo-creds__grid">
        <div className="settings__field">
          <span className="settings__label">{t(translate, 'credentials-form-protocol')}</span>
          <Select<'https' | 'http'>
            value={draft.protocol}
            options={[
              { value: 'https', label: 'https' },
              { value: 'http', label: 'http' }
            ]}
            onChange={(value) => set({ protocol: value })}
            testId="repo-creds-vault-protocol"
          />
        </div>
        <label className="settings__field">
          <span className="settings__label">{t(translate, 'credentials-form-host')}</span>
          <TextInput
            value={draft.host}
            onChange={(value) => set({ host: value })}
            testId="repo-creds-vault-host"
          />
        </label>
      </div>
      <label className="settings__field">
        <span className="settings__label">{t(translate, 'credentials-form-path')}</span>
        <TextInput
          value={draft.pathPrefix}
          onChange={(value) => set({ pathPrefix: value })}
          testId="repo-creds-vault-path"
        />
        <span className="settings__hint">{t(translate, 'credentials-form-path-hint')}</span>
      </label>
      <div className="repo-creds__grid">
        <label className="settings__field">
          <span className="settings__label">{t(translate, 'credentials-form-username')}</span>
          <TextInput
            value={draft.username}
            onChange={(value) => set({ username: value })}
            autoFocus
            testId="repo-creds-vault-username"
          />
        </label>
        <label className="settings__field">
          <span className="settings__label">{t(translate, 'credentials-form-password')}</span>
          <TextInput
            value={draft.password}
            onChange={(value) => set({ password: value })}
            type="password"
            testId="repo-creds-vault-password"
          />
          {isEdit ? (
            <span className="settings__hint">{t(translate, 'credentials-form-password-edit-hint')}</span>
          ) : null}
        </label>
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'credentials-form-storage')}</span>
        <Select<CredentialStorage>
          value={effectiveStorage}
          options={choices.map((storage) => ({
            value: storage,
            label: storageLabel(storage, translate)
          }))}
          onChange={(value) => set({ storage: value })}
          testId="repo-creds-vault-storage"
        />
      </div>
      <div className="settings__row">
        <button
          type="button"
          className="tool-button"
          disabled={busy}
          onClick={onCancel}
          data-testid="repo-creds-vault-cancel"
        >
          {t(translate, 'dialog-cancel')}
        </button>
        <button
          type="button"
          className="tool-button tool-button--primary"
          disabled={busy || !isDraftComplete(draft)}
          onClick={() => onSave({ ...draft, repositoryOnly: true, storage: effectiveStorage })}
          data-testid="repo-creds-vault-save"
        >
          {t(translate, 'credentials-form-save')}
        </button>
      </div>
    </div>
  );
}

function storageLabel(storage: CredentialStorage, translate: (key: string) => string): string {
  return t(translate, `credential-storage-${storage}`);
}

function statusLabel(entry: CredentialEntryView, translate: (key: string) => string): string {
  if (!entry.secretPresent) return t(translate, 'credentials-status-missing-secret');
  if (entry.failing) return t(translate, 'credentials-status-failing');
  if (!entry.lastUsedAt) return t(translate, 'credentials-status-never-used');
  return t(translate, 'credentials-status-ok');
}
