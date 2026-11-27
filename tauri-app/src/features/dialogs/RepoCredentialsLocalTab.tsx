import { useStore } from '../../app/store';
import type {
  CredentialTest,
  RemoteUsernameAction,
  RepoCredentialMode,
  RepoCredentialSnapshot,
  RepoCredentialVerification,
  RepoRemoteCredentialInfo
} from '../../bridge/types';
import { Checkbox, Select, Spinner, TextInput } from '../../components/controls';
import { Icon } from '../../components/Icon';
import { t, ta } from '../../i18n/strings';
import {
  isLocalDraftComplete,
  isLocalDraftDirty,
  previewRemoteUrlWithUsername,
  validateLocalDraft,
  type LocalDraftIssue,
  type RepoLocalDraft
} from './repoCredentialsForm';

export function LocalConfigTab({
  snapshot,
  selectedRemote,
  draft,
  baseline,
  busy,
  verifying,
  testingRemote,
  verification,
  remoteTest,
  onChangeDraft,
  onSave,
  onVerify,
  onTestRemote
}: {
  snapshot: RepoCredentialSnapshot;
  selectedRemote: RepoRemoteCredentialInfo | null;
  draft: RepoLocalDraft;
  baseline: RepoLocalDraft;
  busy: boolean;
  verifying: boolean;
  testingRemote: boolean;
  verification: RepoCredentialVerification | null;
  remoteTest: CredentialTest | null;
  onChangeDraft: (draft: RepoLocalDraft) => void;
  onSave: () => void;
  onVerify: () => void;
  onTestRemote: () => void;
}) {
  const translate = useStore((state) => state.t);
  const issues = validateLocalDraft(draft);
  const canSave =
    !busy && isLocalDraftDirty(draft, baseline) && isLocalDraftComplete(draft);

  const modeOptions: { value: RepoCredentialMode; label: string }[] = [
    { value: 'inherit', label: t(translate, 'repo-creds-mode-inherit') },
    { value: 'use-http-path', label: t(translate, 'repo-creds-mode-use-http-path') },
    { value: 'isolated-store', label: t(translate, 'repo-creds-mode-isolated-store') },
    { value: 'isolated-prompt', label: t(translate, 'repo-creds-mode-isolated-prompt') }
  ];
  if (snapshot.localConfig.mode === 'custom' || draft.mode === 'custom') {
    modeOptions.push({ value: 'custom', label: t(translate, 'repo-creds-mode-custom') });
  }

  const handleModeChange = (nextMode: RepoCredentialMode) => {
    const nextUsername =
      nextMode !== 'inherit' && !draft.username.trim() && selectedRemote?.urlUsername
        ? selectedRemote.urlUsername
        : draft.username;
    const nextUseHttpPath =
      nextMode === 'use-http-path'
        ? true
        : nextMode === 'inherit'
          ? false
          : draft.mode === 'inherit'
            ? true
            : draft.useHttpPath;
    onChangeDraft({
      ...draft,
      mode: nextMode,
      username: nextUsername,
      useHttpPath: nextUseHttpPath
    });
  };

  const urlPreview =
    selectedRemote?.isHttp
      ? previewRemoteUrlWithUsername(
          selectedRemote.url,
          draft.remoteUrlUsernameAction,
          draft.username
        )
      : null;

  return (
    <div className="stack">
      <p className="settings__hint">{t(translate, 'repo-creds-local-description')}</p>

      <div className="settings__field">
        <span className="settings__label">{t(translate, 'repo-creds-mode-label')}</span>
        <Select<RepoCredentialMode>
          value={draft.mode}
          options={modeOptions}
          onChange={handleModeChange}
          testId="repo-creds-mode"
        />
        <span className="settings__hint">{t(translate, `repo-creds-mode-hint-${draft.mode}`)}</span>
      </div>

      {draft.mode !== 'inherit' ? (
        <>
          <label className="settings__field">
            <span className="settings__label">
              {t(translate, 'repo-creds-local-username-label')}
            </span>
            <TextInput
              value={draft.username}
              onChange={(value) => onChangeDraft({ ...draft, username: value })}
              placeholder={
                selectedRemote?.urlUsername ??
                t(translate, 'repo-creds-local-username-placeholder')
              }
              monospace
              testId="repo-creds-local-username"
            />
          </label>

          {draft.mode !== 'use-http-path' ? (
            <Checkbox
              checked={draft.useHttpPath}
              onChange={(value) => onChangeDraft({ ...draft, useHttpPath: value })}
              label={t(translate, 'repo-creds-use-http-path-label')}
              testId="repo-creds-use-http-path"
            />
          ) : null}

          {draft.mode === 'isolated-store' ? (
            <div className="repo-creds__inline-card stack">
              <label className="settings__field">
                <span className="settings__label">
                  {t(translate, 'repo-creds-store-file-label')}
                </span>
                <TextInput
                  value={draft.storeFile}
                  onChange={(value) => onChangeDraft({ ...draft, storeFile: value })}
                  monospace
                  testId="repo-creds-store-file"
                />
                <span className="settings__hint">{t(translate, 'repo-creds-store-file-hint')}</span>
              </label>
              <label className="settings__field">
                <span className="settings__label">
                  {t(translate, 'repo-creds-store-password-label')}
                </span>
                <TextInput
                  value={draft.storePassword}
                  onChange={(value) => onChangeDraft({ ...draft, storePassword: value })}
                  type="password"
                  testId="repo-creds-store-password"
                />
                <span className="settings__hint">
                  {t(translate, 'repo-creds-store-password-hint')}
                </span>
              </label>
              <div className="settings__hint credentials__warning">
                {t(translate, 'repo-creds-store-plaintext-warning')}
              </div>
            </div>
          ) : null}
        </>
      ) : null}

      {selectedRemote?.isHttp ? (
        <div className="settings__field">
          <span className="settings__label">
            {ta(translate, 'repo-creds-remote-url-action-label', { remote: selectedRemote.name })}
          </span>
          <Select<RemoteUsernameAction>
            value={draft.remoteUrlUsernameAction}
            options={[
              { value: 'keep', label: t(translate, 'repo-creds-remote-url-keep') },
              { value: 'embed', label: t(translate, 'repo-creds-remote-url-embed') },
              { value: 'strip', label: t(translate, 'repo-creds-remote-url-strip') }
            ]}
            onChange={(value) => onChangeDraft({ ...draft, remoteUrlUsernameAction: value })}
            testId="repo-creds-remote-url-action"
          />
          {urlPreview ? (
            <span className="mono settings__hint" data-testid="repo-creds-remote-url-preview">
              {ta(translate, 'repo-creds-remote-url-preview', { url: urlPreview })}
            </span>
          ) : null}
        </div>
      ) : null}

      <div className="repo-creds__inline-card stack">
        <strong>{t(translate, 'repo-creds-identity-title')}</strong>
        <span className="settings__hint">{t(translate, 'repo-creds-identity-hint')}</span>
        <div className="repo-creds__grid">
          <label className="settings__field">
            <span className="settings__label">
              {t(translate, 'repo-creds-identity-name-label')}
            </span>
            <TextInput
              value={draft.localUserName}
              onChange={(value) => onChangeDraft({ ...draft, localUserName: value })}
              placeholder={
                snapshot.localConfig.effectiveUserName
                  ? ta(translate, 'repo-creds-identity-inherited', {
                      value: snapshot.localConfig.effectiveUserName
                    })
                  : ''
              }
              testId="repo-creds-user-name"
            />
          </label>
          <label className="settings__field">
            <span className="settings__label">
              {t(translate, 'repo-creds-identity-email-label')}
            </span>
            <TextInput
              value={draft.localUserEmail}
              onChange={(value) => onChangeDraft({ ...draft, localUserEmail: value })}
              placeholder={
                snapshot.localConfig.effectiveUserEmail
                  ? ta(translate, 'repo-creds-identity-inherited', {
                      value: snapshot.localConfig.effectiveUserEmail
                    })
                  : ''
              }
              testId="repo-creds-user-email"
            />
          </label>
        </div>
      </div>

      {issues.length > 0 ? (
        <div className="status-conflict" data-testid="repo-creds-local-validation">
          {validationMessage(issues[0]!, translate)}
        </div>
      ) : null}

      <div className="settings__row">
        <button
          type="button"
          className="tool-button tool-button--primary"
          disabled={!canSave}
          onClick={onSave}
          data-testid="repo-creds-local-save"
        >
          {t(translate, 'repo-creds-local-save')}
        </button>
      </div>

      <section className="stack stack--tight">
        <strong>{t(translate, 'repo-creds-helpers-title')}</strong>
        {snapshot.localConfig.effectiveHelpers.length === 0 ? (
          <div className="settings__hint" data-testid="repo-creds-helpers-empty">
            {t(translate, 'repo-creds-helpers-empty')}
          </div>
        ) : (
          <div className="repo-creds__helpers" data-testid="repo-creds-helpers-list">
            {snapshot.localConfig.effectiveHelpers.map((helper, index) => (
              <div className="repo-creds__helper-row" key={`${helper.scope}:${helper.key}:${index}`}>
                <span className="repo-creds__scope-pill">{helper.scope || 'config'}</span>
                <span className="mono">{helper.key}</span>
                <span className="mono">
                  {helper.value || t(translate, 'repo-creds-helper-empty-reset')}
                </span>
                <span className="settings__hint mono">{helper.origin}</span>
              </div>
            ))}
          </div>
        )}
      </section>

      {selectedRemote ? (
        <section className="stack stack--tight">
          <strong>{t(translate, 'repo-creds-verify-title')}</strong>
          <div className="settings__row">
            <button
              type="button"
              className="tool-button"
              disabled={busy || verifying}
              onClick={onVerify}
              data-testid="repo-creds-verify"
            >
              {verifying ? <Spinner size={12} /> : <Icon name="search" size={13} />}
              {t(translate, 'repo-creds-verify-button')}
            </button>
            <button
              type="button"
              className="tool-button"
              disabled={busy || testingRemote}
              onClick={onTestRemote}
              data-testid="repo-creds-test-remote"
            >
              {testingRemote ? <Spinner size={12} /> : <Icon name="refresh-cw" size={13} />}
              {t(translate, 'repo-creds-test-remote-button')}
            </button>
          </div>

          {verification ? (
            <div className="repo-creds__inline-card stack stack--tight" data-testid="repo-creds-verify-result">
              <div className="settings__hint">
                <strong>{t(translate, 'repo-creds-verify-augur-label')}:</strong>{' '}
                {verification.augurSource === 'repository-vault'
                  ? ta(translate, 'repo-creds-verify-augur-repo', {
                      username: verification.augurUsername ?? ''
                    })
                  : verification.augurSource === 'global-vault'
                    ? ta(translate, 'repo-creds-verify-augur-global', {
                        username: verification.augurUsername ?? ''
                      })
                    : t(translate, 'repo-creds-verify-augur-none')}{' '}
                ·{' '}
                {t(
                  translate,
                  verification.isolatesGlobalHelpers
                    ? 'repo-creds-verify-isolated-yes'
                    : 'repo-creds-verify-isolated-no'
                )}
              </div>
              <div className="settings__hint">
                <strong>{t(translate, 'repo-creds-verify-git-fill-label')}:</strong>{' '}
                {verification.gitFill.ok && verification.gitFill.passwordPresent
                  ? ta(translate, 'repo-creds-verify-git-fill-ok', {
                      username: verification.gitFill.username ?? '—'
                    })
                  : verification.gitFill.username
                    ? ta(translate, 'repo-creds-verify-git-fill-username-only', {
                        username: verification.gitFill.username
                      })
                    : t(translate, 'repo-creds-verify-git-fill-empty')}
                {verification.gitFill.detail ? ` (${verification.gitFill.detail})` : ''}
              </div>
              {verification.warningKeys.map((key) => (
                <div className="credentials__warning settings__hint" key={key}>
                  {t(translate, key)}
                </div>
              ))}
            </div>
          ) : null}

          {remoteTest ? (
            <div
              className={remoteTest.ok ? 'settings__hint' : 'status-conflict'}
              data-testid="repo-creds-test-result"
            >
              {remoteTest.ok
                ? t(translate, remoteTest.key)
                : `${t(translate, remoteTest.key)}${remoteTest.detail ? `: ${remoteTest.detail}` : ''}`}
            </div>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}

function validationMessage(issue: LocalDraftIssue, translate: (key: string) => string): string {
  switch (issue) {
    case 'storeFile':
      return t(translate, 'repo-creds-validation-store-file');
    case 'username':
      return t(translate, 'repo-creds-validation-username');
    case 'localUserEmail':
      return t(translate, 'repo-creds-validation-email');
    case 'remoteUrlEmbed':
      return t(translate, 'repo-creds-validation-embed-username');
    default:
      return t(translate, 'repo-creds-validation-control');
  }
}
