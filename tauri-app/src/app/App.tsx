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
import { DEFAULT_THEME, DEFAULT_TYPOGRAPHY } from "../styles/themes";

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
        // The log plugin does not capture the webview console, so a start-up
        // failure is reported through the interface as well.
        console.error("[boot] start-up failed", error);
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

  if (target.role === "about") {
    return <AboutWindow />;
  }
  if (target.role === "compare") {
    return <CompareWindow repoId={target.compareRepoId} />;
  }
  return <MainWindow />;
}

/** Turn any thrown value into something worth reading on screen. */
function describeFailure(error: unknown): string {
  if (error && typeof error === "object") {
    const record = error as Record<string, unknown>;
    const parts = Object.entries(record)
      .filter(([, value]) => typeof value === "string" || typeof value === "number")
      .map(([key, value]) => `${key}: ${value}`);
    if (parts.length) {
      return parts.join("\n");
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

/** Two seconds, the same interval the reference uses. */
const FOCUS_REFRESH_COOLDOWN_MS = 2000;

/**
 * Whether a focus at `now` should refresh, given the last time one did.
 *
 * A cooldown, because the activation delivered right after the window is created
 * would otherwise refresh while the initial load is still in flight, and
 * because alt-tabbing back and forth should not spawn a repository read per
 * switch.
 */
export function shouldRefreshOnFocus(
  last: number | null,
  now: number,
  cooldown = FOCUS_REFRESH_COOLDOWN_MS,
): boolean {
  return last === null || now - last >= cooldown;
}

/** The last focus-triggered refresh, in milliseconds. */
let lastFocusRefresh: number | null = null;

/** Refresh the active repository when the window regains focus. */
function onWindowFocus(): void {
  const state = useStore.getState();
  if (!state.config.view.auto_refresh_on_focus) {
    return;
  }
  if (state.role !== "main" || !state.activeTabKey) {
    return;
  }
  const now = Date.now();
  if (!shouldRefreshOnFocus(lastFocusRefresh, now)) {
    return;
  }
  lastFocusRefresh = now;
  const tab = state.tabs.find((entry) => entry.key === state.activeTabKey);
  if (tab && tab.repoId !== null) {
    void state.refresh(tab.repoId);
  }
}
