/**
 * The repository tab bar.
 *
 * A tab's state is derived from its repository: loading while the first status
 * snapshot has not arrived, an error when the repository could not be opened,
 * ready otherwise. The indicator is a 6px dot so the tab label stays readable.
 */

import { Icon } from "../../components/Icon";
import { useStore } from "../../app/store";
import { t } from "../../i18n/strings";

export function TabBar({ onNewTab }: { onNewTab: () => void }) {
  const translate = useStore((state) => state.t);
  const tabs = useStore((state) => state.tabs);
  const activeTabKey = useStore((state) => state.activeTabKey);
  const repos = useStore((state) => state.repos);
  const selectTab = useStore((state) => state.selectTab);
  const closeTab = useStore((state) => state.closeTab);

  return (
    <div className="tab-bar" role="tablist" data-testid="tab-bar">
      {tabs.map((tab) => {
        const repo = tab.repoId === null ? undefined : repos[tab.repoId];
        const state = repo?.status ?? "loading";
        // A start page has no repository, so it is named for what it offers.
        const label =
          tab.repoId === null
            ? t(translate, "tab-new")
            : tab.location.kind === "wsl"
              ? `${tab.location.distro} · ${basename(tab.path)}`
              : basename(tab.path);
        return (
          <div
            key={tab.key}
            role="tab"
            aria-selected={tab.key === activeTabKey}
            className={`tab${tab.key === activeTabKey ? " is-active" : ""}`}
            data-testid={`tab-${tab.key}`}
            onClick={() => {
              void selectTab(tab.key);
            }}
            onAuxClick={(event) => {
              // Middle click closes, matching the reference application.
              if (event.button === 1) {
                event.preventDefault();
                void closeTab(tab.key);
              }
            }}
            title={tab.repoId === null ? t(translate, "tab-new") : tab.path}
          >
            <span className={`tab__dot tab__dot--${state}`} />
            <span className="tab__label">{label}</span>
            <button
              type="button"
              className="tab__close"
              title={t(translate, "tab-close")}
              aria-label={t(translate, "tab-close")}
              data-testid={`tab-close-${tab.key}`}
              onClick={(event) => {
                event.stopPropagation();
                void closeTab(tab.key);
              }}
            >
              <Icon name="x" size={11} />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        className="tab-bar__new"
        title={t(translate, "tab-new")}
        aria-label={t(translate, "tab-new")}
        data-testid="tab-new"
        onClick={onNewTab}
      >
        <Icon name="plus" size={13} />
      </button>
    </div>
  );
}

/** Last path segment, with a fallback for a filesystem root. */
function basename(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
