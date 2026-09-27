/**
 * Window router and event bootstrap.
 *
 * One bundle serves every window. The role in the URL decides which surface is
 * mounted; the event subscriptions are shared because a settings change or a
 * forwarded CLI path has to reach all of them.
 */

import { useEffect, useRef, useState } from "react";

import * as ipc from "../bridge/ipc";
import { AboutWindow } from "../features/about/AboutWindow";
import { CompareWindow } from "../features/compare/CompareWindow";
import { MainWindow } from "../features/shell/MainWindow";
import { applyTheme } from "../styles/themes";
import { useStore, type WindowRole } from "./store";

interface WindowTarget {
  role: WindowRole;
  compareRepoId: number | null;
}

function readTarget(): WindowTarget {
  const params = new URLSearchParams(window.location.search);
  const requested = params.get("window");
  if (requested === "compare") {
    const repo = Number(params.get("repo"));
    return {
      role: "compare",
      compareRepoId: Number.isFinite(repo) && repo > 0 ? repo : null,
    };
  }
  if (requested === "about") {
    return { role: "about", compareRepoId: null };
  }
  return { role: "main", compareRepoId: null };
}

export function App() {
  const target = useRef(readTarget()).current;
  const ready = useStore((state) => state.ready);
  const theme = useStore((state) => state.config.theme);
  const typography = useStore((state) => state.config.typography);
  const [fatal, setFatal] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: (() => void)[] = [];

    const boot = async () => {
      try {
        const store = useStore.getState();
        // Subscriptions first: an event that arrives while the interface is
        // still loading must not be dropped on the floor.
        unlisteners.push(
          await ipc.onRepoEvent((event) => {
            useStore.getState().applyEvent(event.repoId, event);
          }),
        );
        unlisteners.push(
          await ipc.onAppEvent((event) => {
            if (event.type === "settingsChanged") {
              void refreshConfig();
            } else if (event.type === "workspaceChanged") {
              // The main window owns the tab list; other windows ignore it.
            } else if (event.type === "notice") {
              useStore.getState().notify({
                level: event.level as "info" | "warning" | "error",
                message: event.message,
              });
            }
          }),
        );
        unlisteners.push(
          await ipc.onOpenPaths((paths) => {
            void useStore.getState().openPaths(paths);
          }),
        );
        unlisteners.push(
          await ipc.onDropPaths((paths) => {
            void useStore.getState().openPaths(paths);
          }),
        );
        unlisteners.push(
          await ipc.onMenuEvent((id) => {
            void handleMenuAction(id);
          }),
        );
        unlisteners.push(
          await ipc.onWindowFocus(() => {
            onWindowFocus();
          }),
        );

        await store.initialize(target.role, target.compareRepoId);
        if (!cancelled) {
          applyThemeFromState();
        }
      } catch (error) {
        if (!cancelled) {
          setFatal(error instanceof Error ? error.message : String(error));
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
        <div className="empty-state">
          <span className="empty-state__message">{fatal}</span>
        </div>
      </div>
    );
  }

  if (!ready) {
    return <div className="app" />;
  }

  if (target.role === "about") {
    return <AboutWindow />;
  }
  if (target.role === "compare") {
    return <CompareWindow repoId={target.compareRepoId} />;
  }
  return <MainWindow />;
}

/** Write the active theme and typography onto the document root. */
export function applyThemeFromState(): void {
  const { config } = useStore.getState();
  applyTheme(document.documentElement, config.theme, {
    uiFontFamily: config.typography.ui_font_family,
    monoFontFamily: config.typography.mono_font_family,
    uiFontSize: config.typography.ui_font_size,
    diffFontSize: config.typography.diff_font_size,
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
  // The in-window menu dispatches the same DOM events, so both surfaces run one
  // handler.
  switch (id) {
    case "menu.open-repository":
      globalThis.dispatchEvent(new CustomEvent("augur:open-repository"));
      break;
    case "menu.open-wsl-repository":
      globalThis.dispatchEvent(new CustomEvent("augur:open-wsl-repository"));
      break;
    case "menu.new-tab":
      globalThis.dispatchEvent(new CustomEvent("augur:new-tab"));
      break;
    case "menu.install-cli":
      globalThis.dispatchEvent(new CustomEvent("augur:install-cli"));
      break;
    case "menu.remove-cli":
      globalThis.dispatchEvent(new CustomEvent("augur:remove-cli"));
      break;
    case "menu.settings":
      state.setSettingsOpen(true);
      break;
    case "menu.about":
      await ipc.openAboutWindow();
      break;
    case "menu.quit":
      // The webview writes the final snapshot before the process ends, so a
      // setting changed seconds earlier is not lost to the debounce window.
      await ipc.flushState();
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().destroy();
      break;
    default:
      break;
  }
}

/** Refresh the active repository when the window regains focus. */
function onWindowFocus(): void {
  const state = useStore.getState();
  if (!state.config.view.auto_refresh_on_focus) {
    return;
  }
  if (state.role !== "main" || !state.activeTabKey) {
    return;
  }
  const tab = state.tabs.find((entry) => entry.key === state.activeTabKey);
  if (tab) {
    void state.refresh(tab.repoId);
  }
}
