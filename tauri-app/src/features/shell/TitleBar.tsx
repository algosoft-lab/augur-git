/**
 * The title bar: the in-window menu, tabs, drag region, app name, and controls.
 *
 * The main window draws its own macOS traffic lights inside the tab row.
 */

import { Icon } from '../../components/Icon';
import * as ipc from '../../bridge/ipc';
import { Menu, type MenuItemSpec } from '../../components/controls';
import { activeRepo, useStore } from '../../app/store';
import { dispatchMenuAction } from '../../app/menu';
import { t } from '../../i18n/strings';
import { TabBar } from './TabBar';
import { IS_MACOS, WindowControls } from './WindowControls';
import { handleTitleBarMouseDown } from './titleBarDrag';
import type { WindowMode } from '../../bridge/types';
import { createBranchMenuItems } from '../repository/Toolbar';

export function TitleBar({
  onOpenRepository,
  onOpenWslRepository,
  onNewTab,
  sidecar = false,
  windowMode,
  onToggleMode
}: {
  onOpenRepository: () => void;
  onOpenWslRepository: () => void;
  onNewTab: () => void;
  sidecar?: boolean;
  windowMode: WindowMode;
  onToggleMode: () => void;
}) {
  const translate = useStore((state) => state.t);
  const build = useStore((state) => state.build);
  const recent = useStore((state) => state.config.recent_repos);
  const openTab = useStore((state) => state.openTab);
  const tabs = useStore((state) => state.tabs);
  const activeTabKey = useStore((state) => state.activeTabKey);
  const selectTab = useStore((state) => state.selectTab);
  const repos = useStore((state) => state.repos);
  const repo = useStore((state) => activeRepo(state));

  const hasRemote = Boolean(repo && repo.refs.remotes.length > 0);
  const networkAvailable = hasRemote && !repo?.busy;

  const recentItems: MenuItemSpec[] = recent.length
    ? recent.map((repo, index) => ({
        id: `recent-${index}`,
        label: repo.location.kind === 'wsl' ? `${repo.location.distro} · ${repo.path}` : repo.path,
        icon: <Icon name="git-branch" />,
        onSelect: () => {
          void openTab(repo.path, repo.location);
        }
      }))
    : [{ id: 'recent-empty', label: t(translate, 'menu-no-recent-repositories'), disabled: true }];

  const fileItems: MenuItemSpec[] = [
    {
      id: 'open-repository',
      label: t(translate, 'menu-open-repository'),
      icon: <Icon name="file" />,
      onSelect: onOpenRepository
    },
    ...(typeof navigator !== 'undefined' && /Win/i.test(navigator.platform)
      ? [
          {
            id: 'open-wsl-repository',
            label: t(translate, 'menu-open-wsl-repository'),
            icon: <Icon name="file" />,
            onSelect: onOpenWslRepository
          } satisfies MenuItemSpec
        ]
      : []),
    {
      id: 'new-tab',
      label: t(translate, 'menu-new-tab'),
      icon: <Icon name="plus" />,
      onSelect: onNewTab
    },
    {
      id: 'recent-repositories',
      label: t(translate, 'menu-recent-repositories'),
      icon: <Icon name="git-branch" />,
      separatorBefore: true,
      children: recentItems
    }
  ];

  const branchItems = createBranchMenuItems(
    repo,
    (key) => t(translate, key),
    (action) => dispatchMenuAction(`menu.branch.${action}`)
  );

  const editItems: MenuItemSpec[] = [
    {
      id: 'fetch',
      label: t(translate, 'toolbar-fetch'),
      icon: <Icon name="download" />,
      disabled: !networkAvailable,
      onSelect: () => dispatchMenuAction('menu.repo.fetch')
    },
    {
      id: 'pull',
      label: t(translate, 'toolbar-pull'),
      icon: <Icon name="download" />,
      disabled: !networkAvailable || Boolean(repo?.hasConflicts),
      onSelect: () => dispatchMenuAction('menu.repo.pull')
    },
    {
      id: 'push',
      label: t(translate, 'toolbar-push'),
      icon: <Icon name="upload" />,
      disabled: !networkAvailable,
      onSelect: () => dispatchMenuAction('menu.repo.push')
    },
    {
      id: 'refresh',
      label: t(translate, 'toolbar-refresh'),
      icon: <Icon name="refresh-cw" />,
      disabled: !repo,
      onSelect: () => dispatchMenuAction('menu.repo.refresh')
    },
    {
      id: 'branch',
      label: t(translate, 'menu-branch'),
      icon: <Icon name="git-branch" />,
      disabled: !repo || repo.busy,
      separatorBefore: true,
      children: branchItems
    },
    {
      id: 'settings',
      label: t(translate, 'menu-settings'),
      icon: <Icon name="settings" />,
      separatorBefore: true,
      onSelect: () => {
        void ipc.openSettingsWindow();
      }
    }
  ];

  const viewItems: MenuItemSpec[] = [
    {
      id: 'mode-toggle',
      label: t(
        translate,
        windowMode === 'desktop' ? 'sidecar-switch-mode' : 'sidecar-switch-desktop'
      ),
      icon: <Icon name="panel-right" />,
      onSelect: onToggleMode
    },
    {
      id: 'diff-font-increase',
      label: t(translate, 'shortcut-diff-font-increase'),
      icon: <Icon name="plus" />,
      separatorBefore: true,
      onSelect: () => dispatchMenuAction('menu.view.diff-font-increase')
    },
    {
      id: 'diff-font-decrease',
      label: t(translate, 'shortcut-diff-font-decrease'),
      icon: <Icon name="minus" />,
      onSelect: () => dispatchMenuAction('menu.view.diff-font-decrease')
    },
    {
      id: 'diff-font-reset',
      label: t(translate, 'shortcut-diff-font-reset'),
      icon: <Icon name="undo" />,
      onSelect: () => dispatchMenuAction('menu.view.diff-font-reset')
    },
    {
      id: 'appearance',
      label: t(translate, 'menu-appearance'),
      icon: <Icon name="settings" />,
      separatorBefore: true,
      onSelect: () => dispatchMenuAction('menu.view.appearance')
    }
  ];

  const helpItems: MenuItemSpec[] = [
    {
      id: 'about',
      label: t(translate, 'menu-about'),
      icon: <Icon name="file" />,
      onSelect: () => {
        void ipc.openAboutWindow();
      }
    }
  ];

  const menuItems: MenuItemSpec[] = [
    { id: 'file', label: t(translate, 'menu-file'), children: fileItems },
    { id: 'edit', label: t(translate, 'menu-edit'), children: editItems },
    { id: 'view', label: t(translate, 'menu-view'), children: viewItems },
    { id: 'help', label: t(translate, 'menu-help'), children: helpItems },
    {
      id: 'quit',
      label: t(translate, 'menu-quit'),
      danger: true,
      separatorBefore: true,
      onSelect: async () => {
        await ipc.flushState();
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        await getCurrentWindow().close();
      }
    }
  ];

  return (
    <div
      className={`title-bar${IS_MACOS ? ' title-bar--macos' : ''}${sidecar ? ' title-bar--sidecar' : ''}`}
      data-testid="title-bar"
      onMouseDown={handleTitleBarMouseDown}
    >
      {!IS_MACOS ? (
        <Menu items={menuItems} testId="menu-file">
          <button
            type="button"
            className="tool-button tool-button--compact title-bar__menu-trigger"
            title={t(translate, 'menu-open')}
            aria-label={t(translate, 'menu-open')}
          >
            <Icon name="menu" size={15} />
          </button>
        </Menu>
      ) : null}
      {build?.name && !sidecar ? (
        <span className="title-bar__brand">
          <span>{build.name}</span>
          <Icon name="git-branch" size={14} />
        </span>
      ) : null}
      {sidecar ? (
        <select
          className="title-bar__repo-select"
          value={activeTabKey ?? ''}
          aria-label={t(translate, 'sidecar-select-repository')}
          data-testid="sidecar-repository-select"
          onChange={(event) => void selectTab(event.currentTarget.value)}
        >
          {tabs.map((tab) => {
            const repo = tab.repoId === null ? null : repos[tab.repoId];
            const label = repo?.path ?? (tab.path || t(translate, 'status-no-repo-selected'));
            return (
              <option key={tab.key} value={tab.key}>
                {label.split(/[\\/]/).filter(Boolean).slice(-1)[0] ?? label}
              </option>
            );
          })}
        </select>
      ) : (
        <TabBar onNewTab={onNewTab} />
      )}
      <div className="title-bar__drag" {...(IS_MACOS ? { 'data-tauri-drag-region': true } : {})} />
      <button
        type="button"
        className="title-bar__settings"
        title={t(translate, 'menu-settings')}
        aria-label={t(translate, 'menu-settings')}
        data-testid="title-settings"
        onClick={() => {
          void ipc.openSettingsWindow();
        }}
      >
        <Icon name="settings" size={14} />
      </button>
      <button
        type="button"
        className="title-bar__settings"
        title={t(
          translate,
          windowMode === 'sidecar' ? 'sidecar-switch-desktop' : 'sidecar-switch-mode'
        )}
        aria-label={t(
          translate,
          windowMode === 'sidecar' ? 'sidecar-switch-desktop' : 'sidecar-switch-mode'
        )}
        data-testid="title-sidecar-toggle"
        onClick={onToggleMode}
      >
        <Icon name="panel-right" size={14} />
      </button>
      {!IS_MACOS ? <WindowControls flushBeforeClose /> : null}
    </div>
  );
}
