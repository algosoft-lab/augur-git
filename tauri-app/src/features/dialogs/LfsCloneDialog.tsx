/**
 * The clone wizard.
 *
 * Cloning over HTTPS is the one flow that needs an account before a repository
 * exists, so it is asked for in three short steps instead of one crowded form:
 * which repository, where it should live, and then the clone itself. A prompt
 * from Git for a username and password is answered inside the same card, which
 * keeps the whole flow on one surface.
 */

import { useEffect, useState } from 'react';
import { pickDirectory } from '../../platform';

import { Icon } from '../../components/Icon';
import { Checkbox, DialogCard, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import type { CloneProgressEvent, CloneResult, CredentialLookup } from '../../bridge/types';
import { t, ta } from '../../i18n/strings';
import {
  joinCloneDestination,
  splitCloneDestination,
  suggestedCloneName
} from './cloneDestination';
import {
  CredentialLoginActions,
  CredentialLoginFields,
  useCredentialLogin
} from './credentialLogin';
import { useCredentialRequests } from './credentialRequests';

type Step = 'remote' | 'destination' | 'running' | 'result';

const STEPS: Step[] = ['remote', 'destination', 'running'];

export function LfsCloneDialog() {
  const translate = useStore((state) => state.t);
  const config = useStore((state) => state.config);
  const openTab = useStore((state) => state.openTab);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [step, setStep] = useState<Step>('remote');
  const [remote, setRemote] = useState('');
  const [destination, setDestination] = useState('');
  const [downloadLfs, setDownloadLfs] = useState(config.lfs_auto_download);
  const [busy, setBusy] = useState(false);
  const [savedAccount, setSavedAccount] = useState<CredentialLookup | null>(null);
  const [operationId, setOperationId] = useState<string | null>(null);
  const [progress, setProgress] = useState<CloneProgressEvent | null>(null);
  const [error, setError] = useState('');
  const [result, setResult] = useState<CloneResult | null>(null);

  const requests = useCredentialRequests();
  // Git can only ask for an account once the clone is running, but a request is
  // answered on whichever page is showing, so an unrelated prompt is never
  // hidden behind the wizard.
  const signIn = requests.find((entry) => entry.kind === 'login') ?? null;
  const login = useCredentialLogin(signIn);
  const name = suggestedCloneName(remote);
  const target = splitCloneDestination(destination);

  useEffect(() => {
    setSavedAccount(null);
    if (!remote.trim()) return;
    const timer = window.setTimeout(() => {
      void ipc
        .lookupCredential(remote)
        .then((found) => setSavedAccount(found.saved ? found : null))
        .catch(() => setSavedAccount(null));
    }, 300);
    return () => window.clearTimeout(timer);
  }, [remote]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void ipc
      .onLfsCloneProgress((event) => {
        if (active && event.operationId === operationId) setProgress(event);
      })
      .then((dispose) => {
        if (!active) dispose();
        else unlisten = dispose;
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [operationId]);

  const chooseDestination = async () => {
    const selected = await pickDirectory(t(translate, 'clone-parent-title'));
    if (typeof selected === 'string') setDestination(joinCloneDestination(selected, name));
  };

  const startClone = async (from: string) => {
    const parsed = splitCloneDestination(from);
    if (!parsed.ok) return;
    const id = `clone-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    setBusy(true);
    setError('');
    setProgress(null);
    setResult(null);
    setOperationId(id);
    setStep('running');
    try {
      const cloned = await ipc.lfsClone(
        remote.trim(),
        parsed.parent,
        parsed.directoryName,
        downloadLfs,
        id
      );
      setResult(cloned);
      if (!cloned.lfsError) {
        closeOverlay();
        await openTab(cloned.path, { kind: 'local' });
      } else {
        setStep('result');
      }
    } catch (reason) {
      setError(describeError(reason));
      setStep('destination');
    } finally {
      setBusy(false);
      setOperationId(null);
    }
  };

  const cancel = () => {
    login.cancel();
    if (operationId) void ipc.cancelLfsClone(operationId);
  };

  // Signing in happens after the destination is chosen, so the indicator is at
  // its last stage even though the visible page is the credential form.
  const stepIndex = signIn ? STEPS.length - 1 : STEPS.indexOf(step);

  return (
    <DialogCard
      testId="lfs-clone-dialog"
      title={t(translate, 'clone-title')}
      icon={<Icon name="download" size={16} />}
      width={520}
      onBackdrop={() => {
        if (!busy) closeOverlay();
      }}
      body={
        <div className="clone">
          {step === 'result' ? null : (
            <div className="clone__steps" aria-hidden>
              {STEPS.map((entry, index) => (
                <span
                  key={entry}
                  className={`clone__step${index <= stepIndex ? ' is-active' : ''}`}
                />
              ))}
            </div>
          )}
          {signIn ? (
            <div className="clone__panel" data-testid="clone-credentials">
              <div className="clone__question">
                {ta(translate, 'credential-login-title', { host: signIn.host ?? '' })}
              </div>
              <CredentialLoginFields request={signIn} login={login} />
            </div>
          ) : null}
          {signIn ? null : step === 'remote' ? (
            <div className="clone__panel">
              <div className="clone__question">{t(translate, 'clone-remote-question')}</div>
              <TextInput
                value={remote}
                onChange={setRemote}
                placeholder="https://gitlab.example.com/team/project.git"
                monospace
                autoFocus
                testId="lfs-clone-remote"
                onSubmit={() => {
                  if (remote.trim()) setStep('destination');
                }}
              />
              {savedAccount?.username ? (
                <div className="clone__note" data-testid="clone-saved-account">
                  {ta(translate, 'remotes-saved-account', { username: savedAccount.username })}
                </div>
              ) : null}
            </div>
          ) : null}
          {signIn ? null : step === 'destination' ? (
            <div className="clone__panel">
              <div className="clone__question">{t(translate, 'clone-destination-question')}</div>
              <div className="clone__remote">{remote.trim()}</div>
              <div className="clone__field">
                <TextInput
                  value={destination}
                  onChange={setDestination}
                  placeholder={`/home/you/projects/${name || 'project'}`}
                  monospace
                  autoFocus
                  testId="lfs-clone-destination"
                  onSubmit={() => void startClone(destination)}
                />
                <button
                  type="button"
                  className="tool-button"
                  onClick={() => void chooseDestination()}
                  data-testid="lfs-clone-browse"
                >
                  {t(translate, 'clone-choose-parent')}
                </button>
              </div>
              {target.ok || !destination.trim() ? (
                <div className="clone__note">{t(translate, 'clone-destination-hint')}</div>
              ) : (
                <div
                  className="clone__note clone__note--warn"
                  data-testid="clone-destination-error"
                >
                  {destinationProblem(target.reason, translate)}
                </div>
              )}
              <Checkbox
                checked={downloadLfs}
                onChange={setDownloadLfs}
                label={t(translate, 'clone-download-lfs')}
                testId="lfs-clone-download-lfs"
              />
              {error ? (
                <div className="status-conflict" role="alert">
                  {error}
                </div>
              ) : null}
            </div>
          ) : null}
          {signIn ? null : step === 'running' ? (
            <div className="clone__panel">
              <div className="clone__question">{t(translate, 'clone-running-question')}</div>
              <div className="clone__note" role="status" data-testid="lfs-clone-progress">
                {progress?.message ?? t(translate, 'clone-progress-starting')}
              </div>
              {progress?.progress ? (
                <div className="clone__note">
                  {`${progress.progress.currentFile}/${progress.progress.totalFiles} · ${formatBytes(progress.progress.transferredBytes)} / ${formatBytes(progress.progress.totalBytes)}`}
                </div>
              ) : null}
            </div>
          ) : null}
          {signIn ? null : step === 'result' && result ? (
            <div className="clone__panel">
              <div className="clone__question">{t(translate, 'clone-partial-question')}</div>
              <div className="status-conflict" role="alert">
                {result.lfsError}
              </div>
              <div className="clone__note mono">{result.path}</div>
              <div className="clone__note">{t(translate, 'clone-partial-help')}</div>
            </div>
          ) : null}
        </div>
      }
      footer={footer()}
    />
  );

  function footer() {
    if (signIn) return <CredentialLoginActions login={login} />;
    if (step === 'result' && result) {
      return (
        <>
          <button type="button" className="tool-button" onClick={closeOverlay}>
            {t(translate, 'dialog-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            onClick={() => {
              closeOverlay();
              void openTab(result.path, { kind: 'local' });
            }}
          >
            {t(translate, 'clone-open-partial')}
          </button>
        </>
      );
    }
    if (step === 'running') {
      return (
        <button
          type="button"
          className="tool-button tool-button--danger"
          onClick={cancel}
          data-testid="lfs-clone-cancel"
        >
          {t(translate, 'clone-cancel')}
        </button>
      );
    }
    if (step === 'destination') {
      return (
        <>
          <button
            type="button"
            className="tool-button"
            onClick={() => setStep('remote')}
            data-testid="lfs-clone-back"
          >
            {t(translate, 'clone-back')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={destination === '' || !target.ok}
            onClick={() => void startClone(destination)}
            data-testid="lfs-clone-start"
          >
            {t(translate, 'clone-start')}
          </button>
        </>
      );
    }
    return (
      <>
        <button type="button" className="tool-button" onClick={closeOverlay}>
          {t(translate, 'dialog-cancel')}
        </button>
        <button
          type="button"
          className="tool-button tool-button--primary"
          disabled={!remote.trim()}
          onClick={() => setStep('destination')}
          data-testid="lfs-clone-continue"
        >
          {t(translate, 'clone-continue')}
        </button>
      </>
    );
  }
}

function destinationProblem(
  reason: 'empty' | 'relative' | 'home' | 'no-name',
  translate: (key: string) => string
): string {
  switch (reason) {
    case 'empty':
    case 'no-name':
      return t(translate, 'clone-destination-invalid');
    case 'home':
      return t(translate, 'clone-destination-home');
    case 'relative':
      return t(translate, 'clone-destination-relative');
  }
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

function describeError(reason: unknown): string {
  if (reason && typeof reason === 'object' && 'detail' in reason) {
    return String((reason as { detail: unknown }).detail);
  }
  return String(reason);
}
