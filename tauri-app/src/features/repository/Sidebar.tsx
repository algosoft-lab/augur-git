/**
 * The repository sidebar: local branches, remote branches grouped by remote,
 * tags, and stashes.
 *
 * Rows are 22px tall with a leading marker — a filled dot for the checked-out
 * ref, a hollow ring otherwise — and the checked-out row is semibold. Context
 * menus are the same on right click and on long press.
 */

import { useLayoutEffect, useRef, useState } from 'react';

import { writeText } from '@tauri-apps/plugin-clipboard-manager';

import { Icon } from '../../components/Icon';
import type { ContextMenuEntry } from '../../components/controls';
import { ContextMenu, IconButton } from '../../components/controls';
import type { RefsInfo } from '../../bridge/types';
import { useStore, type RepoState } from '../../app/store';
import { hasOpenPopup, keysForCommand, matchesShortcut, moveListFocus } from '../../app/keyboard';
import { groupRemoteBranches, OTHER_REMOTE_GROUP } from './remoteGroups';
import {
  planPushCurrent,
  remoteBranchItems,
  remoteGroupItems,
  type RemoteMenuCommand,
  type RemoteMenuItem,
  type RemoteMenuOptions
} from './remoteMenu';
import { preflightPullRebase } from './Toolbar';
import { t, ta } from '../../i18n/strings';

interface SectionProps {
  sectionKey: string;
  title: string;
  count: number;
  collapsed: boolean;
  onToggle: (key: string) => void;
  /** A trailing control that belongs to the section rather than to one row. */
  action?: React.ReactNode;
  children: React.ReactNode;
}

/**
 * One collapsible section header with a count.
 *
 * The header is a button, so an action control has to sit beside it rather
 * than inside it; nesting one button in another is invalid and would make the
 * click target ambiguous.
 */
function Section({
  sectionKey,
  title,
  count,
  collapsed,
  onToggle,
  action,
  children
}: SectionProps) {
  return (
    <div className="sidebar__section" data-testid={`sidebar-${sectionKey}`}>
      <div className="sidebar__section-header-row">
        <button
          type="button"
          className="sidebar__section-header"
          aria-expanded={!collapsed}
          onClick={() => onToggle(sectionKey)}
          data-testid={`sidebar-toggle-${sectionKey}`}
        >
          <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={12} />
          <span className="sidebar__section-title">{title}</span>
          <span className="sidebar__section-count">{count}</span>
        </button>
        {action}
      </div>
      {collapsed ? null : children}
    </div>
  );
}

