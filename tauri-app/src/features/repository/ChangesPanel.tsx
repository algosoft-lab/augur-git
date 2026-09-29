/**
 * The working-tree panel: the staged group and the changes group.
 *
 * Git reports an unmerged file on both sides, so the reference application
 * keeps it in the unstaged group and lets the row's status character show the
 * conflict. That is reproduced here: two groups, not three, with the conflicts
 * counted as changes.
 *
 * Row actions are revealed on hover. Discarding is gated while conflicts exist,
 * because a restore during an unresolved merge would destroy the work that the
 * merge is waiting on, and a discard always goes through a confirmation because
 * it destroys work.
 */

import { useState } from 'react';

import { Icon } from '../../components/Icon';
import { ContextMenu, IconButton } from '../../components/controls';
import type { FileStatus, WorkingTreeAction } from '../../bridge/types';
import * as ipc from '../../bridge/ipc';
import {
  codeFor,
  groupFiles,
  isConflicted,
  isUntracked,
  useStore,
  type RepoState
} from '../../app/store';
import { t } from '../../i18n/strings';
import { hasOpenPopup, keysForCommand, matchesShortcut, moveListFocus } from '../../app/keyboard';
import { copyAgentPrompt } from '../agentPrompt/copyAgentPrompt';

interface Group {
  key: 'staged' | 'changes';
  /** The catalog key for the header, which the reference names explicitly. */
  titleKey: string;
  files: FileStatus[];
}

