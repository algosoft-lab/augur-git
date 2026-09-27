/**
 * The title bar: the in-window application menu, the window drag region, and
 * the quick actions.
 *
 * macOS renders the traffic lights over the content because the window uses an
 * overlay title bar, so the bar reserves space for them.
 */

import { Icon } from "../../components/Icon";
import * as ipc from "../../bridge/ipc";
import { Menu, type MenuItemSpec } from "../../components/controls";
import { useStore } from "../../app/store";
import { t } from "../../i18n/strings";

const IS_MACOS =
  typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);

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
    ? recent.map((repo) => ({
        id: `recent-${repo.path}`,
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
      id: "recent",
      label: t(translate, "menu-recent-repositories"),
      icon: <Icon name="git-branch" />,
      separatorBefore: true,
      disabled: true,
    },
    ...recentItems.map((item) => ({ ...item, separatorBefore: false })),
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

  return (
    <div
      className={`title-bar${IS_MACOS ? " title-bar--macos" : ""}`}
      data-tauri-drag-region
      data-testid="title-bar"
    >
      <Menu items={fileItems} testId="menu-file">
        <button
          type="button"
          className="tool-button tool-button--compact"
          title={t(translate, "menu-open")}
          aria-label={t(translate, "menu-open")}
        >
          <Icon name="menu" size={15} />
        </button>
      </Menu>
      <Menu items={editItems} testId="menu-edit">
        <button
          type="button"
          className="tool-button tool-button--compact"
          title={t(translate, "menu-edit")}
        >
          {t(translate, "menu-edit")}
        </button>
      </Menu>
      <Menu items={helpItems} testId="menu-help">
        <button
          type="button"
          className="tool-button tool-button--compact"
          title={t(translate, "menu-help")}
        >
          {t(translate, "menu-help")}
        </button>
      </Menu>
      <div className="title-bar__drag" data-tauri-drag-region />
      <div className="title-bar__brand">
        <img src="/logo.svg" alt="" />
        <span>{build?.name ?? "Augur Git Tauri"}</span>
      </div>
    </div>
  );
}
