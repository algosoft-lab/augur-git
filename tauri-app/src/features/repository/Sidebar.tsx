/**
 * The repository sidebar: local branches, remote branches grouped by remote,
 * tags, and stashes.
 *
 * Rows are 22px tall with a leading marker — a filled dot for the checked-out
 * ref, a hollow ring otherwise — and the checked-out row is semibold. Context
 * menus are the same on right click and on long press.
 */

import { useState } from "react";

import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { Icon } from "../../components/Icon";
import type { ContextMenuEntry } from "../../components/controls";
import { ContextMenu } from "../../components/controls";
import type { RefsInfo } from "../../bridge/types";
import { useStore, type RepoState } from "../../app/store";
import { groupRemoteBranches } from "./remoteGroups";
import { t, ta } from "../../i18n/strings";

interface SectionProps {
  sectionKey: string;
  title: string;
  count: number;
  collapsed: boolean;
  onToggle: (key: string) => void;
  children: React.ReactNode;
}

/** One collapsible section header with a count. */
function Section({ sectionKey, title, count, collapsed, onToggle, children }: SectionProps) {
  return (
    <div className="sidebar__section" data-testid={`sidebar-${sectionKey}`}>
      <button
        type="button"
        className="sidebar__section-header"
        aria-expanded={!collapsed}
        onClick={() => onToggle(sectionKey)}
        data-testid={`sidebar-toggle-${sectionKey}`}
      >
        <Icon name={collapsed ? "chevron-right" : "chevron-down"} size={12} />
        <span className="sidebar__section-title">{title}</span>
        <span className="sidebar__section-count">{count}</span>
      </button>
      {collapsed ? null : children}
    </div>
  );
}

