import { useEffect } from 'react';

import { Icon } from '../../components/Icon';
import { Spinner } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import { useStore, type RepoState, type SidecarListPage } from '../../app/store';
import { t } from '../../i18n/strings';
import { BottomPanel } from '../diff/BottomPanel';
import { GraphView } from '../graph/GraphView';
import { ChangesPanel } from '../repository/ChangesPanel';
import { CommitPanel } from '../repository/CommitPanel';
import { Sidebar } from '../repository/Sidebar';
import { Toolbar } from '../repository/Toolbar';
import { IS_MACOS } from './WindowControls';

const NAV_ITEMS: {
  page: SidecarListPage;
  icon: 'archive' | 'git-commit-horizontal' | 'git-branch';
  key: string;
}[] = [
  { page: 'changes', icon: 'archive', key: 'sidecar-changes' },
  { page: 'history', icon: 'git-commit-horizontal', key: 'sidecar-history' },
  { page: 'branches', icon: 'git-branch', key: 'section-branches' }
];

export function SidecarWindow({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const ui = useStore((state) => state.sidecarUi[repo.id]);
  const patchSidecarUi = useStore((state) => state.patchSidecarUi);
  const setWindowMode = useStore((state) => state.setWindowMode);
  const page = ui?.page ?? 'changes';
  const returnPage = ui?.diffReturnPage ?? 'changes';
  const activePage = page === 'diff' ? returnPage : page;

  useEffect(() => {
    if (page === 'diff' && repo.pane.kind === 'none') {
      patchSidecarUi(repo.id, { page: returnPage });
    }
  }, [page, repo.id, repo.pane.kind, returnPage, patchSidecarUi]);

  const navigate = (next: SidecarListPage) => patchSidecarUi(repo.id, { page: next });
  const openDiff = (from: SidecarListPage) =>
    patchSidecarUi(repo.id, { page: 'diff', diffReturnPage: from });
  const backFromDiff = () => navigate(returnPage);
  const pageTitle =
    page === 'diff'
      ? t(translate, 'sidecar-diff')
      : t(translate, NAV_ITEMS.find((item) => item.page === page)?.key ?? 'changes-title');

  return (
    <div className="sidecar" data-testid="sidecar-window">
      <nav className="sidecar__rail" aria-label={t(translate, 'sidecar-navigation')}>
        <div className="sidecar__rail-main">
          {NAV_ITEMS.map((item) => (
            <button
              key={item.page}
              type="button"
              className={`sidecar__rail-button${activePage === item.page ? ' is-active' : ''}`}
              aria-label={t(translate, item.key)}
              aria-current={activePage === item.page ? 'page' : undefined}
              title={t(translate, item.key)}
              data-testid={`sidecar-nav-${item.page}`}
              onClick={() => navigate(item.page)}
            >
              <Icon name={item.icon} size={17} />
            </button>
          ))}
        </div>
        <div className="sidecar__rail-bottom">
          <button
            type="button"
            className="sidecar__rail-button"
            aria-label={t(translate, 'menu-settings')}
            title={t(translate, 'menu-settings')}
            data-testid="sidecar-settings"
            onClick={() => void ipc.openSettingsWindow()}
          >
            <Icon name="settings" size={16} />
          </button>
          <button
            type="button"
            className="sidecar__rail-button"
            aria-label={t(translate, 'sidecar-switch-desktop')}
            title={t(translate, 'sidecar-switch-desktop')}
            data-testid="sidecar-mode-toggle"
            onClick={() => void setWindowMode('desktop')}
          >
            <Icon name="panel-right" size={16} />
          </button>
        </div>
      </nav>
      <main className={`sidecar__main${IS_MACOS ? ' sidecar__main--macos' : ''}`}>
        <div className="sidecar__heading">
          <span className="sidecar__heading-title" data-testid="sidecar-page-title">
            {pageTitle}
          </span>
          {page === 'changes' ? (
            <span className="sidecar__branch-state" title={repo.branch}>
              <Icon name="git-branch" size={12} />
              <span>
                {repo.branch || repo.head?.slice(0, 8) || t(translate, 'status-no-repo-selected')}
              </span>
            </span>
          ) : null}
        </div>
        <Toolbar repo={repo} compact />
        {page === 'diff' ? (
          <section className="sidecar__view" data-testid="sidecar-page-diff">
            <BottomPanel
              repo={repo}
              height={null}
              sidecar
              onBack={backFromDiff}
              onFileListRatioChange={() => undefined}
              onFileListRatioChangeEnd={() => undefined}
            />
          </section>
        ) : (
          <>
            <section className="sidecar__view" data-testid={`sidecar-page-${page}`}>
              {page === 'changes' ? (
                <div className="sidecar__changes-layout">
                  <ChangesPanel repo={repo} compact onOpenDiff={() => openDiff('changes')} />
                  <div className="sidecar__commit">
                    <CommitPanel repo={repo} />
                  </div>
                </div>
              ) : page === 'history' ? (
                <GraphView repo={repo} compact onOpenDiff={() => openDiff('history')} />
              ) : (
                <div className="sidecar__branches">
                  <Sidebar repo={repo} refs={repo.refs} compact />
                </div>
              )}
            </section>
          </>
        )}
        <div className="sidecar__repo-status" title={repo.path}>
          <span className="sidecar__repo-path">{repo.path}</span>
          {repo.busyVerb ? (
            <span className="sidecar__repo-message sidecar__repo-message--busy">
              <Spinner color="var(--warning-background)" rhythm="two-turn-pause" />
              <span className="sidecar__repo-message-text" data-testid="sidecar-status-busy">
                {repo.busyVerb}
              </span>
            </span>
          ) : null}
          {!repo.busyVerb && repo.message ? (
            <span
              className={`sidecar__repo-message${
                repo.message.ok === false ? ' is-error' : ' is-success'
              }`}
            >
              {repo.message.text}
            </span>
          ) : null}
          {repo.hasConflicts ? (
            <span className="sidecar__conflict-state">{t(translate, 'sidecar-conflicts')}</span>
          ) : null}
        </div>
      </main>
    </div>
  );
}
