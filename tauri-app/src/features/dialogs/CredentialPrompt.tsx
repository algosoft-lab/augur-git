import { useEffect, useState } from 'react';

import * as ipc from '../../bridge/ipc';
import type { CredentialStorage, LfsCredentialRequest } from '../../bridge/types';
import { DialogCard, Select, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';

type PersistChoice = CredentialStorage | 'none';

/**
 * The one credential surface for every HTTPS prompt.
 *
 * A `login` request is a host this application can remember: it shows the host,
 * both fields, and where the secret may be kept. A `prompt` request is a prompt
 * Git produced that has no known remote behind it; it keeps the original
 * single-field, remember-nothing behaviour.
 */
export function CredentialPrompt() {
  const translate = useStore((state) => state.t);
  const [requests, setRequests] = useState<LfsCredentialRequest[]>([]);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [value, setValue] = useState('');
  const [persist, setPersist] = useState<PersistChoice>('none');
  const [repositoryOnly, setRepositoryOnly] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [answered, setAnswered] = useState(false);
  const active = requests[0] ?? null;
  const login = active?.kind === 'login';

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void ipc
      .onLfsCredentialRequest((request) => {
        setRequests((current) =>
          current.some((entry) => entry.requestId === request.requestId)
            ? current
            : [...current, request]
        );
      })
      .then((dispose) => {
        if (cancelled) dispose();
        else unlisten = dispose;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    setUsername(active?.username ?? '');
    setPassword('');
    setValue('');
    setRepositoryOnly(false);
    setError(null);
    setAnswered(false);
    setBusy(false);
    setPersist(defaultPersist(active));
  }, [active?.requestId]);

  const drop = (requestId: string) => {
    setRequests((current) => current.filter((entry) => entry.requestId !== requestId));
  };

  const respondPrompt = (response: string | null) => {
    if (!active) return;
    drop(active.requestId);
    void ipc.respondToLfsCredential(active.requestId, response);
  };

  const submitPrompt = () => {
    if (!value || value.includes('\n') || value.includes('\r')) return;
    respondPrompt(value);
  };

  const cancel = async () => {
    if (!active) return;
    if (!login) {
      respondPrompt(null);
      return;
    }
    setBusy(true);
    await ipc
      .respondToCredentialLogin(active.requestId, {
        username: '',
        password: '',
        persist: null,
        repositoryOnly: false,
        cancel: true
      })
      .catch(() => undefined);
    setBusy(false);
    drop(active.requestId);
  };

  const submitLogin = async () => {
    if (!active || !canSubmitLogin(username)) return;
    setBusy(true);
    setError(null);
    try {
      const result = await ipc.respondToCredentialLogin(active.requestId, {
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
        drop(active.requestId);
      }
    } catch (cause) {
      setError(ipc.describeError(cause).detail);
      setAnswered(true);
    } finally {
      setBusy(false);
    }
  };

  if (!active) return null;

  if (!login) {
    return (
      <DialogCard
        testId="credential-dialog"
        title={t(translate, 'credential-prompt-title')}
        subtitle={active.host ?? undefined}
        onBackdrop={() => respondPrompt(null)}
        body={
          <div className="stack">
            <div className="settings__hint">{active.prompt}</div>
            <TextInput
              value={value}
              onChange={setValue}
              type={/password|passphrase/i.test(active.prompt) ? 'password' : 'text'}
              autoFocus
              testId="credential-input"
              onSubmit={submitPrompt}
              onEscape={() => respondPrompt(null)}
            />
            <div className="settings__hint">{t(translate, 'credential-prompt-memory-only')}</div>
          </div>
        }
        footer={
          <>
            <button type="button" className="tool-button" onClick={() => respondPrompt(null)}>
              {t(translate, 'dialog-cancel')}
            </button>
            <button
              type="button"
              className="tool-button tool-button--primary"
              disabled={!value || value.includes('\n') || value.includes('\r')}
              onClick={submitPrompt}
              data-testid="credential-submit"
            >
              {t(translate, 'credential-submit')}
            </button>
          </>
        }
      />
    );
  }

  const storageOptions = persistOptions(active, translate);
  const persistHint = persistHintFor(persist, active, translate);
  const canSubmit = canSubmitLogin(username) && !busy && !answered;

  return (
    <DialogCard
      testId="credential-dialog"
      title={ta(translate, 'credential-login-title', { host: active.host ?? '' })}
      subtitle={active.repository ?? undefined}
      width={460}
      onBackdrop={() => void cancel()}
      body={
        <div className="stack">
          {active.stored ? (
            <div className="settings__hint">{t(translate, 'credential-login-stored-note')}</div>
          ) : null}
          <label className="settings__field">
            <span className="settings__label">{t(translate, 'credential-login-username')}</span>
            <TextInput
              value={username}
              onChange={setUsername}
              autoFocus
              disabled={answered}
              testId="credential-username"
              onSubmit={() => void submitLogin()}
              onEscape={() => void cancel()}
            />
          </label>
          <label className="settings__field">
            <span className="settings__label">{t(translate, 'credential-login-password')}</span>
            <TextInput
              value={password}
              onChange={setPassword}
              type="password"
              disabled={answered}
              testId="credential-password"
              onSubmit={() => void submitLogin()}
              onEscape={() => void cancel()}
            />
          </label>
          <div className="settings__field" data-testid="credential-storage-field">
            <span className="settings__label">{t(translate, 'credential-login-remember')}</span>
            <Select<PersistChoice>
              value={persist}
              options={storageOptions}
              onChange={setPersist}
              testId="credential-storage"
            />
            {persistHint ? <div className="settings__hint">{persistHint}</div> : null}
          </div>
          {active.canPersist && active.repository ? (
            <div className="settings__field">
              <span className="settings__label">
                {t(translate, 'credential-login-scope-label')}
              </span>
              <Select<'global' | 'repository'>
                value={repositoryOnly ? 'repository' : 'global'}
                options={[
                  { value: 'global', label: t(translate, 'credential-login-scope-global') },
                  {
                    value: 'repository',
                    label: ta(translate, 'credential-login-scope-repository', {
                      repository: active.repository
                    })
                  }
                ]}
                onChange={(next) => setRepositoryOnly(next === 'repository')}
                testId="credential-scope"
              />
            </div>
          ) : null}
          {error ? (
            <div className="settings__hint" data-testid="credential-error">
              {error}
            </div>
          ) : null}
        </div>
      }
      footer={
        answered ? (
          <button
            type="button"
            className="tool-button tool-button--primary"
            onClick={() => drop(active.requestId)}
            data-testid="credential-close"
          >
            {t(translate, 'credential-close')}
          </button>
        ) : (
          <>
            <button
              type="button"
              className="tool-button"
              disabled={busy}
              onClick={() => void cancel()}
              data-testid="credential-cancel"
            >
              {t(translate, 'dialog-cancel')}
            </button>
            <button
              type="button"
              className="tool-button tool-button--primary"
              disabled={!canSubmit}
              onClick={() => void submitLogin()}
              data-testid="credential-submit"
            >
              {t(translate, 'credential-submit')}
            </button>
          </>
        )
      }
    />
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
