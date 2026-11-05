import { useEffect, useMemo, useState } from 'react';

import * as ipc from '../../bridge/ipc';
import type {
  CredentialEntryView,
  CredentialSnapshot,
  CredentialStorage,
  CredentialTest
} from '../../bridge/types';
import {
  DialogCard,
  EmptyState,
  IconButton,
  Select,
  TextInput,
  Checkbox
} from '../../components/controls';
import { Icon } from '../../components/Icon';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import {
  draftFromEntry,
  draftToInput,
  emptyDraft,
  isDraftComplete,
  storageChoices,
  type CredentialDraft
} from './credentialsForm';

/**
 * Credential settings.
 *
 * Three things are separated on purpose, because they fail independently: the
 * backends that can hold a secret, the preferences that decide what a prompt
 * offers, and the list of remembered accounts. A backend that is unavailable is
 * reported with its reason instead of being hidden, so "remember" never appears
 * to work by accident.
 */
export function CredentialsSettings() {
  const translate = useStore((state) => state.t);
  const [snapshot, setSnapshot] = useState<CredentialSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<CredentialDraft | null>(null);
  const [deleting, setDeleting] = useState<CredentialEntryView | null>(null);
  const [test, setTest] = useState<{ id: string; result: CredentialTest | null } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const apply = (next: CredentialSnapshot) => {
    setSnapshot(next);
    setError(null);
  };

  useEffect(() => {
    void ipc
      .credentialSnapshot()
      .then(apply)
      .catch((cause) => setError(ipc.describeError(cause).detail));
  }, []);

  const patch = (values: Parameters<typeof ipc.updateCredentialSettings>[0]) => {
    void ipc
      .updateCredentialSettings(values)
      .then(apply)
      .catch((cause) => setError(ipc.describeError(cause).detail));
  };

  const save = (draft: CredentialDraft) => {
    setBusy(true);
    void ipc
      .saveCredential(draftToInput(draft))
      .then((next) => {
        apply(next);
        setEditing(null);
      })
      .catch((cause) => setError(ipc.describeError(cause).detail))
      .finally(() => setBusy(false));
  };

  const remove = (entry: CredentialEntryView) => {
    setBusy(true);
    void ipc
      .deleteCredential(entry.id)
      .then((next) => {
        apply(next);
        setDeleting(null);
      })
      .catch((cause) => setError(ipc.describeError(cause).detail))
      .finally(() => setBusy(false));
  };

  const runTest = (entry: CredentialEntryView) => {
    setTest({ id: entry.id, result: null });
    void ipc
      .testCredential(entry.id)
      .then((result) => setTest({ id: entry.id, result }))
      .catch((cause) =>
        setTest({
          id: entry.id,
          result: {
            ok: false,
            key: 'credential-test-failed',
            detail: ipc.describeError(cause).detail
          }
        })
      );
  };

  const importStore = () => {
    setBusy(true);
    setNotice(null);
    void ipc
      .importGitCredentials()
      .then((outcome) => {
        apply(outcome.snapshot);
        setNotice(
          ta(translate, 'credentials-import-done', {
            imported: outcome.imported,
            skipped: outcome.skipped,
            source: outcome.source
          })
        );
        if (outcome.warnings.length > 0) {
          setError(outcome.warnings.join(' '));
        }
      })
      .catch((cause) => setError(ipc.describeError(cause).detail))
      .finally(() => setBusy(false));
  };

  if (!snapshot) {
    return (
      <>
        <div className="settings__heading">{t(translate, 'settings-credentials')}</div>
        <p className="settings__description">{t(translate, 'credentials-description')}</p>
        {error ? <div className="settings__hint">{error}</div> : null}
      </>
    );
  }

  const { settings, backends } = snapshot;
  const defaultStorageOptions = storageChoices(
    snapshot.keychainAvailable,
    snapshot.plaintextAvailable
  ).map((storage) => ({ value: storage, label: storageLabel(storage, translate) }));

  return (
    <>
      <div className="settings__heading">{t(translate, 'settings-credentials')}</div>
      <p className="settings__description">{t(translate, 'credentials-description')}</p>
      {error ? (
        <div className="settings__hint credentials__error" data-testid="credentials-error">
          {error}
        </div>
      ) : null}
      {notice ? (
        <div className="settings__hint" data-testid="credentials-notice">
          {notice}
        </div>
      ) : null}

      <div
        className="settings__field credentials__backends"
        data-testid="settings-field-credentials-backends"
      >
        <span className="settings__label">{t(translate, 'credentials-backends-title')}</span>
        {backends.map((backend) => (
          <div className="credentials__backend" key={backend.storage}>
            <span
              className={`credentials__status${backend.available ? ' is-available' : ''}`}
              data-testid={`credentials-backend-${backend.storage}`}
            >
              {t(
                translate,
                backend.available
                  ? 'credentials-backend-available'
                  : 'credentials-backend-unavailable'
              )}
            </span>
            <span className="credentials__backend-name">
              {storageLabel(backend.storage, translate)}
            </span>
            {backend.detail ? (
              <span className="settings__hint credentials__backend-detail">{backend.detail}</span>
            ) : null}
          </div>
        ))}
        {snapshot.plaintextPath && settings.allowPlaintext ? (
          <div className="settings__hint credentials__warning">
            {ta(translate, 'credentials-plaintext-warning', { path: snapshot.plaintextPath })}
          </div>
        ) : null}
      </div>

      <div
        className="settings__field settings__search-target"
        data-testid="settings-field-credentials-auto-fill"
      >
        <span className="settings__label">{t(translate, 'credentials-auto-fill-title')}</span>
        <Checkbox
          checked={settings.autoFill}
          label={t(translate, 'credentials-auto-fill-description')}
          testId="credentials-auto-fill"
          onChange={(value) => patch({ autoFill: value })}
        />
      </div>

      <div className="settings__field" data-testid="settings-field-credentials-default-storage">
        <span className="settings__label">{t(translate, 'credentials-default-storage-title')}</span>
        <Select<CredentialStorage>
          value={settings.defaultStorage}
          options={defaultStorageOptions}
          onChange={(value) => patch({ defaultStorage: value })}
          testId="credentials-default-storage"
        />
        <div className="settings__hint">
          {t(translate, 'credentials-default-storage-description')}
        </div>
      </div>

      <div className="settings__field" data-testid="settings-field-credentials-plaintext">
        <span className="settings__label">{t(translate, 'credentials-allow-plaintext-title')}</span>
        <Checkbox
          checked={settings.allowPlaintext}
          label={t(translate, 'credentials-allow-plaintext-description')}
          testId="credentials-allow-plaintext"
          onChange={(value) => patch({ allowPlaintext: value })}
        />
        <div className="settings__hint credentials__warning">
          {t(translate, 'credentials-allow-plaintext-warning')}
        </div>
      </div>

      <div className="settings__field" data-testid="settings-field-credentials-helper">
        <span className="settings__label">{t(translate, 'credentials-helper-title')}</span>
        <Checkbox
          checked={settings.useHelper}
          label={t(translate, 'credentials-helper-description')}
          testId="credentials-helper"
          onChange={(value) => patch({ useHelper: value })}
        />
        <div className="settings__hint">{t(translate, 'credentials-helper-warning')}</div>
      </div>

      <div
        className="settings__field settings__search-target credentials__list"
        data-testid="settings-field-credentials-list"
      >
        <span className="settings__label">{t(translate, 'credentials-list-title')}</span>
        {snapshot.entries.length === 0 ? (
          <EmptyState
            icon={<Icon name="key" size={18} />}
            message={t(translate, 'credentials-list-empty')}
            testId="credentials-empty"
          />
        ) : (
          <div className="credentials__table" data-testid="credentials-table">
            <div className="credentials__row credentials__row--head" role="row">
              <span>{t(translate, 'credentials-column-host')}</span>
              <span>{t(translate, 'credentials-column-username')}</span>
              <span>{t(translate, 'credentials-column-storage')}</span>
              <span>{t(translate, 'credentials-column-status')}</span>
              <span aria-hidden="true" />
            </div>
            {snapshot.entries.map((entry) => {
              const result = test?.id === entry.id ? test.result : null;
              return (
                <div
                  className="credentials__row"
                  key={entry.id}
                  data-testid={`credentials-entry-${entry.id}`}
                >
                  <span className="credentials__host" title={`${entry.protocol}://${entry.host}`}>
                    <span className="credentials__address">
                      {entry.host}
                      {entry.pathPrefix ? (
                        <span className="credentials__path">/{entry.pathPrefix}</span>
                      ) : null}
                    </span>
                    <span className="credentials__scope">
                      {entry.scopeLabel ?? t(translate, 'credentials-scope-all')}
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
                        testId={`credentials-test-${entry.id}`}
                        onClick={() => runTest(entry)}
                      />
                    ) : null}
                    <IconButton
                      icon={<Icon name="pencil" size={14} />}
                      tooltip={t(translate, 'credentials-action-edit')}
                      testId={`credentials-edit-${entry.id}`}
                      onClick={() => setEditing(draftFromEntry(entry))}
                    />
                    <IconButton
                      icon={<Icon name="trash-2" size={14} />}
                      tooltip={t(translate, 'credentials-action-delete')}
                      testId={`credentials-delete-${entry.id}`}
                      onClick={() => setDeleting(entry)}
                    />
                  </span>
                  {result ? (
                    <div className="credentials__test-result" data-testid="credentials-test-result">
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
        <div className="credentials__footer">
          <button
            type="button"
            className="credentials__action credentials__action--primary"
            data-testid="credentials-add"
            onClick={() => setEditing(emptyDraft(settings.defaultStorage, null))}
          >
            <Icon name="plus" size={14} />
            {t(translate, 'credentials-add')}
          </button>
          <button
            type="button"
            className="credentials__action"
            data-testid="credentials-import"
            disabled={busy}
            onClick={importStore}
          >
            <Icon name="download" size={14} />
            {t(translate, 'credentials-import')}
          </button>
        </div>
        <div className="settings__hint">{t(translate, 'credentials-import-hint')}</div>
      </div>

      {editing ? (
        <CredentialForm
          draft={editing}
          snapshot={snapshot}
          busy={busy}
          onChange={setEditing}
          onCancel={() => setEditing(null)}
          onSave={save}
        />
      ) : null}

      {deleting ? (
        <DialogCard
          testId="credentials-delete-dialog"
          title={t(translate, 'credentials-delete-title')}
          subtitle={`${deleting.username}@${deleting.host}`}
          onBackdrop={() => setDeleting(null)}
          body={<p className="settings__hint">{t(translate, 'credentials-delete-body')}</p>}
          footer={
            <>
              <button type="button" className="tool-button" onClick={() => setDeleting(null)}>
                {t(translate, 'dialog-cancel')}
              </button>
              <button
                type="button"
                className="tool-button tool-button--danger"
                disabled={busy}
                data-testid="credentials-delete-confirm"
                onClick={() => remove(deleting)}
              >
                {t(translate, 'credentials-delete-confirm')}
              </button>
            </>
          }
        />
      ) : null}
    </>
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

function CredentialForm({
  draft,
  snapshot,
  busy,
  onChange,
  onCancel,
  onSave
}: {
  draft: CredentialDraft;
  snapshot: CredentialSnapshot;
  busy: boolean;
  onChange: (draft: CredentialDraft) => void;
  onCancel: () => void;
  onSave: (draft: CredentialDraft) => void;
}) {
  const translate = useStore((state) => state.t);
  const choices = useMemo(
    () => storageChoices(snapshot.keychainAvailable, snapshot.plaintextAvailable),
    [snapshot.keychainAvailable, snapshot.plaintextAvailable]
  );
  const set = (patch: Partial<CredentialDraft>) => onChange({ ...draft, ...patch });
  const editing = draft.id !== null;
  // A backend can disappear between opening the page and opening this form.
  // Submitting the one that is gone would only fail in the backend.
  const effectiveStorage = choices.includes(draft.storage)
    ? draft.storage
    : (choices[0] ?? 'session');

  return (
    <DialogCard
      testId="credentials-form"
      title={t(translate, editing ? 'credentials-form-title-edit' : 'credentials-form-title-add')}
      width={480}
      onBackdrop={onCancel}
      body={
        <div className="stack">
          <div className="settings__field">
            <span className="settings__label">{t(translate, 'credentials-form-protocol')}</span>
            <Select<'https' | 'http'>
              value={draft.protocol}
              options={[
                { value: 'https', label: 'https' },
                { value: 'http', label: 'http' }
              ]}
              onChange={(value) => set({ protocol: value })}
              testId="credentials-form-protocol"
            />
          </div>
          <label className="settings__field">
            <span className="settings__label">{t(translate, 'credentials-form-host')}</span>
            <TextInput
              value={draft.host}
              onChange={(value) => set({ host: value })}
              autoFocus
              testId="credentials-form-host"
            />
            <span className="settings__hint">{t(translate, 'credentials-form-host-hint')}</span>
          </label>
          <label className="settings__field">
            <span className="settings__label">{t(translate, 'credentials-form-path')}</span>
            <TextInput
              value={draft.pathPrefix}
              onChange={(value) => set({ pathPrefix: value })}
              testId="credentials-form-path"
            />
            <span className="settings__hint">{t(translate, 'credentials-form-path-hint')}</span>
          </label>
          <label className="settings__field">
            <span className="settings__label">{t(translate, 'credentials-form-username')}</span>
            <TextInput
              value={draft.username}
              onChange={(value) => set({ username: value })}
              testId="credentials-form-username"
            />
          </label>
          <label className="settings__field">
            <span className="settings__label">{t(translate, 'credentials-form-password')}</span>
            <TextInput
              value={draft.password}
              onChange={(value) => set({ password: value })}
              type="password"
              testId="credentials-form-password"
            />
            {editing ? (
              <span className="settings__hint">
                {t(translate, 'credentials-form-password-edit-hint')}
              </span>
            ) : null}
          </label>
          <div className="settings__field">
            <span className="settings__label">{t(translate, 'credentials-form-storage')}</span>
            <Select<CredentialStorage>
              value={effectiveStorage}
              options={choices.map((storage) => ({
                value: storage,
                label: storageLabel(storage, translate)
              }))}
              onChange={(value) => set({ storage: value })}
              testId="credentials-form-storage"
            />
            {effectiveStorage === 'plaintext' ? (
              <span className="settings__hint credentials__warning">
                {t(translate, 'credential-storage-plaintext-hint')}
              </span>
            ) : null}
          </div>
          {draft.repository ? (
            <div className="settings__field">
              <span className="settings__label">{t(translate, 'credentials-form-scope')}</span>
              <Checkbox
                checked={draft.repositoryOnly}
                label={ta(translate, 'credentials-form-scope-repository', {
                  repository: draft.repository
                })}
                testId="credentials-form-repository-only"
                onChange={(value) => set({ repositoryOnly: value })}
              />
            </div>
          ) : null}
        </div>
      }
      footer={
        <>
          <button type="button" className="tool-button" onClick={onCancel}>
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={busy || !isDraftComplete(draft)}
            onClick={() => onSave({ ...draft, storage: effectiveStorage })}
            data-testid="credentials-form-save"
          >
            {t(translate, 'credentials-form-save')}
          </button>
        </>
      }
    />
  );
}
