/**
 * The sign-in form shared by the credential dialog and the clone wizard.
 *
 * A prompt Git raised for a known host arrives as a `login` request: it carries
 * the host, the account Git already knows about, where the secret may be kept,
 * and whether the account applies to one repository or every one on the host.
 * Both surfaces ask the same questions, so the form lives here and only the
 * frame around it differs.
 */

import { useEffect, useState } from 'react';

import * as ipc from '../../bridge/ipc';
import type { CredentialStorage, LfsCredentialRequest } from '../../bridge/types';
import { Select, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { dropCredentialRequest } from './credentialRequests';

export type PersistChoice = CredentialStorage | 'none';

export interface CredentialLogin {
  username: string;
  setUsername: (value: string) => void;
  password: string;
  setPassword: (value: string) => void;
  persist: PersistChoice;
  setPersist: (value: PersistChoice) => void;
  repositoryOnly: boolean;
  setRepositoryOnly: (value: boolean) => void;
  busy: boolean;
  answered: boolean;
  error: string | null;
  canSubmit: boolean;
  submit: () => void;
  cancel: () => void;
  close: () => void;
}

export function useCredentialLogin(request: LfsCredentialRequest | null): CredentialLogin {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [persist, setPersist] = useState<PersistChoice>('none');
  const [repositoryOnly, setRepositoryOnly] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [answered, setAnswered] = useState(false);

  useEffect(() => {
    setUsername(request?.username ?? '');
    setPassword('');
    setRepositoryOnly(false);
    setError(null);
    setAnswered(false);
    setBusy(false);
    setPersist(defaultPersist(request));
  }, [request?.requestId, request]);

  const close = () => {
    if (request) dropCredentialRequest(request.requestId);
  };

  const cancel = async () => {
    if (!request) return;
    setBusy(true);
    await ipc
      .respondToCredentialLogin(request.requestId, {
        username: '',
        password: '',
        persist: null,
        repositoryOnly: false,
        cancel: true
      })
      .catch(() => undefined);
    setBusy(false);
    close();
  };

  const submit = async () => {
    if (!request || !canSubmitLogin(username)) return;
    setBusy(true);
    setError(null);
    try {
      const result = await ipc.respondToCredentialLogin(request.requestId, {
        username,
        password,
        persist: persist === 'none' ? null : persist,
        repositoryOnly,
        cancel: false
      });
      if (result.error) {
        // Git already received the value, so the form cannot be retried; it
        // stays visible only long enough to explain why nothing was saved.
        setError(result.error);
        setAnswered(true);
      } else {
        close();
      }
    } catch (cause) {
      setError(ipc.describeError(cause).detail);
      setAnswered(true);
    } finally {
      setBusy(false);
    }
  };

  return {
    username,
    setUsername,
    password,
    setPassword,
    persist,
    setPersist,
    repositoryOnly,
    setRepositoryOnly,
    busy,
    answered,
    error,
    canSubmit: canSubmitLogin(username) && !busy && !answered,
    submit: () => void submit(),
    cancel: () => void cancel(),
    close
  };
}

/** The fields of a `login` request, without any framing buttons. */
export function CredentialLoginFields({
  request,
  login
}: {
  request: LfsCredentialRequest;
  login: CredentialLogin;
}) {
  const translate = useStore((state) => state.t);
  const storageOptions = persistOptions(request, translate);
  const persistHint = persistHintFor(login.persist, request, translate);
  return (
    <div className="stack">
      {request.stored ? (
        <div className="settings__hint">{t(translate, 'credential-login-stored-note')}</div>
      ) : null}
      <label className="settings__field">
        <span className="settings__label">{t(translate, 'credential-login-username')}</span>
        <TextInput
          value={login.username}
          onChange={login.setUsername}
          autoFocus
          disabled={login.answered}
          testId="credential-username"
          onSubmit={login.submit}
          onEscape={login.cancel}
        />
      </label>
      <label className="settings__field">
        <span className="settings__label">{t(translate, 'credential-login-password')}</span>
        <TextInput
          value={login.password}
          onChange={login.setPassword}
          type="password"
          disabled={login.answered}
          testId="credential-password"
          onSubmit={login.submit}
          onEscape={login.cancel}
        />
      </label>
      <div className="settings__field" data-testid="credential-storage-field">
        <span className="settings__label">{t(translate, 'credential-login-remember')}</span>
        <Select<PersistChoice>
          value={login.persist}
          options={storageOptions}
          onChange={login.setPersist}
          testId="credential-storage"
        />
        {persistHint ? <div className="settings__hint">{persistHint}</div> : null}
      </div>
      {request.canPersist && request.repository ? (
        <div className="settings__field">
          <span className="settings__label">{t(translate, 'credential-login-scope-label')}</span>
          <Select<'global' | 'repository'>
            value={login.repositoryOnly ? 'repository' : 'global'}
            options={[
              { value: 'global', label: t(translate, 'credential-login-scope-global') },
              {
                value: 'repository',
                label: ta(translate, 'credential-login-scope-repository', {
                  repository: request.repository ?? ''
                })
              }
            ]}
            onChange={(next) => login.setRepositoryOnly(next === 'repository')}
            testId="credential-scope"
          />
        </div>
      ) : null}
      {login.error ? (
        <div className="settings__hint" data-testid="credential-error">
          {login.error}
        </div>
      ) : null}
    </div>
  );
}

/** The two buttons a `login` request needs, in the order every dialog uses. */
export function CredentialLoginActions({ login }: { login: CredentialLogin }) {
  const translate = useStore((state) => state.t);
  if (login.answered) {
    return (
      <button
        type="button"
        className="tool-button tool-button--primary"
        onClick={login.close}
        data-testid="credential-close"
      >
        {t(translate, 'credential-close')}
      </button>
    );
  }
  return (
    <>
      <button
        type="button"
        className="tool-button"
        disabled={login.busy}
        onClick={login.cancel}
        data-testid="credential-cancel"
      >
        {t(translate, 'dialog-cancel')}
      </button>
      <button
        type="button"
        className="tool-button tool-button--primary"
        disabled={!login.canSubmit}
        onClick={login.submit}
        data-testid="credential-submit"
      >
        {t(translate, 'credential-submit')}
      </button>
    </>
  );
}

function canSubmitLogin(username: string): boolean {
  return username.trim().length > 0 && !username.includes('\n') && !username.includes('\r');
}

function defaultPersist(request: LfsCredentialRequest | null): PersistChoice {
  if (!request?.canPersist) return 'none';
  const preferred = request.defaultStorage ?? 'session';
  if (preferred === 'keychain' && request.keychainAvailable) return 'keychain';
  if (preferred === 'plaintext' && request.plaintextAvailable) return 'plaintext';
  return 'session';
}

function persistOptions(
  request: LfsCredentialRequest,
  translate: (key: string) => string
): { value: PersistChoice; label: string }[] {
  const options: { value: PersistChoice; label: string }[] = [
    { value: 'none', label: t(translate, 'credential-storage-none') },
    { value: 'session', label: t(translate, 'credential-storage-session') }
  ];
  if (request.keychainAvailable) {
    options.push({ value: 'keychain', label: t(translate, 'credential-storage-keychain') });
  }
  if (request.plaintextAvailable) {
    options.push({ value: 'plaintext', label: t(translate, 'credential-storage-plaintext') });
  }
  return options;
}

function persistHintFor(
  persist: PersistChoice,
  request: LfsCredentialRequest,
  translate: (key: string) => string
): string | null {
  switch (persist) {
    case 'none':
      return t(translate, 'credential-storage-none-hint');
    case 'session':
      return t(translate, 'credential-storage-session-hint');
    case 'keychain':
      return t(translate, 'credential-storage-keychain-hint');
    case 'plaintext':
      return request.plaintextAvailable ? t(translate, 'credential-storage-plaintext-hint') : null;
  }
}
