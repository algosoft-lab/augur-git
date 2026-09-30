/**
 * The About window.
 *
 * A separate window rather than a settings section, matching the reference
 * application. It reports the identity, version, and build commit of this
 * application and where its data lives.
 */

import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import { openUrl } from '@tauri-apps/plugin-opener';
import { t, ta } from '../../i18n/strings';
import { IS_MACOS } from '../shell/WindowControls';
import { handleTitleBarMouseDown } from '../shell/titleBarDrag';
import { HomebrewUpgradeHint } from '../updates/HomebrewUpgradeHint';

const releaseUrl = 'https://github.com/algosoft-lab/augur-git/releases/tag/tauri-nightly';

function updateStatusText(
  status: ReturnType<typeof useStore.getState>['updateStatus'],
  translate: ReturnType<typeof useStore.getState>['t']
) {
  if (!status) return t(translate, 'update-status-idle');
  if (status.error) return status.error;
  switch (status.state) {
    case 'checking':
      return t(translate, 'update-status-checking');
    case 'up-to-date':
      return t(translate, 'update-status-up-to-date');
    case 'available':
      return ta(translate, 'update-status-available', {
        version: status.latestVersion ?? ''
      });
    case 'downloading':
      return t(translate, 'update-status-downloading');
    case 'downloaded':
      return t(translate, 'update-status-downloaded');
    case 'error':
      return status.error ?? t(translate, 'update-status-error');
    default:
      return t(translate, 'update-status-idle');
  }
}

export function AboutWindow() {
  const translate = useStore((state) => state.t);
  const build = useStore((state) => state.build);
  const storePaths = useStore((state) => state.storePaths);
  const updateStatus = useStore((state) => state.updateStatus);
  const config = useStore((state) => state.config);
  const setUpdateStatus = useStore((state) => state.setUpdateStatus);
  const checking = updateStatus?.state === 'checking';
  const downloading = updateStatus?.state === 'downloading';

  return (
    <div className="window-page">
      {/* The shared title bar reserves the space the macOS traffic lights overlay on. */}
      <div
        className={`window-titlebar${IS_MACOS ? ' window-titlebar--macos' : ''}`}
        onMouseDown={handleTitleBarMouseDown}
      >
        <span className="about__title" data-testid="about-title">
          {t(translate, 'about-title')}
        </span>
        <div
          className="window-titlebar__drag"
          {...(IS_MACOS ? { 'data-tauri-drag-region': true } : {})}
        />
      </div>
      <div className="about about--scroll" data-testid="about">
        <div className="about__identity">
          <img className="about__logo" src="/logo.svg" alt="" />
          <div className="about__name">{build?.name ?? 'Augur Git'}</div>
          <div className="about__tagline">{t(translate, 'about-tagline')}</div>
        </div>
        <dl className="about__rows">
          <div className="about__row">
            <dt>{t(translate, 'about-version')}</dt>
            <dd data-testid="about-version">{build?.version ?? ''}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'about-commit')}</dt>
            <dd data-testid="about-commit">{build?.commit ?? 'unknown'}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'about-author')}</dt>
            <dd data-testid="about-author">{build?.authors ?? ''}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'app-identifier')}</dt>
            <dd data-testid="about-identifier">{build?.identifier ?? ''}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'about-platform')}</dt>
            <dd data-testid="about-platform">{build?.platform ?? ''}</dd>
          </div>
          {storePaths.map((path) => (
            <div className="about__row" key={path}>
              <dt>{t(translate, 'app-data-dir')}</dt>
              <dd title={path}>{path}</dd>
            </div>
          ))}
        </dl>
        <section className="about__updates" aria-label={t(translate, 'updates-title')}>
          <div className="about__updates-title">{t(translate, 'updates-title')}</div>
          <label className="about__auto-check">
            <input
              type="checkbox"
              checked={config.auto_check_updates}
              data-testid="auto-check-updates"
              onChange={(event) =>
                void ipc.setAutoCheckUpdates(event.target.checked).catch(() => undefined)
              }
            />
            <span>{t(translate, 'auto-check-updates')}</span>
          </label>
          <div className="about__update-status" role="status" data-testid="update-status">
            {updateStatusText(updateStatus, translate)}
            {updateStatus?.progress != null ? ` ${Math.round(updateStatus.progress)}%` : ''}
          </div>
          {updateStatus?.installChannel === 'homebrew-cask' ? <HomebrewUpgradeHint /> : null}
          <div className="about__update-actions">
            <button
              type="button"
              className="toolbar-button"
              data-testid="check-for-updates"
              disabled={checking || downloading}
              onClick={() =>
                void ipc
                  .checkForUpdates()
                  .then(setUpdateStatus)
                  .catch(() => undefined)
              }
            >
              {t(translate, 'check-for-updates')}
            </button>
            {updateStatus?.state === 'available' && updateStatus.canInstall ? (
              <button
                type="button"
                className="primary-button"
                data-testid="download-update"
                onClick={() =>
                  void ipc
                    .downloadUpdate()
                    .then(setUpdateStatus)
                    .catch(() => undefined)
                }
              >
                {t(translate, 'download-update')}
              </button>
            ) : null}
            {updateStatus?.state === 'downloaded' && updateStatus.canInstall ? (
              <button
                type="button"
                className="primary-button"
                data-testid="install-update"
                onClick={() => void ipc.installUpdate().catch(() => undefined)}
              >
                {t(translate, 'install-update')}
              </button>
            ) : null}
            <button
              type="button"
              className="toolbar-button"
              data-testid="open-release-page"
              onClick={() => void openUrl(releaseUrl).catch(() => undefined)}
            >
              {t(translate, 'open-release-page')}
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}
