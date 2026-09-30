/**
 * The bottom panel: the changed-file list on the left and the diff on the right.
 *
 * Selecting a commit shows every file it changed at once, which is what the
 * reference application does; choosing a file narrows the view to that one. The
 * header always names what is displayed, so a stale panel cannot be mistaken for
 * current content.
 */

import { useEffect, useMemo, useRef, useState } from 'react';

import { writeText } from '@tauri-apps/plugin-clipboard-manager';

import { EmptyState, Splitter } from '../../components/controls';
import { Icon } from '../../components/Icon';
import type { FileChange } from '../../bridge/types';
import { statBlocks, statusKey, statusModifier } from './fileMeta';
import { groupFiles, useStore, type RepoState } from '../../app/store';
import { DiffView, NARROW_WIDTH, type DiffSection } from './DiffView';
import { ta, t } from '../../i18n/strings';

export function BottomPanel({
  repo,
  height,
  sidecar = false,
  onBack,
  onFileListRatioChange,
  onFileListRatioChangeEnd
}: {
  repo: RepoState;
  height: number | null;
  sidecar?: boolean;
  onBack?: () => void;
  onFileListRatioChange: (ratio: number) => void;
  onFileListRatioChangeEnd: () => void;
}) {
  const translate = useStore((state) => state.t);
  const layout = useStore((state) => state.config.view.diff_layout);
  const softWrap = useStore((state) => state.config.view.diff_soft_wrap);
  const ratio = useStore((state) => state.workspace.layout.file_list_ratio);
  const setView = useStore((state) => state.setView);
  const selectCommitFile = useStore((state) => state.selectCommitFile);
  const selectWorkingFile = useStore((state) => state.selectWorkingFile);
  const selectCommit = useStore((state) => state.selectCommit);
  const clearCommit = useStore((state) => state.clearCommit);
  const [collapsed, setCollapsed] = useState(false);
  const [width, setWidth] = useState(1000);
  const bodyRef = useRef<HTMLDivElement>(null);
  const fileListDragStart = useRef(ratio);

  const pane = repo.pane;
  const commit = repo.selected;
  const showFileList = pane.kind === 'commit' && repo.commitFiles.length > 0;
  const workingFiles = useMemo(() => groupFiles(repo.files, true), [repo.files]);

  useEffect(() => {
    const element = bodyRef.current;
    if (!element) {
      return;
    }
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    setWidth(element.clientWidth);
    return () => observer.disconnect();
  }, []);

  const sections = useMemo<DiffSection[]>(() => {
    if (pane.kind === 'commit' && pane.file) {
      const document = repo.commitDiffs[pane.file.new_path];
      return document ? [{ path: pane.file.new_path, document }] : [];
    }
    if (pane.kind === 'commit') {
      return repo.commitFiles
        .map((file) => {
          const document = repo.commitDiffs[file.new_path];
          return document ? { path: file.new_path, document } : null;
        })
        .filter((entry): entry is DiffSection => entry !== null);
    }
    if (pane.kind === 'working' && repo.workingDocument) {
      return [{ path: pane.file.path, document: repo.workingDocument }];
    }
    return [];
  }, [pane, repo.commitFiles, repo.commitDiffs, repo.workingDocument]);

  const pendingCommitDiffCount = Object.keys(repo.commitDiffPending).length;
  const commitDiffErrorCount = Object.keys(repo.commitDiffErrors).length;
  const focusedCommitError =
    pane.kind === 'commit' && pane.file
      ? (repo.commitDiffErrors[pane.file.new_path] ?? null)
      : null;
  const commitLoading =
    pane.kind === 'commit' &&
    !repo.commitFilesError &&
    (repo.commitFilesLoading ||
      (pane.file
        ? repo.commitDiffPending[pane.file.new_path] === true
        : pendingCommitDiffCount > 0));
  const commitError =
    pane.kind === 'commit'
      ? (repo.commitFilesError ??
        focusedCommitError ??
        (sections.length === 0 ? (Object.values(repo.commitDiffErrors)[0] ?? null) : null))
      : null;
  const canRetryCommitDiff =
    pane.kind === 'commit' &&
    commit !== null &&
    (repo.commitFilesError !== null || commitDiffErrorCount > 0) &&
    !repo.commitFilesLoading &&
    pendingCommitDiffCount === 0;

  const title = (() => {
    if (pane.kind === 'working') {
      return t(translate, pane.staged ? 'diff-working-tree-staged' : 'diff-working-tree-changes');
    }
    if (commit) {
      return commit.subject;
    }
    return t(translate, 'bottom-no-commit');
  })();

  // A wide view of many files forces the inline layout, because a side-by-side
  // layout of every file at once leaves each pane too narrow to read.
  const multiFile = sections.length > 1;
  const narrow = width < NARROW_WIDTH;

  // The commit's own totals, so the size of the change is readable without
  // summing the file list.
  const commitTotals =
    pane.kind === 'commit' && repo.commitFiles.length
      ? repo.commitFiles.reduce(
          (sum, file) => ({
            added: sum.added + (file.added ?? 0),
            deleted: sum.deleted + (file.deleted ?? 0)
          }),
          { added: 0, deleted: 0 }
        )
      : null;

  /**
   * Put the diff on the clipboard.
   *
   * Each document is prefixed with the header `git diff` would print, so a
   * pasted hunk says which file it came from. The single-file case gets it too,
   * because a copied hunk with no file in it cannot be pasted anywhere useful.
   */
  const copyDiff = () => {
    if (!sections.length) {
      return;
    }
    void writeText(
      sections.map((entry) => `diff -- ${entry.path}\n${entry.document.copy_text}`).join('')
    );
  };

  // The reference binds the secondary modifier with `c` in the diff area, which
  // is the gesture people reach for.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key.toLowerCase() === 'c' &&
        sections.length > 0
      ) {
        copyDiff();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [sections, repo.id]);

  if (pane.kind === 'none') {
    return (
      <div
        className={`bottom${height !== null ? ' bottom--fixed' : ''}${sidecar ? ' bottom--sidecar' : ''}`}
        style={height !== null ? { height } : undefined}
        data-testid="bottom-panel"
      >
        {sidecar ? (
          <div className="sidecar-diff-empty">
            <button
              type="button"
              className="tool-button"
              onClick={onBack}
              data-testid="sidecar-diff-back"
            >
              <Icon name="arrow-left" size={12} /> {t(translate, 'sidecar-back')}
            </button>
            <EmptyState
              icon={<Icon name="git-commit-horizontal" />}
              message={t(translate, 'bottom-no-commit')}
              testId="bottom-no-commit-state"
            />
          </div>
        ) : (
          <EmptyState
            icon={<Icon name="git-commit-horizontal" />}
            message={t(translate, 'bottom-no-commit')}
            testId="bottom-no-commit-state"
          />
        )}
      </div>
    );
  }

  return (
    <div
      className={`bottom${height !== null ? ' bottom--fixed' : ''}${sidecar ? ' bottom--sidecar' : ''}`}
      style={height !== null ? { height } : undefined}
      data-testid="bottom-panel"
    >
      <div className="bottom__toolbar">
        {sidecar ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid="sidecar-diff-back"
            title={t(translate, 'sidecar-back')}
            aria-label={t(translate, 'sidecar-back')}
            onClick={onBack}
          >
            <Icon name="arrow-left" size={12} />
          </button>
        ) : null}
        {!sidecar ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid="bottom-toggle-files"
            onClick={() => setCollapsed((value) => !value)}
          >
            <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={11} />
          </button>
        ) : null}
        {commit ? (
          <span className="mono bottom__commit-hash" data-testid="bottom-commit-hash">
            {commit.short}
          </span>
        ) : null}
        {sidecar ? (
          <select
            className="sidecar-diff-select"
            data-testid="sidecar-diff-file-select"
            aria-label={t(translate, 'sidecar-diff-file')}
            value={
              pane.kind === 'commit'
                ? (pane.file?.new_path ?? '__all__')
                : pane.kind === 'working'
                  ? `${pane.staged ? 'staged' : 'changes'}:${pane.file.path}`
                  : ''
            }
            onChange={(event) => {
              if (pane.kind === 'commit') {
                const file = repo.commitFiles.find(
                  (entry) => entry.new_path === event.currentTarget.value
                );
                void selectCommitFile(repo.id, file ?? null);
              } else if (pane.kind === 'working') {
                const [group, ...pathParts] = event.currentTarget.value.split(':');
                const path = pathParts.join(':');
                const staged = group === 'staged';
                const file = (staged ? workingFiles.staged : workingFiles.unstaged).find(
                  (entry) => entry.path === path
                );
                if (file) void selectWorkingFile(repo.id, staged, file);
              }
            }}
          >
            {pane.kind === 'commit' ? (
              <>
                <option value="__all__">{t(translate, 'diff-all-files')}</option>
                {repo.commitFiles.map((file) => (
                  <option key={file.new_path} value={file.new_path}>
                    {file.new_path}
                  </option>
                ))}
              </>
            ) : pane.kind === 'working' ? (
              <>
                {workingFiles.staged.length ? (
                  <optgroup label={t(translate, 'section-staged')}>
                    {workingFiles.staged.map((file) => (
                      <option key={`staged:${file.path}`} value={`staged:${file.path}`}>
                        {file.path}
                      </option>
                    ))}
                  </optgroup>
                ) : null}
                {workingFiles.unstaged.length ? (
                  <optgroup label={t(translate, 'section-changes')}>
                    {workingFiles.unstaged.map((file) => (
                      <option key={`changes:${file.path}`} value={`changes:${file.path}`}>
                        {file.path}
                      </option>
                    ))}
                  </optgroup>
                ) : null}
              </>
            ) : null}
          </select>
        ) : null}
        <span className="bottom__toolbar-title" title={title}>
          {title}
        </span>
        <span className="bottom__toolbar-spacer" />
        {repo.commitMergeParent && pane.kind === 'commit' ? (
          <span className="muted">{t(translate, 'diff-merge-first-parent')}</span>
        ) : null}
        {canRetryCommitDiff && commit ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid="bottom-retry-commit-diff"
            title={t(translate, 'bottom-commit-diff-retry')}
            aria-label={t(translate, 'bottom-commit-diff-retry')}
            onClick={() => void selectCommit(repo.id, commit.oid, commit.short, commit.subject)}
          >
            <Icon name="refresh-cw" size={12} />
          </button>
        ) : null}
        {/* The commit's own totals, so the size of the change is readable
            without summing the file list. */}
        {pane.kind === 'commit' && commitTotals ? (
          <StatBar
            added={commitTotals.added}
            deleted={commitTotals.deleted}
            testId="bottom-commit-stat"
          />
        ) : null}
        {/* The working-tree view has no commit, so it names the file and offers
            the copy, as the reference does. */}
        {pane.kind === 'working' && pane.file ? (
          <span
            className="bottom__toolbar-title mono"
            data-testid="bottom-working-path"
            title={pane.file.path}
          >
            {pane.file.path}
          </span>
        ) : null}
        <button
          type="button"
          className={`tool-button tool-button--compact${softWrap ? ' is-active' : ''}`}
          data-testid="bottom-soft-wrap"
          title={t(translate, 'diff-soft-wrap')}
          aria-label={t(translate, 'diff-soft-wrap')}
          aria-pressed={softWrap}
          onClick={() => void setView({ diff_soft_wrap: !softWrap })}
        >
          <Icon name="wrap-text" size={12} />
        </button>
        {sections.length ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid="bottom-copy-diff"
            title={t(translate, 'diff-copy')}
            onClick={copyDiff}
          >
            <Icon name="copy" size={12} />
          </button>
        ) : null}
        {commit ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid="bottom-clear-commit"
            title={t(translate, 'bottom-clear-selection')}
            aria-label={t(translate, 'bottom-clear-selection')}
            onClick={() => clearCommit(repo.id)}
          >
            <Icon name="x" size={11} />
          </button>
        ) : null}
      </div>
      <div className="bottom__body" ref={bodyRef}>
        {collapsed || !showFileList || sidecar ? null : (
          <>
            <div
              className="bottom__files"
              style={{ width: `${ratio * 100}%` }}
              data-testid="bottom-file-list"
            >
              <FileList
                files={repo.commitFiles}
                selected={pane.kind === 'commit' ? pane.file : null}
                onSelect={(file) => {
                  void selectCommitFile(repo.id, file);
                }}
              />
            </div>
            <Splitter
              orientation="vertical"
              label="resize file list"
              testId="bottom-file-splitter"
              onDragStart={() => {
                fileListDragStart.current = ratio;
              }}
              onDrag={(delta) => {
                const total = Math.max(1, bodyRef.current?.clientWidth ?? 600);
                onFileListRatioChange(clampRatio(fileListDragStart.current + delta / total));
              }}
              onDragEnd={onFileListRatioChangeEnd}
            />
          </>
        )}
        <DiffView
          sections={sections}
          layout={layout}
          softWrap={softWrap}
          forceInline={narrow}
          error={
            pane.kind === 'working'
              ? !repo.workingDocument
                ? repo.workingError
                : null
              : sections.length === 0
                ? commitError
                : null
          }
          // A bare spinner and a bare error both read as a broken panel; the
          // reference names both states.
          loading={pane.kind === 'working' ? repo.workingLoading : commitLoading}
          loadingMessage={
            pane.kind === 'working'
              ? t(translate, 'diff-working-tree-loading')
              : commitLoading
                ? t(translate, 'bottom-loading-commit')
                : undefined
          }
          errorLabel={
            pane.kind === 'working'
              ? t(translate, 'diff-working-tree-error')
              : commitError
                ? t(translate, 'bottom-commit-diff-error')
                : undefined
          }
          statusMessage={
            pane.kind === 'working'
              ? repo.workingDocument
                ? repo.workingInFlight !== null
                  ? {
                      kind: 'refreshing',
                      text: t(translate, 'diff-working-tree-refreshing')
                    }
                  : repo.workingError
                    ? {
                        kind: 'warning',
                        text: ta(translate, 'diff-working-tree-refresh-failed', {
                          error: repo.workingError.split('\n')[0] ?? repo.workingError
                        })
                      }
                    : null
                : null
              : sections.length > 0 && pendingCommitDiffCount > 0
                ? { kind: 'refreshing', text: t(translate, 'bottom-loading-commit') }
                : sections.length > 0 && commitDiffErrorCount > 0
                  ? {
                      kind: 'warning',
                      text: ta(translate, 'bottom-commit-diff-partial', {
                        count: commitDiffErrorCount
                      })
                    }
                  : null
          }
          testId="diff-view"
          header={multiFile ? t(translate, 'diff-all-files') : undefined}
          emptyMessage={
            pane.kind === 'working'
              ? t(translate, 'bottom-no-file')
              : commit
                ? t(translate, repo.commitMergeParent ? 'bottom-merge-empty' : 'bottom-no-changes')
                : t(translate, 'bottom-no-commit')
          }
          onCopy={sections.length ? copyDiff : undefined}
        />
      </div>
    </div>
  );
}

