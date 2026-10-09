import { useEffect, useState } from 'react';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import * as ipc from '../../bridge/ipc';
import type { CliStatus } from '../../bridge/types';
import { useStore } from '../../app/store';
import { t } from '../../i18n/strings';

export function CliSettings() {
  const translate = useStore((state) => state.t);
  const [status, setStatus] = useState<CliStatus | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [copied, setCopied] = useState(false);
  async function run(action: () => Promise<CliStatus>) {
    setPending(true);
    setError('');
    try {
      setStatus(await action());
    } catch (error) {
      setError(String(error));
    } finally {
      setPending(false);
    }
  }
  useEffect(() => {
    let cancelled = false;
    void ipc
      .getCliStatus()
      .then((value) => {
        if (!cancelled) setStatus(value);
      })
      .catch((error) => {
        if (!cancelled) setError(String(error));
      });
    return () => {
      cancelled = true;
    };
  }, []);
  if (status?.state === 'unsupported') return null;
  const shellAvailable = status?.state === 'not-on-path' && status.shell.state === 'available';
  const shellConflict = status?.shell.state === 'conflict';
  return (
    <div className="settings__field" data-testid="settings-cli">
      <span className="settings__label">{t(translate, 'cli-title')}</span>
      <p className="settings__hint">{t(translate, 'cli-description')}</p>
      {status ? (
        <>
          <div className="cli-settings__status" data-testid="cli-status">
            <span className="cli-settings__status-mark" aria-hidden="true" />
            {t(
              translate,
              shellAvailable ? 'cli-status-shell-available' : `cli-status-${status.state}`
            )}
          </div>
          <div className="settings__hint mono">{status.path}</div>
          {status.shell.state === 'conflict' ? (
            <p className="settings__hint" data-testid="cli-shell-conflict">
              {t(translate, 'cli-shell-conflict')}
              {status.shell.path ? <span className="mono"> {status.shell.path}</span> : null}
            </p>
          ) : null}
          {status.shell.state === 'unknown' ? (
            <p className="settings__hint">{t(translate, 'cli-shell-unknown')}</p>
          ) : null}
          {status.shell.state === 'not-found' ? (
            <p className="settings__hint">{t(translate, 'cli-shell-not-found')}</p>
          ) : null}
          {status.detail ? <p className="settings__hint">{status.detail}</p> : null}
          {status.packageManaged ? (
            <p className="settings__hint">{t(translate, 'cli-managed')}</p>
          ) : null}
          <div className="cli-settings__actions">
            {status.canInstall &&
            !shellConflict &&
            status.state !== 'available' &&
            status.state !== 'not-on-path' ? (
              <button
                type="button"
                className="cli-settings__button cli-settings__button--primary"
                disabled={pending}
                aria-busy={pending}
                data-testid="cli-install"
                onClick={() => void run(ipc.installCli)}
              >
                {pending ? <span className="cli-settings__spinner" aria-hidden="true" /> : null}
                {t(translate, status.state === 'broken' ? 'cli-repair' : 'cli-install')}
              </button>
            ) : null}
            {status.canRemove ? (
              <button
                type="button"
                className="cli-settings__button cli-settings__button--danger"
                disabled={pending}
                data-testid="cli-uninstall"
                onClick={() => void run(ipc.uninstallCli)}
              >
                {t(translate, 'cli-uninstall')}
              </button>
            ) : null}
            <button
              type="button"
              className="cli-settings__button cli-settings__button--secondary"
              disabled={pending}
              data-testid="cli-refresh"
              onClick={() => void run(ipc.getCliStatus)}
            >
              {pending ? <span className="cli-settings__spinner" aria-hidden="true" /> : null}
              {t(translate, 'cli-refresh')}
            </button>
          </div>
          {status.pathCommand &&
          status.shell.state !== 'available' &&
          status.shell.state !== 'conflict' ? (
            <div className="cli-settings__path">
              <p className="settings__hint">{t(translate, 'cli-path-hint')}</p>
              <div className="settings__hint mono">{status.pathCommand}</div>
              <button
                type="button"
                className="cli-settings__button cli-settings__button--secondary"
                data-testid="cli-copy-path"
                onClick={() => {
                  setError('');
                  void writeText(status.pathCommand!).then(
                    () => setCopied(true),
                    (error) => setError(String(error))
                  );
                }}
              >
                {copied ? <span aria-hidden="true">✓</span> : null}
                {t(translate, 'cli-copy-path')}
              </button>
              {copied ? (
                <span className="cli-settings__copied" role="status">
                  ✓
                </span>
              ) : null}
            </div>
          ) : null}
        </>
      ) : null}
      {!status ? (
        <button
          type="button"
          className="cli-settings__button cli-settings__button--secondary"
          disabled={pending}
          data-testid="cli-refresh"
          onClick={() => void run(ipc.getCliStatus)}
        >
          {t(translate, 'cli-refresh')}
        </button>
      ) : null}
      {error ? (
        <p className="status-conflict" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
