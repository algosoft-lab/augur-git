import * as ipc from '../../bridge/ipc';
import { t, ta } from '../../i18n/strings';
import { THEME_GROUPS } from '../../styles/theme-catalog';
import { activeRepo, hasLocalBranches, useStore } from '../../app/store';
import { triggerPull, triggerPush } from '../repository/Toolbar';

export type PaletteGroup = 'repository' | 'git' | 'branches' | 'view' | 'themes' | 'app';

export interface PaletteCommand {
  id: string;
  group: PaletteGroup;
  label: string;
  keywords: string[];
  enabled: boolean;
  active?: boolean;
  run: () => void | Promise<void>;
}

export function buildPaletteCommands(): PaletteCommand[] {
  const state = useStore.getState();
  const repo = activeRepo(state);
  const commands: PaletteCommand[] = [];
  const add = (
    id: string,
    group: PaletteGroup,
    label: string,
    keywords: string[],
    enabled: boolean,
    run: PaletteCommand['run'],
    active = false
  ) => commands.push({ id, group, label, keywords, enabled, run, active });
  const send = (name: string): void => {
    window.dispatchEvent(new CustomEvent(name));
  };

  add(
    'repository.open',
    'repository',
    t(state.t, 'menu-open-repository'),
    ['open', 'folder', 'repository', 'repo'],
    true,
    () => send('augur:open-repository')
  );
  state.config.recent_repos.forEach((recent, index) => {
    const pathParts = recent.path.split(/[\\/]/).filter(Boolean);
    const name = pathParts[pathParts.length - 1] ?? recent.path;
    add(
      `repository.recent.${index}`,
      'repository',
      ta(state.t, 'palette-open-recent', { name }),
      ['open', 'recent', 'repository', 'repo', recent.path, name],
      true,
      () => useStore.getState().openTab(recent.path, recent.location)
    );
  });
  add(
    'repository.new-tab',
    'repository',
    t(state.t, 'menu-new-tab'),
    ['new', 'tab', 'start'],
    true,
    () => send('augur:new-tab')
  );
  add(
    'repository.close-tab',
    'repository',
    t(state.t, 'palette-close-tab'),
    ['close', 'tab'],
    state.activeTabKey !== null,
    () => {
      const activeTabKey = useStore.getState().activeTabKey;
      if (activeTabKey) return useStore.getState().closeTab(activeTabKey);
    }
  );
  add(
    'repository.refresh',
    'repository',
    t(state.t, 'toolbar-refresh'),
    ['refresh', 'reload', 'repository', 'repo'],
    !!repo,
    () => {
      const current = activeRepo(useStore.getState());
      if (current) return useStore.getState().refresh(current.id);
    }
  );

  const hasRemote = !!repo && repo.refs.remotes.length > 0;
  const repoReady = !!repo && !repo.busy;
  add(
    'git.pull',
    'git',
    t(state.t, 'toolbar-pull'),
    ['pull', 'update', 'sync', 'fetch'],
    !!repo && hasRemote && !repo.busy && !repo.hasConflicts,
    () => {
      const current = activeRepo(useStore.getState());
      if (current) triggerPull(current);
    }
  );
  add(
    'git.push',
    'git',
    t(state.t, 'toolbar-push'),
    ['push', 'publish', 'upload', 'sync'],
    !!repo && hasRemote && !repo.busy,
    () => {
      const current = activeRepo(useStore.getState());
      if (current) triggerPush(current);
    }
  );
  add(
    'git.fetch',
    'git',
    t(state.t, 'toolbar-fetch'),
    ['fetch', 'download', 'remote', 'sync'],
    !!repo && hasRemote && !repo.busy,
    () => {
      const current = activeRepo(useStore.getState());
      if (current && current.refs.remotes.length > 0 && !current.busy) {
        return useStore.getState().runAction(current.id, { action: 'fetch' });
      }
    }
  );
  add(
    'git.new-branch',
    'git',
    t(state.t, 'menu-branch-new'),
    ['branch', 'new', 'create'],
    repoReady && !repo!.hasConflicts,
    () => useStore.getState().openOverlay({ kind: 'newBranch' })
  );
  add(
    'git.merge',
    'git',
    t(state.t, 'menu-merge'),
    ['merge', 'integrate', 'branch'],
    repoReady && !repo!.hasConflicts && hasLocalBranches(repo!),
    () => useStore.getState().openOverlay({ kind: 'merge', noFf: false })
  );
  add(
    'git.rebase',
    'git',
    t(state.t, 'menu-rebase'),
    ['rebase', 'integrate', 'branch'],
    repoReady && !repo!.hasConflicts && hasLocalBranches(repo!),
    () => useStore.getState().openOverlay({ kind: 'rebase' })
  );
  add(
    'git.stash',
    'git',
    t(state.t, 'menu-stash'),
    ['stash', 'shelve', 'save changes'],
    repoReady && repo!.stashableCount > 0 && !repo!.hasConflicts,
    () => useStore.getState().openOverlay({ kind: 'stash' })
  );
  add(
    'git.reset',
    'git',
    t(state.t, 'menu-reset'),
    ['reset', 'undo', 'discard', 'commit'],
    repoReady && !!repo!.head && !repo!.hasConflicts,
    () => useStore.getState().openOverlay({ kind: 'reset' })
  );
  add(
    'git.manage-remotes',
    'git',
    t(state.t, 'toolbar-manage-remotes'),
    ['remote', 'remotes', 'manage', 'origin', 'url'],
    !!repo,
    () => useStore.getState().openOverlay({ kind: 'manageRemotes' })
  );

  if (repo) {
    repo.branches.forEach((branch) => {
      add(
        `branch.checkout.${branch.name}`,
        'branches',
        ta(state.t, 'palette-checkout-branch', { name: branch.name }),
        ['checkout', 'switch', 'branch', branch.name],
        !repo.busy && !repo.hasConflicts && !branch.is_head,
        () =>
          useStore.getState().runAction(repo.id, {
            action: 'checkout',
            target: { kind: 'localBranch', localBranch: branch.name }
          }),
        branch.is_head
      );
    });
  }

  add(
    'view.mode',
    'view',
    t(
      state.t,
      state.workspace.window_mode === 'sidecar' ? 'sidecar-switch-desktop' : 'sidecar-switch-mode'
    ),
    ['window', 'mode', 'sidecar', 'desktop', 'toggle'],
    true,
    () => send('augur:toggle-window-mode')
  );
  add(
    'view.changes',
    'view',
    t(state.t, 'sidecar-changes'),
    ['changes', 'working tree', 'files'],
    !!repo && state.workspace.window_mode === 'sidecar',
    () => {
      const current = activeRepo(useStore.getState());
      if (current) useStore.getState().patchSidecarUi(current.id, { page: 'changes' });
    },
    !!repo &&
      state.workspace.window_mode === 'sidecar' &&
      state.sidecarUi[repo.id]?.page === 'changes'
  );
  add(
    'view.history',
    'view',
    t(state.t, 'sidecar-history'),
    ['history', 'commits', 'log', 'graph'],
    !!repo && state.workspace.window_mode === 'sidecar',
    () => {
      const current = activeRepo(useStore.getState());
      if (current) useStore.getState().patchSidecarUi(current.id, { page: 'history' });
    },
    !!repo &&
      state.workspace.window_mode === 'sidecar' &&
      state.sidecarUi[repo.id]?.page === 'history'
  );
  add(
    'view.branches',
    'view',
    t(state.t, 'section-branches'),
    ['branches', 'refs'],
    !!repo && state.workspace.window_mode === 'sidecar',
    () => {
      const current = activeRepo(useStore.getState());
      if (current) useStore.getState().patchSidecarUi(current.id, { page: 'branches' });
    },
    !!repo &&
      state.workspace.window_mode === 'sidecar' &&
      state.sidecarUi[repo.id]?.page === 'branches'
  );
  add(
    'view.commit-message',
    'view',
    t(state.t, 'shortcut-commit-focus'),
    ['focus', 'commit', 'message', 'editor'],
    !!repo &&
      (state.workspace.window_mode === 'desktop' || state.sidecarUi[repo.id]?.page === 'changes'),
    () => document.querySelector<HTMLTextAreaElement>('[data-testid="commit-message"]')?.focus()
  );
  add(
    'view.commit-search',
    'view',
    t(state.t, 'shortcut-graph-search'),
    ['focus', 'search', 'history', 'commits', 'graph'],
    !!repo &&
      (state.workspace.window_mode === 'desktop' || state.sidecarUi[repo.id]?.page === 'history'),
    () => document.querySelector<HTMLInputElement>('[data-testid="commit-search"]')?.focus()
  );

  THEME_GROUPS.forEach((themeGroup) => {
    themeGroup.themes.forEach((theme) => {
      const label = theme.labelKey ? t(state.t, theme.labelKey) : theme.name;
      add(
        `theme.${theme.value}`,
        'themes',
        label,
        ['theme', 'appearance', themeGroup.name, theme.name, theme.value],
        theme.value !== state.config.theme,
        () => useStore.getState().setTheme(theme.value),
        theme.value === state.config.theme
      );
    });
  });

  add(
    'app.settings',
    'app',
    t(state.t, 'menu-settings'),
    ['settings', 'preferences', 'configuration'],
    true,
    () => ipc.openSettingsWindow()
  );
  add(
    'app.shortcuts',
    'app',
    t(state.t, 'palette-edit-shortcuts'),
    ['settings', 'shortcuts', 'keybindings', 'keyboard'],
    true,
    () => ipc.openSettingsWindow('shortcuts')
  );
  add(
    'app.compare',
    'app',
    t(state.t, 'toolbar-compare'),
    ['compare', 'diff', 'revision'],
    !!repo && !repo.busy,
    () => {
      const current = activeRepo(useStore.getState());
      if (current) return ipc.openCompareWindow(current.id).then(() => undefined);
    }
  );
  add(
    'app.check-updates',
    'app',
    t(state.t, 'check-for-updates'),
    ['updates', 'update', 'version', 'upgrade'],
    true,
    async () => {
      try {
        const status = await ipc.checkForUpdates();
        useStore.getState().setUpdateStatus(status);
      } catch {
        useStore.getState().notify({
          level: 'error',
          message: useStore.getState().t('palette-command-failed')
        });
      }
    }
  );

  return commands;
}