function clampRatio(value: number): number {
  return Math.min(0.7, Math.max(0.2, value));
}

function FileList({
  files,
  selected,
  onSelect
}: {
  files: FileChange[];
  selected: FileChange | null;
  onSelect: (file: FileChange) => void;
}) {
  const translate = useStore((state) => state.t);
  if (files.length === 0) {
    return <EmptyState message={t(translate, 'bottom-no-changes')} testId="bottom-files-empty" />;
  }
  return (
    <div className="bottom__files-scroll">
      {files.map((file, index) => {
        const isSelected = selected?.new_path === file.new_path;
        const binary = file.added === null || file.deleted === null;
        return (
          <div
            key={`${file.status}-${file.new_path}-${index}`}
            className={`file-row${isSelected ? ' is-selected' : ''}`}
            data-testid={`bottom-file-${file.new_path}`}
            title={file.old_path ? `${file.old_path} → ${file.new_path}` : file.new_path}
            onClick={() => onSelect(file)}
          >
            <span className={`file-row__status status-${statusModifier(file.status)}`}>
              {t(translate, statusKey(file.status))}
            </span>
            <span className="file-row__name mono">{file.new_path}</span>
            {binary ? (
              <span className="file-row__stat muted">{t(translate, 'bottom-bin')}</span>
            ) : (
              <StatBar added={file.added ?? 0} deleted={file.deleted ?? 0} />
            )}
          </div>
        );
      })}
    </div>
  );
}

/**
 * The five-block addition and deletion bar.
 *
 * The numbers are shown, not just the blocks: the blocks say the ratio and the
 * numbers say the size, and a bar alone cannot say whether a change is three
 * lines or three hundred.
 */
function StatBar({ added, deleted, testId }: { added: number; deleted: number; testId?: string }) {
  const blocks = statBlocks(added, deleted);
  if (blocks.added === 0 && blocks.deleted === 0) {
    return <span className="file-row__stat muted">—</span>;
  }
  return (
    <span className="stat-bar" data-testid={testId}>
      <span className="stat-bar__added" data-testid={testId ? `${testId}-added` : undefined}>
        +{added}
      </span>
      <span className="stat-bar__deleted" data-testid={testId ? `${testId}-deleted` : undefined}>
        -{deleted}
      </span>
      <span className="stat-blocks">
        <span className="stat-blocks__added" style={{ flex: blocks.added }} />
        <span className="stat-blocks__deleted" style={{ flex: blocks.deleted }} />
      </span>
    </span>
  );
}
