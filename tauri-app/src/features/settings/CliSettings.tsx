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
  return (
    <div className="settings__field" data-testid="settings-cli">
      <span className="settings__label">{t(translate, 'cli-title')}</span>
      <p className="settings__hint">{t(translate, 'cli-description')}</p>
      {status ? (
        <>
          <div data-testid="cli-status">{t(translate, `cli-status-${status.state}`)}</div>
          <div className="settings__hint mono">{status.path}</div>
          {status.detail ? <p className="settings__hint">{status.detail}</p> : null}
          {status.packageManaged ? (
            <p className="settings__hint">{t(translate, 'cli-managed')}</p>
          ) : null}
          {status.canInstall && status.state !== 'available' && status.state !== 'not-on-path' ? (
            <button
              type="button"
              className="toolbar-button"
              disabled={pending}
              data-testid="cli-install"
              onClick={() => void run(ipc.installCli)}
            >
              {t(translate, status.state === 'broken' ? 'cli-repair' : 'cli-install')}
            </button>
          ) : null}
          {status.canRemove ? (
            <button
              type="button"
              className="toolbar-button"
              disabled={pending}
              data-testid="cli-uninstall"
              onClick={() => void run(ipc.uninstallCli)}
            >
              {t(translate, 'cli-uninstall')}
            </button>
          ) : null}
          {status.pathCommand ? (
            <>
              <p className="settings__hint">{t(translate, 'cli-path-hint')}</p>
              <div className="settings__hint mono">{status.pathCommand}</div>
              <button
                type="button"
                className="toolbar-button"
                onClick={() =>
                  void writeText(status.pathCommand!).catch((error) => setError(String(error)))
                }
              >
                {t(translate, 'cli-copy-path')}
              </button>
            </>
          ) : null}
        </>
      ) : null}
      <button
        type="button"
        className="toolbar-button"
        disabled={pending}
        data-testid="cli-refresh"
        onClick={() => void run(ipc.getCliStatus)}
      >
        {t(translate, 'cli-refresh')}
      </button>
      {error ? (
        <p className="status-conflict" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
