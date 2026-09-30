import { openUrl } from '@tauri-apps/plugin-opener';

import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import { t, ta } from '../../i18n/strings';

const releaseUrl = 'https://github.com/algosoft-lab/augur-git/releases/tag/tauri-nightly';

export function UpdateNoticeCard() {
  const translate = useStore((state) => state.t);
  const notice = useStore((state) => state.updateNotice);
  const status = useStore((state) => state.updateStatus);
  const setUpdateNotice = useStore((state) => state.setUpdateNotice);

  if (!notice) return null;

  const isWindowsInstaller = status?.installChannel === 'windows-installer';
  const description = ta(translate, 'update-notice-description', {
    version: notice.version,
    commit: notice.commitSha.slice(0, 7)
  });

  return (
    <aside
      className="update-notice"
      role="status"
      aria-live="polite"
      aria-atomic="true"
      data-testid="update-notice"
    >
      <div className="update-notice__heading">
        <strong>{t(translate, 'update-notice-title')}</strong>
        <button
          type="button"
          className="update-notice__close"
          aria-label={t(translate, 'notice-dismiss')}
          onClick={() => {
            void ipc
              .dismissUpdateNotice(notice.commitSha)
              .then(() => setUpdateNotice(null))
              .catch(() => undefined);
          }}
        >
          ×
        </button>
      </div>
      <p>{description}</p>
      {status?.installChannel === 'homebrew-cask' ? (
        <p className="update-notice__homebrew">
          {t(translate, 'homebrew-upgrade-hint')} <code>brew upgrade --cask augur-git</code>
        </p>
      ) : null}
      <div className="update-notice__actions">
        <button
          type="button"
          className="primary-button"
          onClick={() => {
            if (isWindowsInstaller) void ipc.openAboutWindow();
            else void openUrl(releaseUrl).catch(() => undefined);
          }}
        >
          {isWindowsInstaller ? t(translate, 'review-update') : t(translate, 'open-release-page')}
        </button>
        <button
          type="button"
          className="toolbar-button"
          onClick={() => {
            void ipc
              .dismissUpdateNotice(notice.commitSha)
              .then(() => setUpdateNotice(null))
              .catch(() => undefined);
          }}
        >
          {t(translate, 'notice-dismiss')}
        </button>
      </div>
    </aside>
  );
}
