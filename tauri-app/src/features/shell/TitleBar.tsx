/**
 * The title bar: the in-window menu, tabs, drag region, app name, and controls.
 *
 * macOS renders the traffic lights over the content because the window uses an
 * overlay title bar, so the bar reserves space for them.
 */

import { Icon } from "../../components/Icon";
import * as ipc from "../../bridge/ipc";
import { Menu, type MenuItemSpec } from "../../components/controls";
import { useStore } from "../../app/store";
import { t } from "../../i18n/strings";
import { TabBar } from "./TabBar";
import { IS_MACOS, WindowControls } from "./WindowControls";
import { handleTitleBarMouseDown } from "./titleBarDrag";

export function TitleBar({
  onOpenRepository,
  onOpenWslRepository,
  onNewTab,
  onInstallCli,
  onRemoveCli,
}: {
  onOpenRepository: () => void;
  onOpenWslRepository: () => void;
  onNewTab: () => void;
  onInstallCli: () => void;
  onRemoveCli: () => void;
}) {
  const translate = useStore((state) => state.t);
  const build = useStore((state) => state.build);
  const recent = useStore((state) => state.config.recent_repos);
  const openTab = useStore((state) => state.openTab);
  const setSettingsOpen = useStore((state) => state.setSettingsOpen);

  const recentItems: MenuItemSpec[] = recent.length
    ? recent.map((repo, index) => ({
        id: `recent-${index}`,
        label:
          repo.location.kind === "wsl"
            ? `${repo.location.distro} · ${repo.path}`
            : repo.path,
        icon: <Icon name="git-branch" />,
        onSelect: () => {
          void openTab(repo.path, repo.location);
        },
      }))
    : [{ id: "recent-empty", label: t(translate, "menu-no-recent-repositories"), disabled: true }];

  const fileItems: MenuItemSpec[] = [
    {
      id: "open-repository",
      label: t(translate, "menu-open-repository"),
      icon: <Icon name="file" />,
      onSelect: onOpenRepository,
    },
    ...(typeof navigator !== "undefined" && /Win/i.test(navigator.platform)
      ? [
          {
            id: "open-wsl-repository",
            label: t(translate, "menu-open-wsl-repository"),
            icon: <Icon name="file" />,
            onSelect: onOpenWslRepository,
          } satisfies MenuItemSpec,
        ]
      : []),
    {
      id: "new-tab",
      label: t(translate, "menu-new-tab"),
      icon: <Icon name="plus" />,
      onSelect: onNewTab,
    },
    { id: "sep-1", label: "", separatorBefore: true, disabled: true },
    {
      id: "install-cli",
      label: t(translate, "menu-install-cli"),
      icon: <Icon name="upload" />,
      onSelect: onInstallCli,
    },
    {
      id: "remove-cli",
      label: t(translate, "menu-remove-cli"),
      icon: <Icon name="trash-2" />,
      onSelect: onRemoveCli,
    },
    {
      id: "recent-repositories",
      label: t(translate, "menu-recent-repositories"),
      icon: <Icon name="git-branch" />,
      separatorBefore: true,
      children: recentItems,
    },
  ];

  const editItems: MenuItemSpec[] = [
    {
      id: "settings",
      label: t(translate, "menu-settings"),
      icon: <Icon name="settings" />,
      onSelect: () => setSettingsOpen(true),
    },
  ];

  const helpItems: MenuItemSpec[] = [
    {
      id: "about",
      label: t(translate, "menu-about"),
      icon: <Icon name="file" />,
      onSelect: () => {
        void ipc.openAboutWindow();
      },
    },
  ];

  const menuItems: MenuItemSpec[] = [
    { id: "file", label: t(translate, "menu-file"), children: fileItems },
    { id: "edit", label: t(translate, "menu-edit"), children: editItems },
    { id: "help", label: t(translate, "menu-help"), children: helpItems },
    {
      id: "quit",
      label: t(translate, "menu-quit"),
      danger: true,
      separatorBefore: true,
      onSelect: async () => {
        await ipc.flushState();
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().close();
      },
    },
  ];

  return (
    <div
      className={`title-bar${IS_MACOS ? " title-bar--macos" : ""}`}
      data-testid="title-bar"
      onMouseDown={handleTitleBarMouseDown}
    >
      {!IS_MACOS ? (
        <Menu items={menuItems} testId="menu-file">
          <button
            type="button"
            className="tool-button tool-button--compact title-bar__menu-trigger"
            title={t(translate, "menu-open")}
            aria-label={t(translate, "menu-open")}
          >
            <Icon name="menu" size={15} />
          </button>
        </Menu>
      ) : null}
      <TabBar onNewTab={onNewTab} />
      <div
        className="title-bar__drag"
        {...(IS_MACOS ? { "data-tauri-drag-region": true } : {})}
      />
      {build?.name ? <span className="title-bar__brand">{build.name}</span> : null}
      <WindowControls flushBeforeClose />
    </div>
  );
}
