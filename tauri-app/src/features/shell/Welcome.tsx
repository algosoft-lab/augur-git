/** The welcome page shown when no repository is open. */

import { Icon } from '../../components/Icon';
import { useStore } from '../../app/store';
import { t } from '../../i18n/strings';

export function Welcome({
  onOpenRepository,
  onOpenWslRepository
}: {
  onOpenRepository: () => void;
  onOpenWslRepository: () => void;
}) {
  const translate = useStore((state) => state.t);
  const recent = useStore((state) => state.config.recent_repos);
  const openTab = useStore((state) => state.openTab);
  const build = useStore((state) => state.build);
  const isWindows = typeof navigator !== 'undefined' && /Win/i.test(navigator.platform);

  return (
    <div className="welcome" data-testid="welcome" data-page="welcome">
      <div className="welcome__brand">
        <img src="/logo.svg" alt="" />
        <div>
          <div className="welcome__title">{build?.name ?? 'Augur Git'}</div>
          <div className="welcome__tagline">{t(translate, 'app-tagline')}</div>
        </div>
      </div>
      <div className="welcome__actions">
        <button
          type="button"
          className="tool-button tool-button--primary"
          data-testid="welcome-open"
          onClick={onOpenRepository}
        >
          {t(translate, 'welcome-open')}
        </button>
        {isWindows ? (
          <button
            type="button"
            className="tool-button"
            data-testid="welcome-open-wsl"
            onClick={onOpenWslRepository}
          >
            {t(translate, 'welcome-open-wsl')}
          </button>
        ) : null}
      </div>
      <div className="welcome__hint">
        <Icon name="download" size={12} /> {t(translate, 'welcome-drop-hint')}
      </div>
      {recent.length ? (
        <div className="welcome__recent">
          <div className="welcome__recent-title">{t(translate, 'recent-repos')}</div>
          {recent.map((repo) => (
            <button
              key={`${repo.location.kind}:${repo.path}`}
              type="button"
              className="welcome__recent-item"
              data-testid={`recent-${repo.path}`}
              title={repo.path}
              onClick={() => {
                void openTab(repo.path, repo.location);
              }}
            >
              <Icon name="git-branch" size={12} />
              <span className="welcome__recent-path">
                {repo.location.kind === 'wsl'
                  ? `${repo.location.distro} · ${repo.path}`
                  : repo.path}
              </span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
