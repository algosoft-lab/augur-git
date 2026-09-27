/**
 * The standalone revision comparison window.
 *
 * Two endpoints are chosen from branches, tags, or a manually typed object id.
 * Selecting a new pair starts a comparison and cancels the previous one, so a
 * slow answer can never replace a newer result. Files load one at a time, and
 * the export writes the full patch with a native save dialog.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { save } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { Icon } from "../../components/Icon";
import { EmptyState, Spinner, TextInput } from "../../components/controls";
import * as ipc from "../../bridge/ipc";
import type { CompareRevision, DiffPayload, FileChange } from "../../bridge/types";
import { useStore, type RepoState } from "../../app/store";
import { statBlocks, statusKey, statusModifier } from "../diff/fileMeta";
import { DiffView } from "../diff/DiffView";
import { t, ta } from "../../i18n/strings";

interface Endpoint {
  input: string;
  selected: CompareRevision | null;
}

export function CompareWindow({ repoId }: { repoId: number | null }) {
  const translate = useStore((state) => state.t);
  const repo = useStore<RepoState | undefined>(
    (state) => (repoId ? state.repos[repoId] : undefined),
  );
  const diffLayout = useStore((state) => state.config.view.diff_layout);

  const [base, setBase] = useState<Endpoint>({ input: "", selected: null });
  const [target, setTarget] = useState<Endpoint>({ input: "", selected: null });
  const [requestId, setRequestId] = useState(0);
  const [files, setFiles] = useState<FileChange[]>([]);
  const [selected, setSelected] = useState<FileChange | null>(null);
  const [documents, setDocuments] = useState<Record<string, DiffPayload>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [finished, setFinished] = useState(false);
  const [exportState, setExportState] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(true);
  const requested = useRef(new Set<number>());

  // Compare events carry the request id, so a stale answer is dropped here.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    if (repoId === null) {
      return;
    }
    void ipc.onRepoEvent((event) => {
      if (event.repoId !== repoId) {
        return;
      }
      switch (event.type) {
        case "branchCompareFiles":
          if (event.requestId !== requestId) {
            return;
          }
          setFiles(event.files);
          setLoading(true);
          setFinished(false);
          setDocuments({});
          setErrors({});
          setSelected(null);
          break;
        case "branchCompareFileDiff":
          if (event.requestId !== requestId) {
            return;
          }
          setDocuments((current) => ({
            ...current,
            [event.file.new_path]: event.document,
          }));
          setErrors((current) => {
            const next = { ...current };
            delete next[event.file.new_path];
            return next;
          });
          setSelected((current) =>
            current === null ? event.file : current,
          );
          break;
        case "branchCompareError":
          if (event.requestId !== requestId) {
            return;
          }
          if (event.file) {
            setErrors((current) => ({
              ...current,
              [event.file!.new_path]: event.detail,
            }));
          } else {
            setErrors((current) => ({ ...current, "": event.detail }));
          }
          break;
        case "branchCompareFinished":
          if (event.requestId !== requestId) {
            return;
          }
          setLoading(false);
          setFinished(true);
          break;
        case "branchComparePatchExported":
          if (event.requestId !== requestId) {
            return;
          }
          setExportState(
            ta(translate, "branch-compare-export-success", {
              path: event.destination,
            }),
          );
          break;
        case "branchComparePatchError":
          if (event.requestId !== requestId) {
            return;
          }
          setExportState(
            ta(translate, "branch-compare-export-error", { error: event.detail }),
          );
          break;
        default:
          break;
      }
    }).then((stop) => {
      unlisten = stop;
    });
    return () => unlisten?.();
  }, [repoId, requestId, translate]);

  // Close the window's request when it goes away.
  useEffect(() => {
    return () => {
      if (repoId !== null) {
        void ipc.cancelCompare(repoId);
      }
    };
  }, [repoId]);

  const run = useCallback(
    async (nextBase: Endpoint, nextTarget: Endpoint) => {
      if (repoId === null) {
        return;
      }
      const left = nextBase.selected ?? fromManualInput(nextBase.input);
      const right = nextTarget.selected ?? fromManualInput(nextTarget.input);
      if (!left || !right) {
        return;
      }
      requested.current.clear();
      setExportState(null);
      const id = await ipc.startCompare(repoId, left, right);
      setRequestId(id);
    },
    [repoId],
  );

  const visibleFiles = useMemo(
    () => (showAll ? files : files.slice(0, 1)),
    [files, showAll],
  );

  if (repoId === null || !repo) {
    return (
      <div className="app">
        <EmptyState message={t(translate, "err-repo-closed")} />
      </div>
    );
  }

  const canRun = (endpoint: Endpoint) =>
    endpoint.selected !== null || fromManualInput(endpoint.input) !== null;

  return (
    <div className="compare" data-testid="compare-window">
      <div className="compare__header" data-tauri-drag-region>
        <RevisionPicker
          label={t(translate, "branch-compare-base")}
          endpoint={base}
          options={repo.refs.comparison_revisions}
          onChange={(next) => {
            setBase(next);
            void run(next, target);
          }}
        />
        <button
          type="button"
          className="tool-button tool-button--compact"
          data-testid="compare-swap"
          title={t(translate, "branch-compare-run")}
          onClick={() => {
            const nextBase = target;
            const nextTarget = base;
            setBase(nextBase);
            setTarget(nextTarget);
            void run(nextBase, nextTarget);
          }}
        >
          ⇄
        </button>
        <RevisionPicker
          label={t(translate, "branch-compare-target")}
          endpoint={target}
          options={repo.refs.comparison_revisions}
          onChange={(next) => {
            setTarget(next);
            void run(base, next);
          }}
        />
        <button
          type="button"
          className="tool-button tool-button--primary"
          disabled={!canRun(base) || !canRun(target)}
          data-testid="compare-run"
          onClick={() => void run(base, target)}
        >
          {t(translate, "branch-compare-run")}
        </button>
        <button
          type="button"
          className="tool-button"
          disabled={!base.selected || !target.selected}
          data-testid="compare-export"
          onClick={async () => {
            const left = base.selected;
            const right = target.selected;
            if (repoId === null || !left || !right) {
              return;
            }
            const suggested = `${left.name}...${right.name}.patch`;
            const destination = await save({
              defaultPath: suggested,
              filters: [{ name: "Patch", extensions: ["patch", "diff"] }],
            });
            if (typeof destination !== "string") {
              return;
            }
            const id = await ipc.exportPatch(repoId, left, right, destination);
            setRequestId(id);
            setExportState(t(translate, "branch-compare-export-saving"));
          }}
        >
          {t(translate, "branch-compare-export-patch")}
        </button>
      </div>
      <div className="compare__body">
        <div
          className="bottom__files"
          style={{ width: "25%", minWidth: 140 }}
          data-testid="compare-file-list"
        >
          <div className="bottom__toolbar">
            <button
              type="button"
              className="tool-button tool-button--compact"
              data-testid="compare-all-files"
              onClick={() => setShowAll((value) => !value)}
            >
              {t(translate, "branch-compare-all-files")}
            </button>
          </div>
          <div style={{ overflowY: "auto" }}>
            {visibleFiles.length === 0 ? (
              <EmptyState
                message={
                  loading
                    ? t(translate, "branch-compare-loading")
                    : t(translate, "branch-compare-select-hint")
                }
                testId="compare-files-empty"
              />
            ) : (
              visibleFiles.map((file) => {
                const blocks = statBlocks(file.added, file.deleted);
                return (
                  <div
                    key={`${file.status}-${file.new_path}`}
                    className={`file-row${selected?.new_path === file.new_path ? " is-selected" : ""}`}
                    data-testid={`compare-file-${file.new_path}`}
                    title={file.path}
                    onClick={() => {
                      setSelected(file);
                      if (repoId !== null) {
                        const left = base.selected ?? fromManualInput(base.input);
                        const right = target.selected ?? fromManualInput(target.input);
                        if (left && right) {
                          void ipc.startCompare(repoId, left, right);
                        }
                      }
                    }}
                  >
                    <span
                      className={`file-row__status status-${statusModifier(file.status)}`}
                    >
                      {t(translate, statusKey(file.status))}
                    </span>
                    <span className="file-row__name">{file.path}</span>
                    <span className="stat-blocks">
                      <span className="stat-blocks__added" style={{ flex: blocks.added }} />
                      <span className="stat-blocks__deleted" style={{ flex: blocks.deleted }} />
                    </span>
                  </div>
                );
              })
            )}
          </div>
        </div>
        <div style={{ display: "flex", minWidth: 0, flex: 1, flexDirection: "column" }}>
          <div className="bottom__toolbar">
            {loading ? (
              <span className="compare__status">
                <Spinner size={11} /> {t(translate, "branch-compare-loading")}
              </span>
            ) : null}
            {errors[""] ? (
              <span className="compare__status compare__status--error">
                {errors[""]}
              </span>
            ) : null}
            {finished && files.length === 0 ? (
              <span className="compare__status">
                {t(translate, "branch-compare-no-changes")}
              </span>
            ) : null}
            <span className="bottom__toolbar-spacer" />
            {exportState ? (
              <span className="compare__status" data-testid="compare-export-status">
                {exportState}
              </span>
            ) : null}
            <button
              type="button"
              className="tool-button tool-button--compact"
              data-testid="compare-copy"
              onClick={() => {
                const document = selected ? documents[selected.new_path] : undefined;
                if (document) {
                  void writeText(document.copy_text);
                }
              }}
            >
              {t(translate, "context-copied")}
            </button>
          </div>
          <DiffView
            sections={
              selected && documents[selected.new_path]
                ? [{ path: selected.new_path, document: documents[selected.new_path]! }]
                : []
            }
            layout={diffLayout}
            loading={loading && selected === null}
            error={selected ? (errors[selected.new_path] ?? null) : null}
            testId="compare-diff"
            emptyMessage={t(translate, "branch-compare-select-file")}
          />
        </div>
      </div>
    </div>
  );
}

/** Turn typed text into a revision, accepting a 7 to 64 digit object id. */
function fromManualInput(input: string): CompareRevision | null {
  const text = input.trim();
  if (text.length < 7 || text.length > 64) {
    return null;
  }
  if (!/^[0-9a-fA-F]+$/.test(text)) {
    return null;
  }
  return { name: text, full_name: text, kind: "commit" };
}

