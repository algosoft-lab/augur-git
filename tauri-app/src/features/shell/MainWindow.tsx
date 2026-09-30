/**
 * The main window: title bar, tab bar, repository content, status bar, and the
 * overlay dialogs.
 *
 * The native menu and the File-menu items in the title bar dispatch DOM events
 * so both surfaces run exactly the same handler.
 */

import { useEffect, useRef, useState } from 'react';

import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';

import { useStore } from '../../app/store';
import * as ipc from '../../bridge/ipc';
import { TitleBar } from './TitleBar';
import { StatusBar } from './StatusBar';
import { Welcome } from './Welcome';
import { RepoTab } from '../repository/RepoTab';
import { Overlays } from '../dialogs/Overlays';
import { t } from '../../i18n/strings';
import { SidecarWindow } from './SidecarWindow';
import { UpdateNoticeCard } from './UpdateNoticeCard';

export function MainWindow() {
  const translate = useStore((state) => state.t);
  const tabs = useStore((state) => state.tabs);
  const activeTabKey = useStore((state) => state.activeTabKey);
  const repos = useStore((state) => state.repos);
  const notice = useStore((state) => state.notice);
  const openPaths = useStore((state) => state.openPaths);
  const windowMode = useStore((state) => state.workspace.window_mode);
  const setWindowMode = useStore((state) => state.setWindowMode);
  const addStartTab = useStore((state) => state.addStartTab);
  const notify = useStore((state) => state.notify);
  const [wslOpen, setWslOpen] = useState(false);
  const targetGeneration = useRef(0);
  const toggleWindowModeRef = useRef<() => void>(() => undefined);

  const pickFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t(translate, 'repo-folder-prompt')
    });
    if (typeof selected === 'string') {
      await openPaths([selected]);
    }
  };

  // The native menu dispatches DOM events so both surfaces share one handler.
  useEffect(() => {
    const onOpen = () => void pickFolder();
    const onWsl = () => setWslOpen(true);
    // A new tab is a start page, not a folder dialog: it shows the recent
    // repositories and a repository opened into it takes the tab's slot.
    const onNewTab = () => addStartTab();
    const onToggleMode = () => toggleWindowModeRef.current();
    window.addEventListener('augur:open-repository', onOpen);
    window.addEventListener('augur:open-wsl-repository', onWsl);
    window.addEventListener('augur:new-tab', onNewTab);
    window.addEventListener('augur:toggle-window-mode', onToggleMode);
    return () => {
      window.removeEventListener('augur:open-repository', onOpen);
      window.removeEventListener('augur:open-wsl-repository', onWsl);
      window.removeEventListener('augur:new-tab', onNewTab);
      window.removeEventListener('augur:toggle-window-mode', onToggleMode);
    };
  }, []);

  const activeTab = tabs.find((tab) => tab.key === activeTabKey) ?? null;
  const activeRepo =
    activeTab && activeTab.repoId !== null ? (repos[activeTab.repoId] ?? null) : null;

  useEffect(() => {
    const generation = ++targetGeneration.current;
    void ipc.setAutoRefreshTarget(activeRepo?.id ?? null, generation).catch((error) => {
      console.warn('[auto_refresh] failed to select active repository', error);
    });
    return () => {
      const releaseGeneration = ++targetGeneration.current;
      void ipc.setAutoRefreshTarget(null, releaseGeneration).catch((error) => {
        console.warn('[auto_refresh] failed to release active repository', error);
      });
    };
  }, [activeRepo?.id]);

  useEffect(() => {
    let cancelled = false;
    let closing = false;
    let timer: number | null = null;
    const unlisten: (() => void)[] = [];
    const persistBounds = () => {
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = null;
        void ipc.saveWindowBounds().catch((error) => {
          console.warn('[window_state] failed to save main window bounds', error);
        });
      }, 450);
    };
    void (async () => {
      const mainWindow = getCurrentWindow();
      const resized = await mainWindow.onResized(persistBounds);
      if (cancelled) resized();
      else unlisten.push(resized);
      const moved = await mainWindow.onMoved(persistBounds);
      if (cancelled) moved();
      else unlisten.push(moved);
      const closeRequested = await mainWindow.onCloseRequested((event) => {
        if (closing) {
          return;
        }
        event.preventDefault();
        closing = true;
        if (timer !== null) {
          window.clearTimeout(timer);
          timer = null;
        }
        void (async () => {
          try {
            await ipc.saveWindowBounds();
            await ipc.flushState();
          } catch (error) {
            console.warn('[window_state] failed to flush the main window before close', error);
          }
          // destroy, not close: this request is already prevented, so close()
          // would only loop back into this handler. destroy() skips the
          // request entirely and needs the core:window:allow-destroy
          // capability granted to this window.
          try {
            await mainWindow.destroy();
          } catch (error) {
            console.error('[window_state] failed to destroy the main window', error);
          }
        })();
      });
      if (cancelled) closeRequested();
      else unlisten.push(closeRequested);
    })();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
      unlisten.forEach((release) => release());
    };
    // Nothing in this effect reads the window mode: the bounds command picks
    // the mode server-side. Registering once avoids a re-registration gap in
    // which a close request would bypass the flush entirely.
  }, []);

  const toggleWindowMode = async () => {
    if (windowMode === 'sidecar') {
      await setWindowMode('desktop');
      return;
    }
    if (activeRepo) {
      const state = useStore.getState();
      const origin = activeRepo.pane.kind === 'working' ? 'changes' : 'history';
      state.patchSidecarUi(activeRepo.id, {
        page: activeRepo.pane.kind === 'none' ? 'changes' : 'diff',
        diffReturnPage: origin
      });
    }
    await setWindowMode('sidecar');
  };
  toggleWindowModeRef.current = () => void toggleWindowMode();

  return (
    <div className="app">
      <TitleBar
        onOpenRepository={() => void pickFolder()}
        onOpenWslRepository={() => setWslOpen(true)}
        onNewTab={addStartTab}
        sidecar={windowMode === 'sidecar'}
        hasActiveRepo={activeRepo !== null}
        windowMode={windowMode}
        onToggleMode={() => void toggleWindowMode()}
      />
      {activeRepo ? (
        windowMode === 'sidecar' ? (
          <SidecarWindow repo={activeRepo} />
        ) : (
          <RepoTab repo={activeRepo} />
        )
      ) : (
        // A start page and a window with no tabs show the same page; the
        // difference is only that a start page holds a slot for a repository.
        <div className="app__page" data-testid={activeTab ? 'start-page' : 'window-welcome'}>
          <Welcome
            onOpenRepository={() => void pickFolder()}
            onOpenWslRepository={() => setWslOpen(true)}
          />
        </div>
      )}
      {windowMode === 'sidecar' ? null : <StatusBar repo={activeRepo} />}
      <Overlays wslOpen={wslOpen} onWslOpenChange={setWslOpen} onOpenPaths={openPaths} />
      <UpdateNoticeCard />
      {notice ? (
        <div className={`notice notice--${notice.level}`} data-testid="notice">
          <span>{notice.message}</span>
          <button
            type="button"
            className="notice__close"
            aria-label={t(translate, 'notice-dismiss')}
            onClick={() => notify(null)}
          >
            ×
          </button>
        </div>
      ) : null}
    </div>
  );
}
