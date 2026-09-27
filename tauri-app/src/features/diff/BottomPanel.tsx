/**
 * The bottom panel: the changed-file list on the left and the diff on the right.
 *
 * Selecting a commit shows every file it changed at once, which is what the
 * reference application does; choosing a file narrows the view to that one. The
 * header always names what is displayed, so a stale panel cannot be mistaken for
 * current content.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { EmptyState, Splitter } from "../../components/controls";
import { Icon } from "../../components/Icon";
import type { FileChange } from "../../bridge/types";
import * as ipc from "../../bridge/ipc";
import { statBlocks, statusKey, statusModifier } from "./fileMeta";
import { useStore, type RepoState } from "../../app/store";
import { DiffView, NARROW_WIDTH, type DiffSection } from "./DiffView";
import { t } from "../../i18n/strings";

export function BottomPanel({
  repo,
  height,
  onFileListRatioChange,
}: {
  repo: RepoState;
  height: number | null;
  onFileListRatioChange: (ratio: number) => void;
}) {
  const translate = useStore((state) => state.t);
  const layout = useStore((state) => state.config.view.diff_layout);
  const ratio = useStore((state) => state.workspace.layout.file_list_ratio);
  const selectCommitFile = useStore((state) => state.selectCommitFile);
  const clearCommit = useStore((state) => state.clearCommit);
  const [collapsed, setCollapsed] = useState(false);
  const [width, setWidth] = useState(1000);
  const bodyRef = useRef<HTMLDivElement>(null);
  const requested = useRef(new Set<string>());

  const pane = repo.pane;
  const commit = repo.selected;
  const showFileList = pane.kind === "commit" && repo.commitFiles.length > 0;

  // Every file of the selected commit is loaded, because the panel shows them
  // all. The requested set keeps a re-render from asking for the same file
  // twice, and it resets when the selection changes.
  const selectionKey = commit?.oid ?? "";
  useEffect(() => {
    requested.current = new Set();
  }, [selectionKey]);
  useEffect(() => {
    if (!commit) {
      return;
    }
    for (const file of repo.commitFiles) {
      if (repo.commitDiffs[file.new_path] || requested.current.has(file.new_path)) {
        continue;
      }
      requested.current.add(file.new_path);
      void ipc.loadCommitFileDiff(
        repo.id,
        commit.oid,
        repo.commitMergeParent,
        file,
      );
    }
  }, [repo.id, commit, repo.commitFiles, repo.commitDiffs]);

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
    if (pane.kind === "commit" && pane.file) {
      const document = repo.commitDiffs[pane.file.new_path];
      return document ? [{ path: pane.file.new_path, document }] : [];
    }
    if (pane.kind === "commit") {
      return repo.commitFiles
        .map((file) => {
          const document = repo.commitDiffs[file.new_path];
          return document ? { path: file.new_path, document } : null;
        })
        .filter((entry): entry is DiffSection => entry !== null);
    }
    if (pane.kind === "working" && repo.workingDocument) {
      return [{ path: pane.file.path, document: repo.workingDocument }];
    }
    return [];
  }, [pane, repo.commitFiles, repo.commitDiffs, repo.workingDocument]);

  const title = (() => {
    if (pane.kind === "working") {
      return t(
        translate,
        pane.staged ? "diff-working-tree-staged" : "diff-working-tree-changes",
      );
    }
    if (commit) {
      return commit.subject;
    }
    return t(translate, "bottom-no-commit");
  })();

  // A wide view of many files forces the inline layout, because a side-by-side
  // layout of every file at once leaves each pane too narrow to read.
  const multiFile = sections.length > 1;
  const narrow = width < NARROW_WIDTH;

  // The commit's own totals, so the size of the change is readable without
  // summing the file list.
  const commitTotals =
    pane.kind === "commit" && repo.commitFiles.length
      ? repo.commitFiles.reduce(
          (sum, file) => ({
            added: sum.added + (file.added ?? 0),
            deleted: sum.deleted + (file.deleted ?? 0),
          }),
          { added: 0, deleted: 0 },
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
      sections.map((entry) => `diff -- ${entry.path}\n${entry.document.copy_text}`).join(""),
    );
  };

  // The reference binds the secondary modifier with `c` in the diff area, which
  // is the gesture people reach for.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key.toLowerCase() === "c" &&
        sections.length > 0
      ) {
        copyDiff();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [sections, repo.id]);

  return (
    <div
      className={`bottom${height !== null ? " bottom--fixed" : ""}`}
      style={height !== null ? { height } : undefined}
      data-testid="bottom-panel"
    >
      <div className="bottom__toolbar">
        <button
          type="button"
          className="tool-button tool-button--compact"
          data-testid="bottom-toggle-files"
          onClick={() => setCollapsed((value) => !value)}
        >
          <Icon name={collapsed ? "chevron-right" : "chevron-down"} size={11} />
        </button>
        <span className="bottom__toolbar-title" title={title}>
          {title}
        </span>
        <span className="bottom__toolbar-spacer" />
        {repo.commitMergeParent && pane.kind === "commit" ? (
          <span className="muted">{t(translate, "diff-merge-first-parent")}</span>
        ) : null}
        {commit ? <span className="mono muted">{commit.short}</span> : null}
        {/* The commit's own totals, so the size of the change is readable
            without summing the file list. */}
        {pane.kind === "commit" && commitTotals ? (
          <StatBar
            added={commitTotals.added}
            deleted={commitTotals.deleted}
            testId="bottom-commit-stat"
          />
        ) : null}
        {/* The working-tree view has no commit, so it names the file and offers
            the copy, as the reference does. */}
        {pane.kind === "working" && pane.file ? (
          <span
            className="bottom__toolbar-title mono"
            data-testid="bottom-working-path"
            title={pane.file.path}
          >
            {pane.file.path}
          </span>
        ) : null}
        {sections.length ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid="bottom-copy-diff"
            title={t(translate, "diff-copy")}
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
            onClick={() => clearCommit(repo.id)}
          >
            {t(translate, "bottom-no-commit")}
          </button>
        ) : null}
      </div>
      <div className="bottom__body" ref={bodyRef}>
        {collapsed || !showFileList ? null : (
          <>
            <div
              className="bottom__files"
              style={{ width: `${ratio * 100}%` }}
              data-testid="bottom-file-list"
            >
              <FileList
                files={repo.commitFiles}
                selected={pane.kind === "commit" ? pane.file : null}
                onSelect={(file) => {
                  void selectCommitFile(repo.id, file);
                }}
              />
            </div>
            <Splitter
              orientation="vertical"
              label="resize file list"
              testId="bottom-file-splitter"
              onDrag={(delta) => {
                const total = Math.max(1, bodyRef.current?.clientWidth ?? 600);
                onFileListRatioChange(clampRatio(ratio + delta / total));
              }}
            />
          </>
        )}
        <DiffView
          sections={sections}
          layout={layout}
          forceInline={narrow}
          loading={pane.kind === "working" ? repo.workingLoading : false}
          error={pane.kind === "working" ? repo.workingError : null}
          testId="diff-view"
          header={multiFile ? t(translate, "diff-all-files") : undefined}
          emptyMessage={
            pane.kind === "working"
              ? t(translate, "bottom-no-file")
              : commit
                ? t(
                    translate,
                    repo.commitMergeParent ? "bottom-merge-empty" : "bottom-no-changes",
                  )
                : t(translate, "bottom-no-commit")
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
  onSelect,
}: {
  files: FileChange[];
  selected: FileChange | null;
  onSelect: (file: FileChange) => void;
}) {
  const translate = useStore((state) => state.t);
  if (files.length === 0) {
    return (
      <EmptyState
        message={t(translate, "bottom-no-changes")}
        testId="bottom-files-empty"
      />
    );
  }
  return (
    <div className="bottom__files-scroll">
      {files.map((file, index) => {
        const isSelected = selected?.new_path === file.new_path;
        const binary = file.added === null || file.deleted === null;
        return (
          <div
            key={`${file.status}-${file.new_path}-${index}`}
            className={`file-row${isSelected ? " is-selected" : ""}`}
            data-testid={`bottom-file-${file.new_path}`}
            title={file.old_path ? `${file.old_path} → ${file.new_path}` : file.new_path}
            onClick={() => onSelect(file)}
          >
            <span
              className={`file-row__status status-${statusModifier(file.status)}`}
            >
              {t(translate, statusKey(file.status))}
            </span>
            <span className="file-row__name mono">{file.new_path}</span>
            {binary ? (
              <span className="file-row__stat muted">
                {t(translate, "bottom-bin")}
              </span>
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
function StatBar({
  added,
  deleted,
  testId,
}: {
  added: number;
  deleted: number;
  testId?: string;
}) {
  const blocks = statBlocks(added, deleted);
  if (blocks.added === 0 && blocks.deleted === 0) {
    return <span className="file-row__stat muted">—</span>;
  }
  return (
    <span className="stat-bar" data-testid={testId}>
      <span className="stat-bar__added" data-testid={testId ? `${testId}-added` : undefined}>
        +{added}
      </span>
      <span
        className="stat-bar__deleted"
        data-testid={testId ? `${testId}-deleted` : undefined}
      >
        -{deleted}
      </span>
      <span className="stat-blocks">
        <span className="stat-blocks__added" style={{ flex: blocks.added }} />
        <span className="stat-blocks__deleted" style={{ flex: blocks.deleted }} />
      </span>
    </span>
  );
}