export function Sidebar({ repo, refs }: { repo: RepoState; refs: RefsInfo }) {
  const translate = useStore((state) => state.t);
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const runAction = useStore((state) => state.runAction);
  const openOverlay = useStore((state) => state.openOverlay);
  const setMessage = useStore((state) => state.setMessage);

  const toggle = (key: string) => {
    setCollapsed((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : [...current, key],
    );
  };

  const isCollapsed = (key: string) => collapsed.includes(key);
  const blocked = repo.hasConflicts || repo.busy;
  const groups = groupRemoteBranches(refs.remotes, refs.remote_branches);

  const copy = (value: string) => {
    void writeText(value).then(() => {
      setMessage(repo.id, ta(translate, "context-copied", { name: value }), true);
    });
  };

  const branchEntries = (name: string, isHead: boolean): ContextMenuEntry[] => [
    {
      id: "checkout",
      label: t(translate, "context-checkout"),
      icon: <Icon name="git-branch" size={12} />,
      disabled: blocked || isHead,
      onSelect: () => {
        void runAction(repo.id, {
          action: "checkout",
          target: { kind: "localBranch", localBranch: name },
        });
      },
    },
    {
      id: "copy-branch",
      label: t(translate, "context-copy-branch"),
      icon: <Icon name="copy" size={12} />,
      onSelect: () => copy(name),
    },
    {
      id: "rename",
      label: t(translate, "context-rename"),
      icon: <Icon name="pencil" size={12} />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: "renameBranch", old: name }),
    },
    {
      id: "delete",
      label: t(translate, "context-delete"),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked || isHead,
      onSelect: () => openOverlay({ kind: "deleteRef", name, isTag: false }),
    },
    {
      id: "merge-into-current",
      label: t(translate, "context-merge-into-current"),
      icon: <Icon name="git-merge" size={12} />,
      disabled: blocked || isHead,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: "merge", noFf: false }),
    },
  ];

  const tagEntries = (name: string): ContextMenuEntry[] => [
    {
      id: "checkout",
      label: t(translate, "context-checkout"),
      icon: <Icon name="git-branch" size={12} />,
      disabled: blocked,
      onSelect: () => {
        void runAction(repo.id, { action: "checkout", target: { kind: "tag", tag: name } });
      },
    },
    {
      id: "copy-tag",
      label: t(translate, "context-copy-tag"),
      icon: <Icon name="copy" size={12} />,
      onSelect: () => copy(name),
    },
    {
      id: "delete",
      label: t(translate, "context-delete"),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: "deleteRef", name, isTag: true }),
    },
  ];

  const remoteEntries = (remote: string, branch: string): ContextMenuEntry[] => [
    {
      id: "checkout",
      label: t(translate, "context-checkout"),
      icon: <Icon name="git-branch" size={12} />,
      disabled: blocked,
      onSelect: () => {
        void runAction(repo.id, {
          action: "checkout",
          target: { kind: "remoteBranch", remoteBranch: `${remote}/${branch}` },
        });
      },
    },
    {
      id: "copy-branch",
      label: t(translate, "context-copy-branch"),
      icon: <Icon name="copy" size={12} />,
      onSelect: () => copy(`${remote}/${branch}`),
    },
    {
      id: "rename",
      label: t(translate, "context-rename"),
      icon: <Icon name="pencil" size={12} />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: "renameRemoteBranch", remote, old: branch }),
    },
    {
      id: "delete",
      label: t(translate, "context-delete"),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked,
      onSelect: () => openOverlay({ kind: "deleteRemoteBranch", remote, branch }),
    },
  ];

  const stashEntries = (reference: string): ContextMenuEntry[] => [
    {
      id: "pop",
      label: t(translate, "menu-stash-pop"),
      icon: <Icon name="archive-restore" size={12} />,
      disabled: blocked,
      onSelect: () => {
        void runAction(repo.id, { action: "stashPop", stashRef: reference });
      },
    },
    {
      id: "drop",
      label: t(translate, "menu-stash-drop"),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked,
      onSelect: () => openOverlay({ kind: "stashDrop", reference }),
    },
  ];

  return (
    <div className="sidebar" data-testid="sidebar">
      <Section
        sectionKey="branches"
        title={t(translate, "section-branches")}
        count={repo.branches.length}
        collapsed={isCollapsed("branches")}
        onToggle={toggle}
      >
        {repo.branches.map((branch) => (
          <ContextMenu
            key={branch.name}
            testId={`ref-${branch.name}`}
            entries={branchEntries(branch.name, branch.is_head)}
          >
            <button
              type="button"
              className={`ref-row${branch.is_head ? " is-head" : ""}`}
              data-testid={`branch-${branch.name}`}
              title={branch.name}
              onDoubleClick={() => {
                if (!blocked && !branch.is_head) {
                  void runAction(repo.id, {
                    action: "checkout",
                    target: { kind: "localBranch", localBranch: branch.name },
                  });
                }
              }}
              onClick={() => {
                setMessage(
                  repo.id,
                  ta(translate, "branch-selected", { name: branch.name }),
                  null,
                );
              }}
            >
              <span className={`ref-marker${branch.is_head ? " ref-marker--head" : ""}`} />
              <span className="ref-row__label">{branch.name}</span>
            </button>
          </ContextMenu>
        ))}
      </Section>

      <Section
        sectionKey="remote-branches"
        title={t(translate, "section-remote-branches")}
        count={refs.remote_branches.length}
        collapsed={isCollapsed("remote-branches")}
        onToggle={toggle}
      >
        {groups.map((group) => (
          <div key={group.remote} className="sidebar__section">
            <button
              type="button"
              className="sidebar__section-header sidebar__group-header"
              onClick={() => toggle(`remote-${group.remote}`)}
              data-testid={`sidebar-toggle-remote-${group.remote}`}
            >
              <Icon
                name={isCollapsed(`remote-${group.remote}`) ? "chevron-right" : "chevron-down"}
                size={12}
              />
              <Icon name="git-branch" size={12} />
              <span className="sidebar__section-title">{group.remote}</span>
              <span className="sidebar__section-count">{group.branches.length}</span>
            </button>
            {isCollapsed(`remote-${group.remote}`)
              ? null
              : group.branches.map((entry) => (
                  <ContextMenu
                    key={entry.fullName}
                    testId={`ref-${entry.fullName}`}
                    entries={remoteEntries(group.remote, entry.label)}
                  >
                    <button
                      type="button"
                      className="ref-row"
                      style={{ paddingLeft: 20 }}
                      data-testid={`remote-branch-${entry.fullName}`}
                      title={entry.fullName}
                      onDoubleClick={() => {
                        if (!blocked) {
                          void runAction(repo.id, {
                            action: "checkout",
                            target: {
                              kind: "remoteBranch",
                              remoteBranch: entry.fullName,
                            },
                          });
                        }
                      }}
                    >
                      <span className="ref-marker" />
                      <span className="ref-row__label">{entry.label}</span>
                    </button>
                  </ContextMenu>
                ))}
          </div>
        ))}
      </Section>

      <Section
        sectionKey="tags"
        title={t(translate, "section-tags")}
        count={refs.tags.length}
        collapsed={isCollapsed("tags")}
        onToggle={toggle}
      >
        {refs.tags.map((name) => (
          <ContextMenu key={name} testId={`tag-${name}`} entries={tagEntries(name)}>
            <button
              type="button"
              className="ref-row"
              data-testid={`tag-row-${name}`}
              title={name}
              onDoubleClick={() => {
                if (!blocked) {
                  void runAction(repo.id, {
                    action: "checkout",
                    target: { kind: "tag", tag: name },
                  });
                }
              }}
            >
              <span className="ref-marker" />
              <span className="ref-row__label">{name}</span>
            </button>
          </ContextMenu>
        ))}
      </Section>

      <Section
        sectionKey="stashes"
        title={t(translate, "section-stashes")}
        count={refs.stashes.length}
        collapsed={isCollapsed("stashes")}
        onToggle={toggle}
      >
        {refs.stashes.map((stash) => (
          <ContextMenu
            key={stash.reference}
            testId={`stash-${stash.reference}`}
            entries={stashEntries(stash.reference)}
          >
            <button
              type="button"
              className="ref-row"
              data-testid={`stash-row-${stash.reference}`}
              title={stash.description}
              onClick={() => {
                void runAction(repo.id, { action: "stashPop", stashRef: stash.reference });
              }}
            >
              <span className="ref-marker" />
              <span className="ref-row__label">{stash.description}</span>
            </button>
          </ContextMenu>
        ))}
      </Section>
    </div>
  );
}
