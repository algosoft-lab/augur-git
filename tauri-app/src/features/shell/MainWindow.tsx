/**
 * The main window: title bar, tab bar, repository content, status bar, and the
 * overlay dialogs.
 *
 * The native menu and the File-menu items in the title bar dispatch DOM events
 * so both surfaces run exactly the same handler.
 */

import { useEffect, useState } from "react";

import { open } from "@tauri-apps/plugin-dialog";

import * as ipc from "../../bridge/ipc";
import type { ChangeReport } from "../../bridge/types";
import { useStore } from "../../app/store";
import { TabBar } from "./TabBar";
import { TitleBar } from "./TitleBar";
import { StatusBar } from "./StatusBar";
import { Welcome } from "./Welcome";
import { RepoTab } from "../repository/RepoTab";
import { Overlays } from "../dialogs/Overlays";
import { SettingsWindow } from "../settings/SettingsWindow";
import { t } from "../../i18n/strings";

export function MainWindow() {
  const translate = useStore((state) => state.t);
  const tabs = useStore((state) => state.tabs);
  const activeTabKey = useStore((state) => state.activeTabKey);
  const repos = useStore((state) => state.repos);
  const settingsOpen = useStore((state) => state.settingsOpen);
  const notice = useStore((state) => state.notice);
  const openPaths = useStore((state) => state.openPaths);
  const openOverlay = useStore((state) => state.openOverlay);
  const notify = useStore((state) => state.notify);
  const [wslOpen, setWslOpen] = useState(false);

  const pickFolder = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      await openPaths([selected]);
    }
  };

  const installCli = async (operation: "install" | "remove") => {
    try {
      const report: ChangeReport = await ipc.runCliInstaller(operation);
      openOverlay({ kind: "cliReport", report });
    } catch (error) {
      const failure = ipc.describeError(error);
      notify({ level: "error", message: `${failure.key}: ${failure.detail}` });
    }
  };

  // The native menu dispatches DOM events so both surfaces share one handler.
  useEffect(() => {
    const onOpen = () => void pickFolder();
    const onWsl = () => setWslOpen(true);
    const onNewTab = () => void pickFolder();
    const onInstall = () => void installCli("install");
    const onRemove = () => void installCli("remove");
    window.addEventListener("augur:open-repository", onOpen);
    window.addEventListener("augur:open-wsl-repository", onWsl);
    window.addEventListener("augur:new-tab", onNewTab);
    window.addEventListener("augur:install-cli", onInstall);
    window.addEventListener("augur:remove-cli", onRemove);
    return () => {
      window.removeEventListener("augur:open-repository", onOpen);
      window.removeEventListener("augur:open-wsl-repository", onWsl);
      window.removeEventListener("augur:new-tab", onNewTab);
      window.removeEventListener("augur:install-cli", onInstall);
      window.removeEventListener("augur:remove-cli", onRemove);
    };
  }, []);

  const activeTab = tabs.find((tab) => tab.key === activeTabKey) ?? null;
  const activeRepo = activeTab ? (repos[activeTab.repoId] ?? null) : null;

  return (
    <div className="app">
      <TitleBar
        onOpenRepository={() => void pickFolder()}
        onOpenWslRepository={() => setWslOpen(true)}
        onNewTab={() => void pickFolder()}
        onInstallCli={() => void installCli("install")}
        onRemoveCli={() => void installCli("remove")}
      />
      <TabBar onNewTab={() => void pickFolder()} />
      {activeRepo ? (
        <RepoTab repo={activeRepo} />
      ) : (
        <Welcome
          onOpenRepository={() => void pickFolder()}
          onOpenWslRepository={() => setWslOpen(true)}
        />
      )}
      <StatusBar repo={activeRepo} />
      <Overlays
        wslOpen={wslOpen}
        onWslOpenChange={setWslOpen}
        onOpenPaths={openPaths}
      />
      {settingsOpen ? <SettingsWindow /> : null}
      {notice ? (
        <div className={`notice notice--${notice.level}`} data-testid="notice">
          <span>{notice.message}</span>
          <button
            type="button"
            className="notice__close"
            aria-label={t(translate, "notice-dismiss")}
            onClick={() => notify(null)}
          >
            ×
          </button>
        </div>
      ) : null}
    </div>
  );
}
