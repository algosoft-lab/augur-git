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
import { useStore, type WindowRole } from './store';
import { DEFAULT_THEME, DEFAULT_TYPOGRAPHY } from '../styles/themes';

interface WindowTarget {
  role: WindowRole;
  compareRepoId: number | null;
}

function readTarget(): WindowTarget {
  const params = new URLSearchParams(window.location.search);
  const requested = params.get('window');
  if (requested === 'compare') {
    const repo = Number(params.get('repo'));
    return {
      role: 'compare',
      compareRepoId: Number.isFinite(repo) && repo > 0 ? repo : null
    };
  }
  if (requested === 'about') {
    return { role: 'about', compareRepoId: null };
  }
  if (requested === 'settings') {
    return { role: 'settings', compareRepoId: null };
  }
  return { role: 'main', compareRepoId: null };
}

export function App() {
  const target = useRef(readTarget()).current;
  const ready = useStore((state) => state.ready);
  const theme = useStore((state) => state.config.theme);
  const typography = useStore((state) => state.config.typography);
  const [fatal, setFatal] = useState<string | null>(null);

  // The document is styled before anything is fetched, so a failure during
  // start-up is a readable message rather than an unstyled white page.
  useEffect(() => {
    applyTheme(document.documentElement, DEFAULT_THEME, DEFAULT_TYPOGRAPHY);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: (() => void)[] = [];

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
          ipc.onMenuEvent((id) => {
            void handleMenuAction(id);
          })
        );
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
    surface = <SettingsWindow />;
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
