/**
 * Window router and event bootstrap.
 *
 * One bundle serves every window. The role in the URL decides which surface is
 * mounted; the event subscriptions are shared because a settings change or a
 * forwarded CLI path has to reach all of them.
 */

import { Component, useEffect, useRef, useState, type ReactNode } from 'react';

import * as ipc from '../bridge/ipc';
import { AboutWindow } from '../features/about/AboutWindow';
import { CompareWindow } from '../features/compare/CompareWindow';
import { MainWindow } from '../features/shell/MainWindow';
import { SettingsWindow } from '../features/settings/SettingsWindow';
import { applyTheme } from '../styles/themes';
import { activeRepo, hasLocalBranches, useStore, type WindowRole } from './store';
import type { SettingsSection } from '../bridge/types';
import { DEFAULT_THEME, DEFAULT_TYPOGRAPHY } from '../styles/themes';
import { hasOpenPopup, keysForCommand, matchesShortcut } from './keyboard';
import { triggerPull, triggerPush } from '../features/repository/Toolbar';
import { MENU_ACTION_EVENT } from './menu';

interface WindowTarget {
  role: WindowRole;
  compareRepoId: number | null;
  settingsSection: SettingsSection | null;
}

function readTarget(): WindowTarget {
  const params = new URLSearchParams(window.location.search);
  const requested = params.get('window');
  if (requested === 'compare') {
    const repo = Number(params.get('repo'));
    return {
      role: 'compare',
      compareRepoId: Number.isFinite(repo) && repo > 0 ? repo : null,
      settingsSection: null
    };
  }
  if (requested === 'about') {
    return { role: 'about', compareRepoId: null, settingsSection: null };
  }
  if (requested === 'settings') {
    const section = params.get('section');
    const settingsSection: SettingsSection | null =
      section === 'general' ||
      section === 'appearance' ||
      section === 'layout' ||
      section === 'shortcuts'
        ? section
        : null;
    return { role: 'settings', compareRepoId: null, settingsSection };
  }
  return { role: 'main', compareRepoId: null, settingsSection: null };
}