function RevisionPicker({
  label,
  endpoint,
  options,
  onChange,
}: {
  label: string;
  endpoint: Endpoint;
  options: CompareRevision[];
  onChange: (next: Endpoint) => void;
}) {
  const translate = useStore((state) => state.t);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const query = endpoint.input;
  const filtered = options.filter((option) => {
    if (!query) {
      return true;
    }
    const needle = query.toLowerCase();
    return (
      option.name.toLowerCase().includes(needle) ||
      option.full_name.toLowerCase().includes(needle)
    );
  });
  const manual = fromManualInput(query);
  const invalid = query.length > 0 && !manual && !endpoint.selected && filtered.length === 0;

  // The list is a popup, so a click elsewhere or an Escape dismisses it.
  useEffect(() => {
    if (!open) {
      return;
    }
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="compare__picker" ref={rootRef}>
      <span className="compare__picker-label">{label}</span>
      <div className="compare__picker-input">
        <TextInput
          value={query}
          size="small"
          monospace
          testId={`compare-input-${label}`}
          onChange={(value) => onChange({ input: value, selected: null })}
          onSubmit={() => setOpen(true)}
          onEscape={() => setOpen(false)}
          placeholder={t(translate, "branch-compare-revision-placeholder")}
        />
        {open ? (
          <div className="compare__picker-options">
            {filtered.length === 0 && !manual ? (
              <div className="compare__picker-section">
                {t(translate, "branch-compare-no-matches")}
              </div>
            ) : null}
            {(["local", "remote", "tag", "commit"] as const).map((kind) => {
              const group = filtered.filter((option) => option.kind === kind);
              if (group.length === 0) {
                return null;
              }
              return (
                <div key={kind}>
                  <div className="compare__picker-section">
                    {t(
                      translate,
                      kind === "local"
                        ? "branch-compare-branches"
                        : kind === "remote"
                          ? "branch-compare-remote"
                          : kind === "tag"
                            ? "branch-compare-tags"
                            : "branch-compare-commits",
                    )}
                  </div>
                  {group.map((option) => (
                    <button
                      key={option.full_name}
                      type="button"
                      className="compare__picker-option"
                      data-testid={`compare-option-${option.kind}-${option.full_name}`}
                      onClick={() => {
                        onChange({ input: option.name, selected: option });
                        setOpen(false);
                      }}
                    >
                      <Icon name="git-branch" size={11} />
                      {option.name}
                    </button>
                  ))}
                </div>
              );
            })}
            {manual ? (
              <button
                type="button"
                className="compare__picker-option"
                data-testid="compare-use-commit"
                onClick={() => {
                  onChange({ input: manual.name, selected: manual });
                  setOpen(false);
                }}
              >
                <Icon name="git-commit-horizontal" size={11} />
                {t(translate, "branch-compare-use-commit")} {manual.name}
              </button>
            ) : null}
            {invalid ? (
              <div className="compare__picker-error">
                {t(translate, "branch-compare-invalid-revision")}
              </div>
            ) : null}
          </div>
        ) : null}
        <button
          type="button"
          className="tool-button tool-button--compact"
          data-testid={`compare-toggle-${label}`}
          onClick={() => setOpen((value) => !value)}
        >
          <Icon name="chevron-down" size={11} />
        </button>
      </div>
    </div>
  );
}

export { ta };
