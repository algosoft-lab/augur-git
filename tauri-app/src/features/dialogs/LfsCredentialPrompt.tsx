import { useEffect, useState } from 'react';

import * as ipc from '../../bridge/ipc';
import type { LfsCredentialRequest } from '../../bridge/types';
import { DialogCard, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import { t } from '../../i18n/strings';

export function LfsCredentialPrompt() {
  const translate = useStore((state) => state.t);
  const [requests, setRequests] = useState<LfsCredentialRequest[]>([]);
  const [value, setValue] = useState('');
  const active = requests[0] ?? null;
  const password = Boolean(active && /password|passphrase/i.test(active.prompt));

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void ipc.onLfsCredentialRequest((request) => {
      setRequests((current) => current.some((entry) => entry.requestId === request.requestId)
        ? current
        : [...current, request]);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => setValue(''), [active?.requestId]);

  const respond = (response: string | null) => {
    if (!active) return;
    const requestId = active.requestId;
    setRequests((current) => current.filter((entry) => entry.requestId !== requestId));
    void ipc.respondToLfsCredential(requestId, response);
  };

  if (!active) return null;
  const submit = () => {
    if (!value || value.includes('\n') || value.includes('\r')) return;
    respond(value);
  };

  return (
    <DialogCard
      testId="lfs-credential-dialog"
      title={t(translate, password ? 'lfs-credential-password-title' : 'lfs-credential-username-title')}
      onBackdrop={() => respond(null)}
      body={
        <div className="stack">
          <div className="settings__hint">{active.prompt}</div>
          <TextInput
            value={value}
            onChange={setValue}
            type={password ? 'password' : 'text'}
            autoFocus
            testId="lfs-credential-input"
            onSubmit={submit}
            onEscape={() => respond(null)}
          />
          <div className="settings__hint">{t(translate, 'lfs-credential-memory-only')}</div>
        </div>
      }
      footer={
        <>
          <button type="button" className="tool-button" onClick={() => respond(null)}>{t(translate, 'dialog-cancel')}</button>
          <button type="button" className="tool-button tool-button--primary" disabled={!value || value.includes('\n') || value.includes('\r')} onClick={submit} data-testid="lfs-credential-submit">{t(translate, 'lfs-credential-submit')}</button>
        </>
      }
    />
  );
}
