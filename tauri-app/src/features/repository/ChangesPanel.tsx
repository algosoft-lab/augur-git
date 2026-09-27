/**
 * The working-tree panel: staged, unstaged, and conflicted files.
 *
 * Files are grouped the way Git reports them, and each group can be staged,
 * unstaged, or discarded as a whole. Row actions are revealed on hover, and a
 * discard always goes through the confirmation overlay because it destroys work.
 */

import { useState } from "react";

import { Icon } from "../../components/Icon";
import { ContextMenu, IconButton } from "../../components/controls";
import type { FileStatus, WorkingTreeAction } from "../../bridge/types";
import * as ipc from "../../bridge/ipc";
import {
  codeFor,
  isConflicted,
  isStaged,
  isUntracked,
  useStore,
  type RepoState,
} from "../../app/store";
import { t } from "../../i18n/strings";

interface Group {
  key: "staged" | "unstaged" | "conflicts";
  files: FileStatus[];
}

export function ChangesPanel({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const showUntracked = useStore((state) => state.config.view.show_untracked);
  const pane = repo.pane;
  const selectWorkingFile = useStore((state) => state.selectWorkingFile);
  const openOverlay = useStore((state) => state.openOverlay);
  const setMessage = useStore((state) => state.setMessage);
  const [collapsed, setCollapsed] = useState<string[]>([]);

  const blocked = repo.busy;
  const groups: Group[] = [];
  const conflicts = repo.files.filter((file) => isConflicted(file));
  const staged = repo.files.filter((file) => isStaged(file));
  const unstaged = repo.files.filter(
    (file) =>
      !isConflicted(file) &&
      !isStaged(file) &&
      (showUntracked || !isUntracked(file)),
  );
  if (conflicts.length) {
    groups.push({ key: "conflicts", files: conflicts });
  }
  if (staged.length) {
    groups.push({ key: "staged", files: staged });
  }
  if (unstaged.length) {
    groups.push({ key: "unstaged", files: unstaged });
  }

  const toggle = (key: string) => {
    setCollapsed((current) =>
      current.includes(key)
        ? current.filter((item) => item !== key)
        : [...current, key],
    );
  };

  const operate = async (action: WorkingTreeAction, files: FileStatus[], all: boolean) => {
    if (blocked || files.length === 0) {
      return;
    }
    if (action === "discard") {
      const tracked = files.filter((file) => !isUntracked(file)).length;
      const untracked = files.length - tracked;
      openOverlay({
        kind: "discard",
        scope: { kind: "workingTree", staged: false, all },
        trackedCount: tracked,
        untrackedCount: untracked,
      });
      return;
    }
    try {
      const requestId = await ipc.workingTreeOperation(repo.id, action, files, all);
      useStore.getState().setBusy(repo.id, true);
      void requestId;
    } catch (error) {
      setMessage(repo.id, ipc.describeError(error).detail, false);
    }
  };

  if (repo.files.length === 0) {
    return (
      <div className="changes" data-testid="changes-empty">
        <div className="empty-state" style={{ minHeight: 120 }}>
          <span className="empty-state__message">{t(translate, "changes-empty")}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="changes" data-testid="changes-panel">
      {groups.map((group) => {
        const isCollapsed = collapsed.includes(group.key);
        const stagedGroup = group.key === "staged";
        return (
          <div key={group.key}>
            <div className="changes__section-header changes__row-group">
              <button
                type="button"
                className="changes__section-header"
                style={{ flex: 1 }}
                aria-expanded={!isCollapsed}
                onClick={() => toggle(group.key)}
                data-testid={`changes-toggle-${group.key}`}
              >
                <Icon
                  name={isCollapsed ? "chevron-right" : "chevron-down"}
                  size={12}
                />
                <span className="changes__section-title">
                  {t(translate, `section-${group.key === "conflicts" ? "changes" : group.key}`)}
                </span>
                <span className="changes__section-count">{group.files.length}</span>
              </button>
              {stagedGroup ? (
                <IconButton
                  icon={<Icon name="minus" size={12} />}
                  tooltip={t(translate, "changes-unstage-all")}
                  disabled={blocked || group.files.length === 0}
                  onClick={() => void operate("unstage", group.files, true)}
                  testId="changes-unstage-all"
                />
              ) : (
                <>
                  <IconButton
                    icon={<Icon name="undo" size={12} />}
                    tooltip={t(translate, "changes-discard-all")}
                    disabled={
                      blocked || group.files.length === 0 || repo.hasConflicts
                    }
                    onClick={() => void operate("discard", group.files, true)}
                    testId="changes-discard-all"
                  />
                  <IconButton
                    icon={<Icon name="plus" size={12} />}
                    tooltip={t(translate, "changes-stage-all")}
                    disabled={
                      blocked || group.files.length === 0 || repo.hasConflicts
                    }
                    onClick={() => void operate("stage", group.files, true)}
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
                      pane.kind === "working" &&
                      pane.staged === stagedGroup &&
                      pane.file.path === file.path
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
  );
}

function FileRow({
  repo,
  file,
  staged,
  selected,
  onSelect,
  onOperate,
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
  const label = conflicted
    ? t(translate, "status-conflict")
    : t(translate, statusKey(code, untracked));

  const entries = conflicted
    ? []
    : [
        {
          id: "toggle-stage",
          label: staged
            ? t(translate, "changes-unstage")
            : t(translate, "changes-stage"),
          icon: <Icon name={staged ? "minus" : "plus"} size={12} />,
          disabled: repo.busy,
          onSelect: () => onOperate(staged ? "unstage" : "stage"),
        },
        {
          id: "discard",
          label: t(translate, "changes-discard"),
          icon: <Icon name="undo" size={12} />,
          disabled: repo.busy || repo.hasConflicts,
          onSelect: () => onOperate("discard"),
        },
      ];

  return (
    <ContextMenu testId={`changes-row-${file.path}`} entries={entries}>
      <div
        className={`file-row changes__row-group${selected ? " is-selected" : ""}`}
        data-testid={`changes-file-${file.path}`}
        onClick={onSelect}
        title={file.old_path ? `${file.old_path} → ${file.path}` : file.path}
      >
        <span className={`file-row__status status-${statusKey(code, untracked)}`}>
          {label}
        </span>
        <span className="file-row__name">{file.path}</span>
        <IconButton
          icon={
            staged || conflicted ? (
              <Icon name="minus" size={12} />
            ) : (
              <Icon name="plus" size={12} />
            )
          }
          tooltip={staged ? t(translate, "changes-unstage") : t(translate, "changes-stage")}
          disabled={repo.busy || (conflicted && false)}
          onClick={() => onOperate(staged ? "unstage" : "stage")}
          testId={`changes-toggle-${file.path}`}
        />
      </div>
    </ContextMenu>
  );
}

/** Map a porcelain status character to its i18n key. */
export function statusKey(code: string, untracked: boolean): string {
  if (untracked) {
    return "status-unknown";
  }
  switch (code) {
    case "A":
      return "status-add";
    case "D":
      return "status-del";
    case "M":
      return "status-mod";
    case "R":
      return "status-ren";
    case "C":
      return "status-cpy";
    case "U":
      return "status-conflict";
    case "?":
      return "status-unknown";
    default:
      return "status-unknown";
  }
}
