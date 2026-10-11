/**
 * The dialog a blocked commit opens.
 *
 * Git refuses to create a commit until it knows who the author is. Rather than
 * sending the interface's user to the settings window, this dialog collects
 * the two missing values, writes them to the repository's own global
 * configuration — a WSL repository gets its distro's configuration — and
 * retries the commit that was interrupted. The full settings window stays one
 * click away for everything beyond identity.
 */

import { useEffect, useState } from 'react';

import { Icon } from '../../components/Icon';
import { DialogCard, TextInput } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import { Overlay, renderGitError, useStore } from '../../app/store';
import { t } from '../../i18n/strings';

function isValidEmail(value: string): boolean {
  return value.includes('@') && !/\s/.test(value);
}

export function GitIdentityDialog({
  overlay
}: {
  overlay: Extract<Overlay, { kind: 'gitIdentity' }>;
}) {
  const translate = useStore((state) => state.t);
  const repo = useStore((state) => state.repos[overlay.repoId]);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  // Whatever Git already resolves prefills the fields, so a half-configured
  // identity (a name in the repository, an email nowhere) shows as it is.
  useEffect(() => {
    let cancelled = false;
    void ipc
      .probeCommitIdentity(overlay.repoId)
      .then((identity) => {
        if (cancelled) return;
        setName((current) => current || identity.name || '');
        setEmail((current) => current || identity.email || '');
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [overlay.repoId]);

  const valid = Boolean(name.trim()) && isValidEmail(email.trim());

  const save = async () => {
    if (!repo || !valid || saving) {
      return;
    }
    setSaving(true);
    setError('');
    try {
      await ipc.writeGitConfig(
        [
          { key: 'user.name', value: name.trim() },
          { key: 'user.email', value: email.trim() }
        ],
        repo.location
      );
      closeOverlay();
      // The guard in runAction probes again, so the retry cannot fail for the
      // same reason; the commit now either runs or reports a different error.
      void runAction(overlay.repoId, {
        action: 'commit',
        message: overlay.message,
        amend: overlay.amend
      });
    } catch (failure) {
      const described = ipc.describeError(failure);
      setError(renderGitError(translate, described.key, described.detail));
      setSaving(false);
    }
  };

  return (
    <DialogCard
      testId="git-identity-dialog"
      title={t(translate, 'git-identity-dialog-title')}
      icon={<Icon name="settings" size={16} />}
      onBackdrop={closeOverlay}
      width={480}
      body={
        <>
          <div className="muted" data-testid="git-identity-description">
            {t(translate, 'git-identity-dialog-description')}
          </div>
          <label className="settings__label" htmlFor="git-identity-name">
            {t(translate, 'git-user-name-title')}
          </label>
          <TextInput
            id="git-identity-name"
            value={name}
            onChange={setName}
            autoFocus
            testId="git-identity-name"
            onSubmit={() => void save()}
            onEscape={closeOverlay}
          />
          <label className="settings__label" htmlFor="git-identity-email">
            {t(translate, 'git-user-email-title')}
          </label>
          <TextInput
            id="git-identity-email"
            value={email}
            onChange={setEmail}
            testId="git-identity-email"
            onSubmit={() => void save()}
            onEscape={closeOverlay}
          />
          {!valid && (name.trim() || email.trim()) ? (
            <div className="status-conflict" data-testid="git-identity-invalid">
              {!name.trim() ? t(translate, 'git-identity-name-required') : null}
              {!isValidEmail(email.trim()) ? t(translate, 'git-identity-email-required') : null}
            </div>
          ) : null}
          {error ? (
            <div className="status-conflict" role="alert" data-testid="git-identity-error">
              {error}
            </div>
          ) : null}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            data-testid="git-identity-settings"
            onClick={() => void ipc.openSettingsWindow('git')}
          >
            {t(translate, 'git-identity-open-settings')}
          </button>
          <span style={{ flex: 1 }} />
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="git-identity-cancel"
          >
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!valid || saving}
            aria-busy={saving}
            data-testid="git-identity-save"
            onClick={() => void save()}
          >
            {t(translate, 'git-identity-save-commit')}
          </button>
        </>
      }
    />
  );
}