export function Sidebar({
  repo,
  refs,
  compact = false
}: {
  repo: RepoState;
  refs: RefsInfo;
  compact?: boolean;
}) {
  const translate = useStore((state) => state.t);
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const runAction = useStore((state) => state.runAction);
  const openOverlay = useStore((state) => state.openOverlay);
  const setMessage = useStore((state) => state.setMessage);
  const shortcuts = useStore((state) => state.shortcuts);
  const pullAction = useStore((state) => state.config.view.pull_action);
  const ui = useStore((state) => state.sidecarUi[repo.id]);
  const patchSidecarUi = useStore((state) => state.patchSidecarUi);
  const scrollRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (compact && scrollRef.current) {
      scrollRef.current.scrollTop = ui?.branchesScrollTop ?? 0;
    }
  }, [compact, repo.id]);

  const toggle = (key: string) => {
    setCollapsed((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : [...current, key]
    );
  };

  const isCollapsed = (key: string) => collapsed.includes(key);
  const blocked = repo.hasConflicts || repo.busy || repo.status !== 'ready' || repo.statusStale;
  // Mirroring tags needs at least one remote to mirror, and a second network
  // command while one is running would interleave two ref updates.
  const canSyncTags = refs.remotes.length > 0 && !repo.busy;
  // Remote tag operations need a remote to target, chosen in the dialog.
  const hasRemotes = refs.remotes.length > 0;
  const groups = groupRemoteBranches(refs.remotes, refs.remote_branches);
  // A network command may run while the tree is in conflict, the way the
  // toolbar's fetch and push do; only a busy repository or a stale snapshot
  // stops one.
  const networkAvailable = !repo.busy && repo.status === 'ready' && !repo.statusStale;
  // A detached HEAD has no branch to name, so the operations that publish the
  // current branch are refused rather than sent to Git.
  const currentBranch = repo.branch.startsWith('HEAD') ? '' : repo.branch;
  const menuOptions: RemoteMenuOptions = {
    networkAvailable,
    pullAvailable: !blocked,
    branch: currentBranch,
    upstream: repo.upstream,
    remotes: refs.remotes,
    hasHead: repo.head !== null,
    rebase: pullAction === 'rebase'
  };

  const copy = (value: string) => {
    void writeText(value).then(() => {
      setMessage(repo.id, ta(translate, 'context-copied', { name: value }), true);
    });
  };

  /**
   * Run one of the remote operations.
   *
   * The command names the target, and the target decides what runs: a push
   * that would publish a branch asks first, and a pull that rebases is
   * checked before it starts.
   */
  const runRemoteCommand = (command: RemoteMenuCommand) => {
    switch (command.kind) {
      case 'action':
        void runAction(repo.id, command.action);
        break;
      case 'pull':
        if (command.rebase) {
          void preflightPullRebase(repo, { remote: command.remote, branch: command.branch });
        } else {
          void runAction(repo.id, {
            action: 'pullFrom',
            remote: command.remote,
            branch: command.branch,
            rebase: false
          });
        }
        break;
      case 'pushCurrent': {
        const plan = planPushCurrent(command.remote, menuOptions);
        if (!plan) {
          return;
        }
        if (plan.kind === 'push') {
          void runAction(repo.id, {
            action: 'pushTo',
            remote: plan.remote,
            branch: plan.branch,
            force: false
          });
        } else {
          // The branch does not exist on that remote yet, so publishing it is
          // confirmed first.
          openOverlay({ kind: 'pushSetUpstream', branch: plan.branch, remote: plan.remote });
        }
        break;
      }
      case 'editRemote':
        openOverlay({ kind: 'manageRemotes', focus: { remote: command.remote, panel: 'edit' } });
        break;
      case 'removeRemote':
        openOverlay({ kind: 'manageRemotes', focus: { remote: command.remote, panel: 'remove' } });
        break;
      case 'manageRemotes':
        openOverlay({ kind: 'manageRemotes' });
        break;
    }
  };

  const remoteMenuEntries = (items: RemoteMenuItem[]): ContextMenuEntry[] =>
    items.map((item) => ({
      id: item.id,
      label: ta(translate, item.labelKey, item.labelArgs),
      icon: <Icon name={item.icon} size={12} />,
      disabled: item.disabled,
      separatorBefore: item.separatorBefore,
      onSelect: () => runRemoteCommand(item.command)
    }));

  const branchEntries = (name: string, isHead: boolean): ContextMenuEntry[] => [
    {
      id: 'checkout',
      label: t(translate, 'context-checkout'),
      icon: <Icon name="git-branch" size={12} />,
      disabled: blocked || isHead,
      onSelect: () => {
        void runAction(repo.id, {
          action: 'checkout',
          target: { kind: 'localBranch', localBranch: name }
        });
      }
    },
    {
      id: 'copy-branch',
      label: t(translate, 'context-copy-branch'),
      icon: <Icon name="copy" size={12} />,
      onSelect: () => copy(name)
    },
    {
      id: 'rename',
      label: t(translate, 'context-rename'),
      icon: <Icon name="pencil" size={12} />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: 'renameBranch', old: name })
    },
    {
      id: 'delete',
      label: t(translate, 'context-delete'),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked || isHead,
      onSelect: () => openOverlay({ kind: 'deleteRef', name, isTag: false })
    },
    // Merging a branch into the current one is immediate here: the branch is
    // already named, so there is nothing to ask about. The dialog the toolbar
    // opens is for choosing the source.
    {
      id: 'merge-into-current',
      label: t(translate, 'context-merge-into-current'),
      icon: <Icon name="git-merge" size={12} />,
      disabled: blocked || isHead,
      separatorBefore: true,
      onSelect: () => {
        void runAction(repo.id, { action: 'merge', source: name, noFf: false });
      }
    },
    {
      id: 'merge-no-ff-into-current',
      label: t(translate, 'context-merge-no-ff-into-current'),
      icon: <Icon name="git-merge" size={12} />,
      disabled: blocked || isHead,
      onSelect: () => {
        void runAction(repo.id, { action: 'merge', source: name, noFf: true });
      }
    }
  ];

  const tagEntries = (name: string): ContextMenuEntry[] => [
    {
      id: 'checkout',
      label: t(translate, 'context-checkout'),
      icon: <Icon name="git-branch" size={12} />,
      disabled: blocked,
      onSelect: () => {
        void runAction(repo.id, { action: 'checkout', target: { kind: 'tag', tag: name } });
      }
    },
    {
      id: 'copy-tag',
      label: t(translate, 'context-copy-tag'),
      icon: <Icon name="copy" size={12} />,
      onSelect: () => copy(name)
    },
    {
      id: 'delete',
      label: t(translate, 'context-delete'),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: 'deleteRef', name, isTag: true })
    },
    {
      id: 'delete-remote',
      label: t(translate, 'context-delete-remote-tag'),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked || !hasRemotes,
      onSelect: () => openOverlay({ kind: 'deleteRemoteTag', name })
    },
    {
      id: 'rename-remote',
      label: t(translate, 'context-rename-remote-tag'),
      icon: <Icon name="pencil" size={12} />,
      disabled: blocked || !hasRemotes,
      onSelect: () => openOverlay({ kind: 'renameRemoteTag', old: name })
    }
  ];

  const remoteEntries = (remote: string, branch: string): ContextMenuEntry[] => [
    // The network operations come first: they are what the row is for, and
    // the ref operations follow behind a separator.
    ...remoteMenuEntries(remoteBranchItems(remote, branch, menuOptions)),
    {
      id: 'checkout',
      label: t(translate, 'context-checkout'),
      icon: <Icon name="git-branch" size={12} />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => {
        void runAction(repo.id, {
          action: 'checkout',
          target: { kind: 'remoteBranch', remoteBranch: `${remote}/${branch}` }
        });
      }
    },
    {
      id: 'copy-branch',
      label: t(translate, 'context-copy-branch'),
      icon: <Icon name="copy" size={12} />,
      onSelect: () => copy(`${remote}/${branch}`)
    },
    {
      id: 'rename',
      label: t(translate, 'context-rename'),
      icon: <Icon name="pencil" size={12} />,
      disabled: blocked,
      separatorBefore: true,
      onSelect: () => openOverlay({ kind: 'renameRemoteBranch', remote, old: branch })
    },
    {
      id: 'delete',
      label: t(translate, 'context-delete'),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked,
      onSelect: () => openOverlay({ kind: 'deleteRemoteBranch', remote, branch })
    }
  ];

  const stashEntries = (reference: string): ContextMenuEntry[] => [
    {
      id: 'pop',
      label: t(translate, 'menu-stash-pop'),
      icon: <Icon name="archive-restore" size={12} />,
      disabled: blocked,
      onSelect: () => {
        void runAction(repo.id, { action: 'stashPop', stashRef: reference });
      }
    },
    {
      id: 'drop',
      label: t(translate, 'menu-stash-drop'),
      icon: <Icon name="trash-2" size={12} />,
      disabled: blocked,
      onSelect: () => openOverlay({ kind: 'stashDrop', reference })
    }
  ];

  return (
    <div className={`sidebar${compact ? ' sidebar--sidecar' : ''}`} data-testid="sidebar">
      <div className="panel-header" data-testid="sidebar-header">
        {t(translate, 'sidebar-repo')}
      </div>
      <div
        className="sidebar__scroll"
        ref={scrollRef}
        data-keyboard-list="refs"
        onScroll={
          compact
            ? (event) =>
                patchSidecarUi(repo.id, { branchesScrollTop: event.currentTarget.scrollTop })
            : undefined
        }
        onKeyDown={(event) => moveListFocus(event, shortcuts.resolved)}
      >
        <Section
          sectionKey="branches"
          title={t(translate, 'section-branches')}
          count={repo.branches.length}
          collapsed={isCollapsed('branches')}
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
                className={`ref-row${branch.is_head ? ' is-head' : ''}`}
                data-keyboard-list-item
                data-testid={`branch-${branch.name}`}
                title={branch.name}
                onKeyDown={(event) =>
                  checkoutOnSpace(
                    event,
                    keysForCommand(shortcuts.resolved, 'refs.checkout'),
                    !blocked && !branch.is_head,
                    () =>
                      runAction(repo.id, {
                        action: 'checkout',
                        target: { kind: 'localBranch', localBranch: branch.name }
                      })
                  )
                }
                onDoubleClick={() => {
                  if (!blocked && !branch.is_head) {
                    void runAction(repo.id, {
                      action: 'checkout',
                      target: { kind: 'localBranch', localBranch: branch.name }
                    });
                  }
                }}
                onClick={() => {
                  setMessage(
                    repo.id,
                    ta(translate, 'branch-selected', { name: branch.name }),
                    null
                  );
                }}
              >
                <span className={`ref-marker${branch.is_head ? ' ref-marker--head' : ''}`} />
                <span className="ref-row__label">{branch.name}</span>
              </button>
            </ContextMenu>
          ))}
        </Section>

        <Section
          sectionKey="remote-branches"
          title={t(translate, 'section-remote-branches')}
          count={refs.remote_branches.length}
          collapsed={isCollapsed('remote-branches')}
          onToggle={toggle}
        >
          {groups.map((group) => {
            const isRealRemote = group.remote !== OTHER_REMOTE_GROUP;
            const header = (
              <div className="sidebar__section-header-row">
                <button
                  type="button"
                  className="sidebar__section-header sidebar__group-header"
                  onClick={() => toggle(`remote-${group.remote}`)}
                  data-testid={`sidebar-toggle-remote-${group.remote}`}
                >
                  <Icon
                    name={isCollapsed(`remote-${group.remote}`) ? 'chevron-right' : 'chevron-down'}
                    size={12}
                  />
                  <Icon name="git-branch" size={12} />
                  <span className="sidebar__section-title">{group.remote}</span>
                  <span className="sidebar__section-count">{group.branches.length}</span>
                </button>
                {isRealRemote ? (
                  <IconButton
                    icon={<Icon name="refresh-cw" size={12} />}
                    tooltip={ta(translate, 'context-fetch-remote', { remote: group.remote })}
                    disabled={!networkAvailable}
                    onClick={() =>
                      void runAction(repo.id, { action: 'fetchRemote', remote: group.remote })
                    }
                    testId={`remote-fetch-${group.remote}`}
                  />
                ) : null}
              </div>
            );
            return (
              <div key={group.remote} className="sidebar__section">
                {isRealRemote ? (
                  <ContextMenu
                    testId={`remote-group-${group.remote}`}
                    entries={remoteMenuEntries(remoteGroupItems(group.remote, menuOptions))}
                  >
                    {header}
                  </ContextMenu>
                ) : (
                  header
                )}
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
                          data-keyboard-list-item
                          style={{ paddingLeft: 20 }}
                          data-testid={`remote-branch-${entry.fullName}`}
                          title={entry.fullName}
                          onKeyDown={(event) =>
                            checkoutOnSpace(
                              event,
                              keysForCommand(shortcuts.resolved, 'refs.checkout'),
                              !blocked,
                              () =>
                                runAction(repo.id, {
                                  action: 'checkout',
                                  target: {
                                    kind: 'remoteBranch',
                                    remoteBranch: entry.fullName
                                  }
                                })
                            )
                          }
                          onDoubleClick={() => {
                            if (!blocked) {
                              void runAction(repo.id, {
                                action: 'checkout',
                                target: {
                                  kind: 'remoteBranch',
                                  remoteBranch: entry.fullName
                                }
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
            );
          })}
        </Section>

        <Section
          sectionKey="tags"
          title={t(translate, 'section-tags')}
          count={refs.tags.length}
          collapsed={isCollapsed('tags')}
          onToggle={toggle}
          action={
            <IconButton
              icon={<Icon name="refresh-cw" size={12} />}
              tooltip={t(translate, 'tags-sync-warning')}
              // A conflicted worktree still syncs its tags, so this deliberately
              // does not reuse `blocked`; only a missing remote or a command
              // already in flight can stop it.
              disabled={!canSyncTags}
              onClick={() => void runAction(repo.id, { action: 'syncTags' })}
              testId="tags-sync"
            />
          }
        >
          {refs.tags.map((name) => (
            <ContextMenu key={name} testId={`tag-${name}`} entries={tagEntries(name)}>
              <button
                type="button"
                className="ref-row"
                data-keyboard-list-item
                data-testid={`tag-row-${name}`}
                title={name}
                onKeyDown={(event) =>
                  checkoutOnSpace(
                    event,
                    keysForCommand(shortcuts.resolved, 'refs.checkout'),
                    !blocked,
                    () =>
                      runAction(repo.id, { action: 'checkout', target: { kind: 'tag', tag: name } })
                  )
                }
                onDoubleClick={() => {
                  if (!blocked) {
                    void runAction(repo.id, {
                      action: 'checkout',
                      target: { kind: 'tag', tag: name }
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
          title={t(translate, 'section-stashes')}
          count={refs.stashes.length}
          collapsed={isCollapsed('stashes')}
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
                data-keyboard-list-item
                data-testid={`stash-row-${stash.reference}`}
                title={stash.description}
                onKeyDown={(event) => {
                  if (event.key === ' ' && !hasOpenPopup()) event.preventDefault();
                }}
                onClick={() => {
                  void runAction(repo.id, { action: 'stashPop', stashRef: stash.reference });
                }}
              >
                <span className="ref-marker" />
                <span className="ref-row__label">{stash.description}</span>
              </button>
            </ContextMenu>
          ))}
        </Section>
      </div>
    </div>
  );
}

function checkoutOnSpace(
  event: React.KeyboardEvent<HTMLElement>,
  keys: string[],
  allowed: boolean,
  checkout: () => void
): void {
  if (hasOpenPopup() || !matchesShortcut(event.nativeEvent, keys)) {
    return;
  }
  event.preventDefault();
  event.stopPropagation();
  if (!event.repeat && allowed) {
    checkout();
  }
}
