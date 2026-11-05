import { useState } from 'react';

import * as ipc from '../../bridge/ipc';
import type { LfsCredentialRequest } from '../../bridge/types';
import { DialogCard, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import {
  CredentialLoginActions,
  CredentialLoginFields,
  useCredentialLogin
} from './credentialLogin';
import { dropCredentialRequest, useCredentialRequests } from './credentialRequests';

/**
 * The one credential surface for every HTTPS prompt outside the clone wizard.
 *
 * A `login` request is a host this application can remember: it shows the host,
 * both fields, and where the secret may be kept. A `prompt` request is a prompt
 * Git produced that has no known remote behind it; it keeps the original
 * single-field, remember-nothing behaviour.
 *
 * The clone wizard answers its own `login` request inline, so this dialog steps
 * aside while that overlay is open rather than covering it with a second card.
 */
export function CredentialPrompt() {
  const translate = useStore((state) => state.t);
  const cloneWizardOpen = useStore((state) => state.overlay?.kind === 'lfsClone');
  const requests = useCredentialRequests();
  const active = requests[0] ?? null;
  const login = active?.kind === 'login' ? active : null;
  const form = useCredentialLogin(login);

  if (!active) return null;
  if (login && cloneWizardOpen) return null;

  if (!login) {
    return <PromptValue request={active} />;
  }

  const title = ta(translate, 'credential-login-title', { host: login.host ?? '' });

  return (
    <DialogCard
      testId="credential-dialog"
      title={title}
      subtitle={login.repository ?? undefined}
      width={460}
      onBackdrop={form.cancel}
      body={<CredentialLoginFields request={login} login={form} />}
      footer={<CredentialLoginActions login={form} />}
    />
  );
}

/** A prompt with no host behind it: one field, kept in memory for this answer. */
function PromptValue({ request }: { request: LfsCredentialRequest }) {
  const translate = useStore((state) => state.t);
  const [value, setValue] = useState('');
  const invalid = !value || value.includes('\n') || value.includes('\r');

  const respond = (response: string | null) => {
    dropCredentialRequest(request.requestId);
    void ipc.respondToLfsCredential(request.requestId, response);
  };

  return (
    <DialogCard
      testId="credential-dialog"
      title={t(translate, 'credential-prompt-title')}
      subtitle={request.host ?? undefined}
      onBackdrop={() => respond(null)}
      body={
        <div className="stack">
          <div className="settings__hint">{request.prompt}</div>
          <TextInput
            value={value}
            onChange={setValue}
            type={/password|passphrase/i.test(request.prompt) ? 'password' : 'text'}
            autoFocus
            testId="credential-input"
            onSubmit={() => {
              if (!invalid) respond(value);
            }}
            onEscape={() => respond(null)}
          />
          <div className="settings__hint">{t(translate, 'credential-prompt-memory-only')}</div>
        </div>
      }
      footer={
        <>
          <button type="button" className="tool-button" onClick={() => respond(null)}>
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={invalid}
            onClick={() => respond(value)}
            data-testid="credential-submit"
          >
            {t(translate, 'credential-submit')}
          </button>
        </>
      }
    />
  );
}
