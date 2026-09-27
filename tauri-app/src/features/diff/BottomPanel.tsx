/**
 * The bottom panel: the changed-file list on the left and the diff on the right.
 *
 * The two are split by a ratio the user can drag, and the header names what is
 * displayed so a stale viewer can never be mistaken for current content. The
 * list shows either the files of the selected commit or the working-tree file
 * the changes panel selected.
 */

import { useState } from "react";

import { EmptyState, Splitter, Spinner } from "../../components/controls";
import { Icon } from "../../components/Icon";
import type { FileChange } from "../../bridge/types";
import { statBlocks, statusKey } from "./fileMeta";
import { useStore, type RepoState } from "../../app/store";
import { DiffView } from "./DiffView";
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
  const selectCommitFile = useStore((state) => state.selectCommitFile);
  const clearCommit = useStore((state) => state.clearCommit);
  const [collapsed, setCollapsed] = useState(false);

  const pane = repo.pane;
  const commitSelected = repo.selected;
  const files = pane.kind === "commit" ? repo.commitFiles : [];

  const header = (() => {
    if (pane.kind === "working") {
      return t(
        translate,
        pane.staged ? "diff-working-tree-staged" : "diff-working-tree-changes",
      );
    }
    if (commitSelected) {
      return commitSelected.subject;
    }
    return t(translate, "bottom-no-commit");
  })();

  const document =
    pane.kind === "commit" ? repo.commitDocument : repo.workingDocument;
  const ranges =
    document && pane.kind === "commit"
      ? { old: document.inline_old, new: document.inline_new }
      : null;

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
        <span className="bottom__toolbar-title" title={header}>
          {header}
        </span>
        <span className="bottom__toolbar-spacer" />
        {commitSelected ? (
          <span className="muted">{commitSelected.short}</span>
        ) : null}
        {pane.kind === "working" && repo.workingLoading ? <Spinner size={11} /> : null}
        {commitSelected ? (
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
      <div className="bottom__body">
        {collapsed ? null : pane.kind === "commit" ? (
          <>
            <div
              className="bottom__files"
              style={{ width: "25%", minWidth: 120 }}
              data-testid="bottom-file-list"
            >
              <FileList
                files={files}
                selected={pane.file}
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
                // The panel is measured in pixels, so the ratio is recomputed
                // from the delta against the current width.
                onFileListRatioChange(clampRatio(0.25 + delta / 600));
              }}
            />
          </>
        ) : null}
        <DiffView
          document={document}
          layout={layout}
          ranges={ranges}
          loading={pane.kind === "working" ? repo.workingLoading : false}
          error={pane.kind === "working" ? repo.workingError : null}
          testId="diff-view"
          emptyMessage={
            pane.kind === "working"
              ? t(translate, "bottom-no-file")
              : t(translate, "bottom-no-changes")
          }
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
    <div style={{ overflowY: "auto", height: "100%" }}>
      {files.map((file) => {
        const blocks = statBlocks(file.added, file.deleted);
        const isSelected = selected?.new_path === file.new_path;
        return (
          <div
            key={`${file.status}-${file.new_path}`}
            className={`file-row${isSelected ? " is-selected" : ""}`}
            data-testid={`bottom-file-${file.new_path}`}
            title={file.path}
            onClick={() => onSelect(file)}
          >
            <span className={`file-row__status status-${statusKey(file.status)}`}>
              {t(translate, fileStatusKey(file.status))}
            </span>
            <span className="file-row__name">{file.path}</span>
            {blocks.added || blocks.deleted ? (
              <span className="stat-blocks" title={`+${file.added ?? 0} -${file.deleted ?? 0}`}>
                <span className="stat-blocks__added" style={{ flex: blocks.added }} />
                <span className="stat-blocks__deleted" style={{ flex: blocks.deleted }} />
              </span>
            ) : (
              <span className="file-row__stat muted">—</span>
            )}
          </div>
        );
      })}
    </div>
  );
}

function fileStatusKey(status: FileChange["status"]): string {
  switch (status) {
    case "added":
      return "status-add";
    case "deleted":
      return "status-del";
    case "modified":
      return "status-mod";
    case "renamed":
      return "status-ren";
    case "copied":
      return "status-cpy";
    case "unmerged":
      return "status-conflict";
    default:
      return "status-unknown";
  }
}