export function App() {
  const target = useRef(readTarget()).current;
  const ready = useStore((state) => state.ready);
  const theme = useStore((state) => state.config.theme);
  const typography = useStore((state) => state.config.typography);
  const [fatal, setFatal] = useState<string | null>(null);
  const [settingsSection, setSettingsSection] = useState<SettingsSection | null>(
    target.settingsSection
  );

  // The document is styled before anything is fetched, so a failure during
  // start-up is a readable message rather than an unstyled white page.
  useEffect(() => {
    applyTheme(document.documentElement, DEFAULT_THEME, DEFAULT_TYPOGRAPHY);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: (() => void)[] = [];
    const onInWindowMenuAction = (event: Event) => {
      const id = (event as CustomEvent<string>).detail;
      if (typeof id === 'string') void handleMenuAction(id);
    };
    window.addEventListener(MENU_ACTION_EVENT, onInWindowMenuAction);
    unlisteners.push(() => window.removeEventListener(MENU_ACTION_EVENT, onInWindowMenuAction));

    const boot = async () => {
      try {
        const store = useStore.getState();
        // A teardown can land before a subscription has resolved, and the
        // listener it produced would then never be removed, so a strict-mode
        // remount would leave a second copy of every handler behind.
        const subscribe = async (pending: Promise<() => void>) => {
          const stop = await pending;
          if (cancelled) {
            stop();
            return;
          }
          unlisteners.push(stop);
        };
        // Subscriptions first: an event that arrives while the interface is
        // still loading must not be dropped on the floor.
        await subscribe(
          ipc.onRepoEvent((event) => {
            useStore.getState().applyEvent(event.repoId, event);
          })
        );
        await subscribe(
          ipc.onAppEvent((event) => {
            if (event.type === 'settingsChanged') {
              void refreshConfig();
            } else if (event.type === 'workspaceChanged') {
              // The main window owns the tab list; other windows ignore it.
            } else if (event.type === 'notice') {
              useStore.getState().notify({
                level: event.level as 'info' | 'warning' | 'error',
                message: event.message
              });
            }
          })
        );
        await subscribe(
          ipc.onUpdateEvent((event) => {
            if (event.type === 'status') {
              useStore.getState().setUpdateStatus(event.status);
            } else {
              useStore.getState().setUpdateNotice(event.notice);
            }
          })
        );
        await subscribe(
          ipc.onOpenPaths((paths) => {
            void useStore.getState().openPaths(paths);
          })
        );
        await subscribe(
          ipc.onDropPaths((paths) => {
            void useStore.getState().openPaths(paths);
          })
        );
        await subscribe(
          ipc.onDragState(({ label, active }) => {
            if (target.role === 'main' && label === 'main') {
              useStore.getState().setDragOver(active);
            }
          })
        );
        await subscribe(
          ipc.onMenuEvent((id) => {
            void handleMenuAction(id);
          })
        );
        await subscribe(ipc.onSettingsNavigate(setSettingsSection));
        await store.initialize(target.role, target.compareRepoId);
        if (!cancelled) {
          applyThemeFromState();
        }
      } catch (error) {
        // The log plugin does not capture the webview console, so a start-up
        // failure is reported through the interface as well.
        console.error('[boot] start-up failed', error);
        if (!cancelled) {
          setFatal(describeFailure(error));
        }
      }
    };

    void boot();
    return () => {
      cancelled = true;
      for (const stop of unlisteners) {
        stop();
      }
    };
  }, [target.role, target.compareRepoId]);

  // Re-apply the theme whenever the preference changes, including when another
  // window changes it.
  useEffect(() => {
    if (ready) {
      applyThemeFromState();
    }
  }, [ready, theme, typography]);

  useEffect(() => {
    if (!ready) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || event.keyCode === 229) {
        return;
      }
      const targetElement = event.target;
      if (!(targetElement instanceof HTMLElement)) {
        return;
      }
      const blockedTarget = (element: Element | null) =>
        element?.closest(
          'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="dialog"]'
        );
      if (blockedTarget(targetElement) || blockedTarget(document.activeElement) || hasOpenPopup()) {
        return;
      }
      const state = useStore.getState();
      if (state.overlay.kind !== 'none') {
        return;
      }
      const matches = (command: string) =>
        matchesShortcut(event, keysForCommand(state.shortcuts.resolved, command));

      if (target.role === 'main') {
        const repo = activeRepo(state);
        if (matches('repo.pull')) {
          event.preventDefault();
          if (repo) triggerPull(repo);
          return;
        }
        if (matches('repo.push')) {
          event.preventDefault();
          if (repo) triggerPush(repo);
          return;
        }
        if (matches('repo.fetch')) {
          event.preventDefault();
          if (repo && repo.refs.remotes.length > 0 && !repo.busy) {
            void state.runAction(repo.id, { action: 'fetch' });
          }
          return;
        }
        if (matches('repo.refresh')) {
          event.preventDefault();
          if (repo) void state.refresh(repo.id);
          return;
        }
        if (matches('commit.focus')) {
          event.preventDefault();
          document.querySelector<HTMLTextAreaElement>('[data-testid="commit-message"]')?.focus();
          return;
        }
        if (matches('graph.search') && targetElement.closest('[data-testid="graph-list"]')) {
          event.preventDefault();
          document.querySelector<HTMLInputElement>('[data-testid="commit-search"]')?.focus();
          return;
        }
      }

      if (target.role !== 'main' && target.role !== 'compare') {
        return;
      }
      const typography = state.config.typography;
      if (matches('diff.font-increase')) {
        event.preventDefault();
        if (typography.diff_font_size < 20) {
          void state.setTypography({ diff_font_size: Math.min(20, typography.diff_font_size + 1) });
        }
      } else if (matches('diff.font-decrease')) {
        event.preventDefault();
        if (typography.diff_font_size > 12) {
          void state.setTypography({ diff_font_size: Math.max(12, typography.diff_font_size - 1) });
        }
      } else if (matches('diff.font-reset')) {
        event.preventDefault();
        if (typography.diff_font_size !== 16) {
          void state.setTypography({ diff_font_size: 16 });
        }
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [ready, target.role]);

  if (fatal) {
    return (
      <div className="app">
        <div className="fatal">
          <div className="fatal__title">The application could not start</div>
          <pre className="fatal__detail" data-testid="fatal">
            {fatal}
          </pre>
        </div>
      </div>
    );
  }

  if (!ready) {
    return <div className="app" />;
  }

  let surface: ReactNode;
  if (target.role === 'about') {
    surface = <AboutWindow />;
  } else if (target.role === 'settings') {
    surface = <SettingsWindow initialSection={settingsSection ?? 'general'} />;
  } else if (target.role === 'compare') {
    surface = <CompareWindow repoId={target.compareRepoId} />;
  } else {
    surface = <MainWindow />;
  }

  return <WindowErrorBoundary>{surface}</WindowErrorBoundary>;
}

/**
 * Contain a render crash to a readable report.
 *
 * Without a boundary React unmounts the whole tree when a render throws, and
 * the window shows as blank; the boundary keeps the failure on screen instead.
 */
class WindowErrorBoundary extends Component<{ children: ReactNode }, { error: unknown }> {
  state: { error: unknown } = { error: null };

  static getDerivedStateFromError(error: unknown): { error: unknown } {
    return { error };
  }

  componentDidCatch(error: unknown): void {
    console.error('[render] window crashed', error);
  }

  render() {
    if (this.state.error !== null) {
      return (
        <div className="app">
          <div className="fatal">
            <div className="fatal__title">The window failed to render</div>
            <pre className="fatal__detail" data-testid="render-crash">
              {describeFailure(this.state.error)}
            </pre>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}

/** Turn any thrown value into something worth reading on screen. */
function describeFailure(error: unknown): string {
  if (error && typeof error === 'object') {
    const record = error as Record<string, unknown>;
    const parts = Object.entries(record)
      .filter(([, value]) => typeof value === 'string' || typeof value === 'number')
      .map(([key, value]) => `${key}: ${value}`);
    if (parts.length) {
      return parts.join('\n');
    }
    try {
      return JSON.stringify(record, null, 2);
    } catch {
      // A value that cannot be serialized still has a string form.
    }
  }
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

/** Write the active theme and typography onto the document root. */
export function applyThemeFromState(): void {
  const { config } = useStore.getState();
  applyTheme(document.documentElement, config.theme, {
    uiFontFamily: config.typography.ui_font_family,
    monoFontFamily: config.typography.mono_font_family,
    uiFontSize: config.typography.ui_font_size,
    diffFontSize: config.typography.diff_font_size
  });
}

/** Pull the current preferences so another window's change is adopted. */
async function refreshConfig(): Promise<void> {
  const config = await ipc.currentConfig();
  const boot = await ipc.bootstrap();
  const state = useStore.getState();
  state.setTranslator(boot.locale, boot.catalogs);
  useStore.setState({ config, shortcuts: boot.shortcuts, storePaths: boot.store_paths });
  applyThemeFromState();
}

/** Route a native menu activation to the same action the in-window menu uses. */
async function handleMenuAction(id: string): Promise<void> {
  const state = useStore.getState();
  if (id.startsWith('menu.recent.')) {
    const indexText = id.slice('menu.recent.'.length);
    const index = /^\d+$/.test(indexText) ? Number(indexText) : -1;
    const repository = Number.isInteger(index) ? state.config.recent_repos[index] : undefined;
    if (repository) {
      await state.openTab(repository.path, repository.location);
    }
    return;
  }
  // The in-window menu dispatches the same DOM events, so both surfaces run one
  // handler.
  switch (id) {
    case 'menu.open-repository':
      globalThis.dispatchEvent(new CustomEvent('augur:open-repository'));
      break;
    case 'menu.open-wsl-repository':
      globalThis.dispatchEvent(new CustomEvent('augur:open-wsl-repository'));
      break;
    case 'menu.new-tab':
      globalThis.dispatchEvent(new CustomEvent('augur:new-tab'));
      break;
    case 'menu.settings':
      await ipc.openSettingsWindow();
      break;
    case 'menu.view.appearance':
      await ipc.openSettingsWindow('appearance');
      break;
    case 'menu.view.mode-toggle':
      window.dispatchEvent(new CustomEvent('augur:toggle-window-mode'));
      break;
    case 'menu.view.diff-font-increase':
    case 'menu.view.diff-font-decrease':
    case 'menu.view.diff-font-reset': {
      const current = state.config.typography.diff_font_size;
      const next =
        id === 'menu.view.diff-font-increase'
          ? Math.min(20, current + 1)
          : id === 'menu.view.diff-font-decrease'
            ? Math.max(12, current - 1)
            : 16;
      if (next !== current) await state.setTypography({ diff_font_size: next });
      break;
    }
    case 'menu.repo.fetch': {
      const repo = activeRepo(state);
      if (repo && repo.refs.remotes.length > 0 && !repo.busy) {
        void state.runAction(repo.id, { action: 'fetch' });
      }
      break;
    }
    case 'menu.repo.pull': {
      const repo = activeRepo(state);
      if (repo) triggerPull(repo);
      break;
    }
    case 'menu.repo.push': {
      const repo = activeRepo(state);
      if (repo) triggerPush(repo);
      break;
    }
    case 'menu.repo.refresh': {
      const repo = activeRepo(state);
      if (repo) void state.refresh(repo.id);
      break;
    }
    case 'menu.branch.branch-new':
    case 'menu.branch.branch-rename':
    case 'menu.branch.stash':
    case 'menu.branch.stash-pop':
    case 'menu.branch.merge':
    case 'menu.branch.merge-no-ff':
    case 'menu.branch.rebase':
    case 'menu.branch.apply-patch':
    case 'menu.branch.apply-patch-ai': {
      const repo = activeRepo(state);
      if (!repo || repo.busy) break;
      if (id === 'menu.branch.branch-new') {
        if (!repo.hasConflicts) state.openOverlay({ kind: 'newBranch' });
      } else if (id === 'menu.branch.branch-rename') {
        if (repo.branch.length > 0) state.openOverlay({ kind: 'renameBranch', old: repo.branch });
      } else if (id === 'menu.branch.stash') {
        if (repo.stashableCount > 0) state.openOverlay({ kind: 'stash' });
      } else if (id === 'menu.branch.stash-pop') {
        if (repo.refs.stashes.length > 0 && !repo.hasConflicts) {
          void state.runAction(repo.id, { action: 'stashPop', stashRef: null });
        }
      } else if (id === 'menu.branch.merge' || id === 'menu.branch.merge-no-ff') {
        if (hasLocalBranches(repo) && !repo.hasConflicts) {
          state.openOverlay({ kind: 'merge', noFf: id === 'menu.branch.merge-no-ff' });
        }
      } else if (id === 'menu.branch.rebase') {
        if (hasLocalBranches(repo) && !repo.hasConflicts) state.openOverlay({ kind: 'rebase' });
      } else if (!repo.hasConflicts) {
        window.dispatchEvent(
          new CustomEvent('augur:toolbar-patch-action', {
            detail: {
              repoId: repo.id,
              copyPrompt: id === 'menu.branch.apply-patch-ai'
            }
          })
        );
      }
      break;
    }
    case 'menu.about':
      await ipc.openAboutWindow();
      break;
    case 'menu.quit':
      // The webview writes the final snapshot before the process ends, so a
      // setting changed seconds earlier is not lost to the debounce window.
      await ipc.flushState();
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().destroy();
      break;
    default:
      break;
  }
}