export function ChangesPanel({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const showUntracked = useStore((state) => state.config.view.show_untracked);
  const selectWorkingFile = useStore((state) => state.selectWorkingFile);
  const openOverlay = useStore((state) => state.openOverlay);
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const shortcuts = useStore((state) => state.shortcuts);

  const busy = repo.busy;
  // The two groups are not exclusive: a partially staged file appears in both,
  // because its two diffs are different files as far as Git is concerned.
  const { staged, unstaged } = groupFiles(repo.files, showUntracked);
  const changesStats = showUntracked
    ? combineStats(repo.diffStats.unstaged, repo.diffStats.untracked)
    : repo.diffStats.unstaged;
  const groups: (Group & { stats: { added: number; deleted: number } | null })[] = [];
  if (staged.length) {
    groups.push({
      key: 'staged',
      titleKey: 'section-staged',
      files: staged,
      stats: repo.diffStats.staged
    });
  }
  if (unstaged.length) {
    groups.push({
      key: 'changes',
      titleKey: 'section-changes',
      files: unstaged,
      stats: changesStats
    });
  }

  const toggle = (key: string) => {
    setCollapsed((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : [...current, key]
    );
  };

  const operate = async (action: WorkingTreeAction, files: FileStatus[], all: boolean) => {
    if (busy || files.length === 0) {
      return;
    }
    if (action === 'discard') {
      const tracked = files.filter((file) => !isUntracked(file)).length;
      openOverlay({
        kind: 'discard',
        scope: { kind: 'workingTree', staged: false, all },
        trackedCount: tracked,
        untrackedCount: files.length - tracked
      });
      return;
    }
    try {
      const requestId = await ipc.workingTreeOperation(repo.id, action, files, all);
      useStore.getState().setBusy(repo.id, true);
      void requestId;
    } catch (error) {
      // Localized, not the raw detail: a refused command has a sentence written
      // for it, and the detail is only half of that sentence.
      useStore.getState().reportError(repo.id, error);
    }
  };

  // The total counts every changed path, so a file that is in both groups is
  // counted once: it is one thing that has two states, not two things.
  const total = new Set<string>([
    ...staged.map((file) => file.path),
    ...unstaged.map((file) => file.path)
  ]).size;

  return (
    <div className="changes" data-testid={groups.length ? 'changes-panel' : 'changes-empty'}>
      <div className="panel-header panel-header--compact" data-testid="changes-header">
        <span>{t(translate, 'changes-title')}</span>
        {repo.hasConflicts ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            disabled={busy}
            data-testid="agent-prompt-resolve-conflicts"
            onClick={() => void copyAgentPrompt(repo.id, { kind: 'resolveConflicts' })}
          >
            <Icon name="copy" size={11} />
            {t(translate, 'agent-prompt-resolve-conflicts')}
          </button>
        ) : null}
        <span className="panel-header__count" data-testid="changes-total">
          {total}
        </span>
      </div>
      <div
        className="changes__scroll"
        data-keyboard-list="changes"
        data-testid="changes-scroll"
        onKeyDown={(event) => moveListFocus(event, shortcuts.resolved)}
      >
        {groups.length === 0 ? (
          <div className="empty-state" style={{ minHeight: 120 }}>
            <span className="empty-state__message">{t(translate, 'changes-empty')}</span>
          </div>
        ) : null}
        {groups.map((group) => {
          const stagedGroup = group.key === 'staged';
          const isCollapsed = collapsed.includes(group.key);
          const groupTitle = t(translate, group.titleKey);
          const statsLabel = group.stats ? `+${group.stats.added} −${group.stats.deleted}` : '—';
          return (
            <div key={group.key}>
              <div
                className="changes__summary"
                data-testid={`changes-stats-${group.key}`}
                aria-label={`${groupTitle}: ${group.files.length}, ${statsLabel}`}
              >
                <span className="changes__summary-files">
                  <Icon name="file" size={11} />
                  <span data-testid={`changes-stats-files-${group.key}`}>{group.files.length}</span>
                </span>
                {group.stats ? (
                  <span className="changes__summary-lines">
                    <span
                      className="stat-bar__added"
                      data-testid={`changes-stats-added-${group.key}`}
                    >
                      +{group.stats.added}
                    </span>
                    <span
                      className="stat-bar__deleted"
                      data-testid={`changes-stats-deleted-${group.key}`}
                    >
                      −{group.stats.deleted}
                    </span>
                  </span>
                ) : (
                  <span className="changes__summary-unavailable">—</span>
                )}
              </div>
              <div className="changes__row-group changes__section-header">
                <button
                  type="button"
                  className="changes__section-header"
                  style={{ flex: '1 1 auto' }}
                  aria-expanded={!isCollapsed}
                  onClick={() => toggle(group.key)}
                  data-testid={`changes-toggle-${group.key}`}
                >
                  <Icon name={isCollapsed ? 'chevron-right' : 'chevron-down'} size={12} />
                  <span className="changes__section-title">{groupTitle}</span>
                </button>
                {stagedGroup ? (
                  <IconButton
                    icon={<Icon name="minus" size={12} />}
                    tooltip={t(translate, 'changes-unstage-all')}
                    disabled={busy}
                    onClick={() => void operate('unstage', group.files, true)}
                    testId="changes-unstage-all"
                  />
                ) : (
                  <>
                    <IconButton
                      icon={<Icon name="undo" size={12} />}
                      tooltip={t(translate, 'changes-discard-all')}
                      disabled={busy || repo.hasConflicts}
                      onClick={() => void operate('discard', group.files, true)}
                      testId="changes-discard-all"
                    />
                    <IconButton
                      icon={<Icon name="plus" size={12} />}
                      tooltip={t(translate, 'changes-stage-all')}
                      disabled={busy || repo.hasConflicts}
                      onClick={() => void operate('stage', group.files, true)}
                      testId="changes-stage-all"
                    />
                  </>
                )}
              </div>
              {isCollapsed
                ? null
                : group.files.map((file) => (
                    <FileRow
                      key={`${group.key}-${file.path}`}
                      repo={repo}
                      file={file}
                      staged={stagedGroup}
                      selected={
                        repo.pane.kind === 'working' &&
                        repo.pane.staged === stagedGroup &&
                        repo.pane.file.path === file.path
                      }
                      onSelect={() => {
                        void selectWorkingFile(repo.id, stagedGroup, file);
                      }}
                      onOperate={(action) => void operate(action, [file], false)}
                    />
                  ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function combineStats(
  first: { added: number; deleted: number } | null,
  second: { added: number; deleted: number } | null
): { added: number; deleted: number } | null {
  if (!first || !second) {
    return null;
  }
  return {
    added: first.added + second.added,
    deleted: first.deleted + second.deleted
  };
}

function FileRow({
  repo,
  file,
  staged,
  selected,
  onSelect,
  onOperate
}: {
  repo: RepoState;
  file: FileStatus;
  staged: boolean;
  selected: boolean;
  onSelect: () => void;
  onOperate: (action: WorkingTreeAction) => void;
}) {
  const translate = useStore((state) => state.t);
  const code = codeFor(file, staged);
  const conflicted = isConflicted(file);
  const untracked = isUntracked(file);
  const modifier = porcelainModifier(code, untracked);

  // A conflicted file keeps its actions, disabled and explained, rather than
  // losing them. An entry that has vanished says nothing about why the operation
  // is impossible, which is the one thing worth saying about a conflict.
  const blocked = t(translate, 'changes-action-conflict');
  const entries = [
    {
      id: 'toggle-stage',
      label: staged ? t(translate, 'changes-unstage') : t(translate, 'changes-stage'),
      icon: <Icon name={staged ? 'minus' : 'plus'} size={12} />,
      disabled: repo.busy || conflicted,
      onSelect: () => onOperate(staged ? 'unstage' : 'stage')
    },
    {
      id: 'discard',
      label: t(translate, 'changes-discard'),
      icon: <Icon name="undo" size={12} />,
      disabled: repo.busy || repo.hasConflicts || conflicted,
      onSelect: () => onOperate('discard')
    }
  ];

  return (
    <ContextMenu testId={`changes-row-${file.path}`} entries={entries}>
      <div
        className={`file-row changes__row-group${selected ? ' is-selected' : ''}`}
        tabIndex={0}
        data-keyboard-list-item
        data-testid={`changes-file-${file.path}`}
        onFocus={onSelect}
        onClick={(event) => {
          if (document.activeElement !== event.currentTarget) {
            event.currentTarget.focus();
          }
        }}
        onKeyDown={(event) => {
          if (
            hasOpenPopup() ||
            !matchesShortcut(
              event.nativeEvent,
              keysForCommand(useStore.getState().shortcuts.resolved, 'changes.toggle-stage')
            )
          ) {
            return;
          }
          event.preventDefault();
          if (!event.repeat && !repo.busy && !conflicted) {
            onOperate(staged ? 'unstage' : 'stage');
          }
        }}
        title={file.old_path ? `${file.old_path} → ${file.path}` : file.path}
      >
        <span className={`file-row__status status-${modifier}`}>
          {t(translate, porcelainKey(code, untracked))}
        </span>
        <span className="file-row__name">{file.path}</span>
        <IconButton
          icon={staged ? <Icon name="minus" size={12} /> : <Icon name="plus" size={12} />}
          tooltip={
            conflicted
              ? blocked
              : staged
                ? t(translate, 'changes-unstage')
                : t(translate, 'changes-stage')
          }
          disabled={repo.busy || conflicted}
          onClick={() => onOperate(staged ? 'unstage' : 'stage')}
          testId={`changes-toggle-${file.path}`}
        />
      </div>
    </ContextMenu>
  );
}

/** Map a porcelain status character to its catalog key. */
export function porcelainKey(code: string, untracked: boolean): string {
  if (untracked) {
    return 'status-unknown';
  }
  switch (code) {
    case 'A':
      return 'status-add';
    case 'D':
      return 'status-del';
    case 'M':
      return 'status-mod';
    case 'R':
      return 'status-ren';
    case 'C':
      return 'status-cpy';
    case 'U':
      return 'status-conflict';
    default:
      return 'status-unknown';
  }
}

/** The CSS colour modifier for a porcelain status character. */
export function porcelainModifier(code: string, untracked: boolean): string {
  if (untracked) {
    return 'unknown';
  }
  switch (code) {
    case 'A':
      return 'add';
    case 'D':
      return 'del';
    case 'M':
      return 'mod';
    case 'R':
      return 'ren';
    case 'C':
      return 'cpy';
    case 'U':
      return 'conflict';
    default:
      return 'unknown';
  }
}
