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

import { useMemo, useState } from 'react';

import { Icon } from '../../components/Icon';
import { ContextMenu, IconButton, VirtualList } from '../../components/controls';
import type { DiffStatCategory, FileStatus, WorkingTreeAction } from '../../bridge/types';
import * as ipc from '../../bridge/ipc';
import {
  codeFor,
  groupFiles,
  isConflicted,
  isUntracked,
  useStore,
  type RepoState
} from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { hasOpenPopup, keysForCommand, matchesShortcut } from '../../app/keyboard';
import { copyAgentPrompt } from '../agentPrompt/copyAgentPrompt';

interface Group {
  key: 'staged' | 'changes';
  /** The catalog key for the header, which the reference names explicitly. */
  titleKey: string;
  files: FileStatus[];
}

type ChangesListItem =
  | { kind: 'summary'; group: Group }
  | { kind: 'section'; group: Group }
  | { kind: 'file'; group: Group; file: FileStatus };

const changeItemHeight = (item: ChangesListItem) => (item.kind === 'section' ? 24 : 22);

export function ChangesPanel({
  repo,
  compact = false,
  onOpenDiff
}: {
  repo: RepoState;
  compact?: boolean;
  onOpenDiff?: () => void;
}) {
  const translate = useStore((state) => state.t);
  const showUntracked = useStore((state) => state.config.view.show_untracked);
  const selectWorkingFile = useStore((state) => state.selectWorkingFile);
  const openOverlay = useStore((state) => state.openOverlay);
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const shortcuts = useStore((state) => state.shortcuts);
  const ui = useStore((state) => state.sidecarUi[repo.id]);
  const patchSidecarUi = useStore((state) => state.patchSidecarUi);
  const actionsBlocked = repo.busy || repo.status !== 'ready' || repo.statusStale;
  // The two groups are not exclusive: a partially staged file appears in both,
  // because its two diffs are different files as far as Git is concerned.
  const { staged, unstaged } = useMemo(
    () => groupFiles(repo.files, showUntracked),
    [repo.files, showUntracked]
  );
  const groups = useMemo(() => {
    const result: Group[] = [];
    if (staged.length) result.push({ key: 'staged', titleKey: 'section-staged', files: staged });
    if (unstaged.length)
      result.push({ key: 'changes', titleKey: 'section-changes', files: unstaged });
    return result;
  }, [staged, unstaged]);
  const items = useMemo(() => {
    const result: ChangesListItem[] = [];
    for (const group of groups) {
      result.push({ kind: 'summary', group }, { kind: 'section', group });
      if (!collapsed.includes(group.key)) {
        for (const file of group.files) result.push({ kind: 'file', group, file });
      }
    }
    return result;
  }, [collapsed, groups]);
  const fileItemIndexes = useMemo(
    () => items.flatMap((item, index) => (item.kind === 'file' ? [index] : [])),
    [items]
  );
  const [focusIndex, setFocusIndex] = useState<number | null>(null);

  const toggle = (key: string) => {
    setCollapsed((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : [...current, key]
    );
  };

  const operate = async (action: WorkingTreeAction, files: FileStatus[], all: boolean) => {
    if (actionsBlocked || files.length === 0) {
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

  const onListKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (hasOpenPopup()) return;
    const command = matchesShortcut(
      event.nativeEvent,
      keysForCommand(shortcuts.resolved, 'list.next')
    )
      ? 'next'
      : matchesShortcut(event.nativeEvent, keysForCommand(shortcuts.resolved, 'list.previous'))
        ? 'previous'
        : null;
    if (!command || fileItemIndexes.length === 0) return;
    const current = (event.target as HTMLElement).closest<HTMLElement>('[data-file-index]');
    const currentIndex = current ? Number(current.dataset.fileIndex) : -1;
    const position = fileItemIndexes.indexOf(currentIndex);
    const nextPosition = Math.max(
      0,
      Math.min(
        fileItemIndexes.length - 1,
        position < 0 ? 0 : position + (command === 'next' ? 1 : -1)
      )
    );
    event.preventDefault();
    setFocusIndex(fileItemIndexes[nextPosition]!);
  };

  // Count each changed path once, including files with both staged and unstaged changes.
  const total = useMemo(
    () => new Set([...staged, ...unstaged].map((file) => file.path)).size,
    [staged, unstaged]
  );

  return (
    <div
      className={`changes${compact ? ' changes--sidecar' : ''}`}
      data-testid={groups.length ? 'changes-panel' : 'changes-empty'}
    >
      <div className="panel-header panel-header--compact" data-testid="changes-header">
        <span>{t(translate, 'changes-title')}</span>
        {repo.hasConflicts ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            disabled={actionsBlocked}
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
      <VirtualList
        items={items}
        rowHeight={22}
        itemHeight={changeItemHeight}
        overscan={8}
        className="changes__scroll"
        testId="changes-scroll"
        keyboardList="changes"
        focusIndex={focusIndex}
        initialScrollTop={compact ? (ui?.changesScrollTop ?? 0) : 0}
        onScrollPosition={
          compact ? (changesScrollTop) => patchSidecarUi(repo.id, { changesScrollTop }) : undefined
        }
        onKeyDown={onListKeyDown}
        empty={
          <div className="empty-state" style={{ minHeight: 120 }}>
            <span className="empty-state__message">
              {repo.status === 'loading'
                ? ta(translate, 'status-scanning-at', {
                    repo: repo.path.split(/[/\\]/).filter(Boolean).pop() ?? repo.path
                  })
                : repo.status === 'error' || repo.statusStale
                  ? (repo.errorMessage ?? t(translate, 'err-unknown'))
                  : t(translate, 'changes-empty')}
            </span>
          </div>
        }
        renderRow={(item, index) => {
          const { group } = item;
          const stagedGroup = group.key === 'staged';
          const groupTitle = t(translate, group.titleKey);
          if (item.kind === 'summary') {
            const categories: DiffStatCategory[] = stagedGroup
              ? [repo.diffStatsReport.staged]
              : showUntracked
                ? [repo.diffStatsReport.unstaged, repo.diffStatsReport.untracked]
                : [repo.diffStatsReport.unstaged];
            const stats = stagedGroup
              ? repo.diffStatsReport.staged.stats
              : showUntracked
                ? combineStats(
                    repo.diffStatsReport.unstaged.stats,
                    repo.diffStatsReport.untracked.stats
                  )
                : repo.diffStatsReport.unstaged.stats;
            const reason = stats ? null : statsUnavailableReason(categories);
            const statsLabel = stats ? `+${stats.added} −${stats.deleted}` : '—';
            return (
              <div
                className="changes__summary"
                data-testid={`changes-stats-${group.key}`}
                aria-label={`${groupTitle}: ${group.files.length}, ${statsLabel}`}
              >
                <span className="changes__summary-files">
                  <Icon name="file" size={11} />
                  <span data-testid={`changes-stats-files-${group.key}`}>{group.files.length}</span>
                </span>
                {stats ? (
                  <span className="changes__summary-lines">
                    <span
                      className="stat-bar__added"
                      data-testid={`changes-stats-added-${group.key}`}
                    >
                      +{stats.added}
                    </span>
                    <span
                      className="stat-bar__deleted"
                      data-testid={`changes-stats-deleted-${group.key}`}
                    >
                      −{stats.deleted}
                    </span>
                  </span>
                ) : showUntracked && !stagedGroup ? (
                  <span className="changes__summary-lines" aria-label={reason ?? undefined}>
                    <CategoryStat
                      category={repo.diffStatsReport.unstaged}
                      label={t(translate, 'changes-stats-tracked')}
                      reasonText={t(
                        translate,
                        `diff-stats-${repo.diffStatsReport.unstaged.reason ?? 'pending'}`
                      )}
                      testId="tracked"
                    />
                    <CategoryStat
                      category={repo.diffStatsReport.untracked}
                      label={t(translate, 'changes-stats-untracked')}
                      reasonText={t(
                        translate,
                        `diff-stats-${repo.diffStatsReport.untracked.reason ?? 'pending'}`
                      )}
                      testId="untracked"
                    />
                  </span>
                ) : (
                  <span
                    className="changes__summary-unavailable"
                    title={reason ? t(translate, `diff-stats-${reason}`) : undefined}
                    data-testid={`changes-stats-reason-${group.key}`}
                  >
                    —
                  </span>
                )}
              </div>
            );
          }
          if (item.kind === 'section') {
            const isCollapsed = collapsed.includes(group.key);
            return (
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
                    disabled={actionsBlocked}
                    onClick={() => void operate('unstage', group.files, true)}
                    testId="changes-unstage-all"
                  />
                ) : (
                  <>
                    <IconButton
                      icon={<Icon name="undo" size={12} />}
                      tooltip={t(translate, 'changes-discard-all')}
                      disabled={actionsBlocked || repo.hasConflicts}
                      onClick={() => void operate('discard', group.files, true)}
                      testId="changes-discard-all"
                    />
                    <IconButton
                      icon={<Icon name="plus" size={12} />}
                      tooltip={t(translate, 'changes-stage-all')}
                      disabled={actionsBlocked || repo.hasConflicts}
                      onClick={() => void operate('stage', group.files, true)}
                      testId="changes-stage-all"
                    />
                  </>
                )}
              </div>
            );
          }
          return (
            <FileRow
              repo={repo}
              file={item.file}
              fileIndex={index}
              staged={stagedGroup}
              actionsBlocked={actionsBlocked}
              selected={
                repo.pane.kind === 'working' &&
                repo.pane.staged === stagedGroup &&
                repo.pane.file.path === item.file.path
              }
              onSelect={() => {
                void selectWorkingFile(repo.id, stagedGroup, item.file);
                onOpenDiff?.();
              }}
              onOperate={(action) => void operate(action, [item.file], false)}
            />
          );
        }}
      />
    </div>
  );
}

function CategoryStat({
  category,
  label,
  reasonText,
  testId
}: {
  category: DiffStatCategory;
  label: string;
  reasonText: string;
  testId: string;
}) {
  return category.stats ? (
    <span
      className="changes__summary-category"
      title={label}
      aria-label={`${label}: +${category.stats.added}, −${category.stats.deleted}`}
      data-testid={`changes-stats-changes-${testId}`}
    >
      <span className="changes__summary-category-label">{label}</span>
      <span className="stat-bar__added">+{category.stats.added}</span>
      <span className="stat-bar__deleted">−{category.stats.deleted}</span>
    </span>
  ) : (
    <span
      className="changes__summary-unavailable"
      title={`${label}: ${reasonText}`}
      aria-label={`${label}: ${reasonText}`}
      data-testid={`changes-stats-changes-${testId}`}
    >
      <span className="changes__summary-category-label">{label}</span>—
    </span>
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

function statsUnavailableReason(categories: DiffStatCategory[]): string {
  const failure = categories.find((category) => category.reason);
  if (failure?.reason) return failure.reason;
  return categories.some((category) => category.status === 'pending') ? 'pending' : 'unavailable';
}

function FileRow({
  repo,
  file,
  fileIndex,
  staged,
  actionsBlocked,
  selected,
  onSelect,
  onOperate
}: {
  repo: RepoState;
  file: FileStatus;
  fileIndex: number;
  staged: boolean;
  actionsBlocked: boolean;
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
      disabled: actionsBlocked || conflicted,
      onSelect: () => onOperate(staged ? 'unstage' : 'stage')
    },
    {
      id: 'discard',
      label: t(translate, 'changes-discard'),
      icon: <Icon name="undo" size={12} />,
      disabled: actionsBlocked || repo.hasConflicts || conflicted,
      onSelect: () => onOperate('discard')
    }
  ];

  return (
    <ContextMenu testId={`changes-row-${file.path}`} entries={entries}>
      <div
        className={`file-row changes__row-group${selected ? ' is-selected' : ''}`}
        tabIndex={0}
        data-keyboard-list-item
        data-file-index={fileIndex}
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
          if (!event.repeat && !actionsBlocked && !conflicted) {
            onOperate(staged ? 'unstage' : 'stage');
          }
        }}
        title={file.old_path ? `${file.old_path} → ${file.path}` : file.path}
      >
        <span className={`file-row__status status-${modifier}`}>
          {t(translate, porcelainKey(code, untracked))}
        </span>
        <span className="file-row__name">{file.path}</span>
        {file.is_lfs ? (
          <span className="file-row__lfs" title={t(translate, 'lfs-file-tag')}>
            LFS
          </span>
        ) : null}
        <IconButton
          icon={staged ? <Icon name="minus" size={12} /> : <Icon name="plus" size={12} />}
          tooltip={
            conflicted
              ? blocked
              : staged
                ? t(translate, 'changes-unstage')
                : t(translate, 'changes-stage')
          }
          disabled={actionsBlocked || conflicted}
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
    return 'status-add';
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
    return 'add';
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
