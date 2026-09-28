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
import type {
  CompareRevision,
  DiffPayload,
  FileChange,
  RepoEventEnvelope,
} from "../../bridge/types";
import { useStore, type RepoState } from "../../app/store";
import { statBlocks, statusKey, statusModifier } from "../diff/fileMeta";
import { DiffView, type DiffSection } from "../diff/DiffView";
import { t, ta } from "../../i18n/strings";
import { isRevisionUnavailable } from "./revisions";
import { IS_MACOS, WindowControls } from "../shell/WindowControls";
import { handleTitleBarMouseDown } from "../shell/titleBarDrag";

interface Endpoint {
  input: string;
  selected: CompareRevision | null;
}

type CompareEvent = Extract<
  RepoEventEnvelope,
  {
    type:
      | "branchCompareFiles"
      | "branchCompareFileDiff"
      | "branchCompareError"
      | "branchCompareFinished"
      | "branchComparePatchExported"
      | "branchComparePatchError";
  }
>;

function isCompareEvent(event: RepoEventEnvelope): event is CompareEvent {
  return event.type.startsWith("branchCompare");
}

function CompareTitleBar({ title }: { title: string }) {
  return (
    <div
      className={`window-titlebar${IS_MACOS ? " window-titlebar--macos" : ""}`}
      onMouseDown={handleTitleBarMouseDown}
    >
      <span className="compare__title" data-testid="compare-title">
        {title}
      </span>
      <div
        className="window-titlebar__drag"
        {...(IS_MACOS ? { "data-tauri-drag-region": true } : {})}
      />
      <WindowControls />
    </div>
  );
}

export function CompareWindow({ repoId }: { repoId: number | null }) {
  const translate = useStore((state) => state.t);
  const repo = useStore<RepoState | undefined>(
    (state) => (repoId ? state.repos[repoId] : undefined),
  );
  const diffLayout = useStore((state) => state.config.view.diff_layout);

  const [base, setBase] = useState<Endpoint>({ input: "", selected: null });
  const [target, setTarget] = useState<Endpoint>({ input: "", selected: null });
  /**
   * The comparison this window is waiting for.
   *
   * A ref rather than state: the backend's first answer can arrive before the
   * `await` that yields the id has committed, and a comparison whose file list
   * was dropped would sit empty with no way to tell why.
   */
  const requestId = useRef(0);
  const [files, setFiles] = useState<FileChange[]>([]);
  const [selected, setSelected] = useState<FileChange | null>(null);
  const [documents, setDocuments] = useState<Record<string, DiffPayload>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [finished, setFinished] = useState(false);
  const [exportState, setExportState] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(true);
  const activeGeneration = useRef(0);
  const pendingGeneration = useRef<number | null>(null);
  const bufferedEvents = useRef<CompareEvent[]>([]);
  const consumeEvent = useRef<(event: CompareEvent) => void>(() => {});
  const subscriptionReady = useRef<Promise<void>>(Promise.resolve());
  const subscriptionGeneration = useRef(0);
  const translateRef = useRef(translate);
  translateRef.current = translate;

  // Compare events are buffered while the command reply assigns the request id.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    if (repoId === null) {
      return;
    }
    const generation = ++subscriptionGeneration.current;
    let resolveReady: () => void = () => {};
    let rejectReady: (error: unknown) => void = () => {};
    subscriptionReady.current = new Promise((resolve, reject) => {
      resolveReady = resolve;
      rejectReady = reject;
    });
    consumeEvent.current = (event) => {
      switch (event.type) {
        case "branchCompareFiles":
          if (event.requestId !== requestId.current) {
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
          if (event.requestId !== requestId.current) {
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
          setSelected((current) => current ?? event.file);
          break;
        case "branchCompareError":
          if (event.requestId !== requestId.current) {
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
          if (event.requestId !== requestId.current) {
            return;
          }
          setLoading(false);
          setFinished(true);
          break;
        case "branchComparePatchExported":
          if (event.requestId === requestId.current) {
            setExportState(
              ta(translateRef.current, "branch-compare-export-success", {
                path: event.destination,
              }),
            );
          }
          break;
        case "branchComparePatchError":
          if (event.requestId === requestId.current) {
            setExportState(
              ta(translateRef.current, "branch-compare-export-error", { detail: event.detail }),
            );
          }
          break;
      }
    };
    void ipc.onRepoEvent((event) => {
      if (event.repoId !== repoId || !isCompareEvent(event)) {
        return;
      }
      if (event.requestId === requestId.current) {
        consumeEvent.current(event);
      } else if (pendingGeneration.current !== null) {
        bufferedEvents.current = [...bufferedEvents.current, event].slice(-256);
      }
    }).then((stop) => {
      if (subscriptionGeneration.current !== generation) {
        stop();
        return;
      }
      unlisten = stop;
      resolveReady();
    }).catch((error: unknown) => {
      if (subscriptionGeneration.current === generation) {
        rejectReady(error);
        setLoading(false);
        setErrors({ "": String(error) });
      } else {
        resolveReady();
      }
    });
    return () => {
      if (subscriptionGeneration.current === generation) {
        subscriptionGeneration.current += 1;
      }
      resolveReady();
      unlisten?.();
    };
  }, [repoId]);

  const trackRequest = useCallback(async (start: () => Promise<number>) => {
    const generation = activeGeneration.current + 1;
    const listenerGeneration = subscriptionGeneration.current;
    activeGeneration.current = generation;
    pendingGeneration.current = generation;
    bufferedEvents.current = [];
    requestId.current = 0;
    await subscriptionReady.current;
    if (
      generation !== activeGeneration.current ||
      listenerGeneration !== subscriptionGeneration.current
    ) {
      return null;
    }
    let id: number;
    try {
      id = await start();
    } catch (error) {
      if (generation !== activeGeneration.current) {
        return null;
      }
      pendingGeneration.current = null;
      bufferedEvents.current = [];
      throw error;
    }
    if (generation !== activeGeneration.current) {
      return null;
    }
    requestId.current = id;
    pendingGeneration.current = null;
    const early = bufferedEvents.current;
    bufferedEvents.current = [];
    for (const event of early) {
      if (event.requestId === id) {
        consumeEvent.current(event);
      }
    }
    return id;
  }, []);

  const run = useCallback(
    async (nextBase: Endpoint, nextTarget: Endpoint) => {
      if (repoId === null) {
        return;
      }
      const left = nextBase.selected ?? fromManualInput(nextBase.input);
      const right = nextTarget.selected ?? fromManualInput(nextTarget.input);
      if (!left || !right) {
        activeGeneration.current += 1;
        pendingGeneration.current = null;
        bufferedEvents.current = [];
        requestId.current = 0;
        setFiles([]);
        setSelected(null);
        setDocuments({});
        setErrors({});
        setLoading(false);
        setFinished(false);
        void ipc.cancelCompare(repoId);
        return;
      }
      setFiles([]);
      setSelected(null);
      setDocuments({});
      setErrors({});
      setLoading(true);
      setFinished(false);
      setExportState(null);
      try {
        await trackRequest(() => ipc.startCompare(repoId, left, right));
      } catch (error) {
        if (pendingGeneration.current !== null) {
          pendingGeneration.current = null;
        }
        setLoading(false);
        setErrors({ "": String(error) });
      }
    },
    [repoId, trackRequest],
  );

  // The aggregate row sits above the list rather than replacing it, so choosing
  // a single file and choosing all of them are the same gesture twice.
  const sections = useMemo<DiffSection[]>(
    () =>
      files
        .map((file) => {
          const document = documents[file.new_path];
          return document ? { path: file.new_path, document } : null;
        })
        .filter((entry): entry is DiffSection => entry !== null),
    [files, documents],
  );

  // The reference pre-selects the current branch as the base and the first
  // other revision as the target, so the window opens with something to read.
  const preset = useRef(false);
  useEffect(() => {
    if (preset.current || repoId === null || !repo) {
      return;
    }
    const values = repo.refs.comparison_revisions;
    const current = values.find(
      (option) => option.kind === "local" && option.name === repo.branch,
    );
    const other =
      values.find((option) => option.full_name !== current?.full_name) ?? values[0];
    if (!current || !other) {
      return;
    }
    const listenerGeneration = subscriptionGeneration.current;
    void subscriptionReady.current.then(() => {
      if (preset.current || listenerGeneration !== subscriptionGeneration.current) {
        return;
      }
      preset.current = true;
      setBase({ input: current.name, selected: current });
      setTarget({ input: other.name, selected: other });
      void run(
        { input: current.name, selected: current },
        { input: other.name, selected: other },
      );
    }).catch((error: unknown) => {
      if (listenerGeneration === subscriptionGeneration.current) {
        setLoading(false);
        setErrors({ "": String(error) });
      }
    });
  }, [repoId, repo, run]);

  const subjects = useMemo(
    () => new Map((repo?.logRows ?? []).map((row) => [row.oid, row.subject])),
    [repo?.logRows],
  );

  // The reference offers the loaded commits alongside the refs, with the recent
  // ones first, so a comparison does not have to be between named branches.
  const offered = useMemo<CompareRevision[]>(
    () => [
      ...(repo?.refs.comparison_revisions ?? []),
      ...(repo?.logRows ?? []).map<CompareRevision>((row) => ({
        name: row.short,
        full_name: row.oid,
        kind: "commit",
      })),
    ],
    [repo?.refs.comparison_revisions, repo?.logRows],
  );

  if (repoId === null || !repo) {
    return (
      <div className="compare" data-testid="compare-window">
        <CompareTitleBar title={t(translate, "branch-compare-title")} />
        <EmptyState message={t(translate, "err-repo-closed")} />
      </div>
    );
  }

  const canRun = (endpoint: Endpoint) =>
    endpoint.selected !== null || fromManualInput(endpoint.input) !== null;

  /**
   * A comparison that failed as a whole, before any file could be listed.
   *
   * The reference reports this state instead of the empty states, because
   * "no changes" would be a lie about a request that never got that far.
   */
  const requestError = errors[""] ?? null;
  const emptyMessage = requestError
    ? t(translate, "branch-compare-error")
    : loading
      ? t(translate, "branch-compare-loading")
      : t(translate, "branch-compare-select-hint");

  return (
    <div className="compare" data-testid="compare-window">
      <CompareTitleBar title={t(translate, "branch-compare-title")} />
      <div className="compare__header">
        <RevisionPicker
          label={t(translate, "branch-compare-base")}
          endpoint={base}
          options={offered}
          subjects={subjects}
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
          options={offered}
          subjects={subjects}
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
          {finished
            ? t(translate, "branch-compare-refresh")
            : t(translate, "branch-compare-run")}
        </button>
        {loading ? (
          // Progress while a large comparison streams in, because a window that
          // looks finished and is not is worse than one that admits it is
          // working.
          <span className="compare__status" data-testid="compare-progress">
            <Spinner size={14} color="var(--base-blue)" />
            {Object.keys(documents).length} / {files.length}
          </span>
        ) : null}
        <div style={{ flex: 1 }} />
        <button
          type="button"
          className="tool-button"
          // A typed object id is a revision like any other, so it exports; what
          // cannot be exported is a comparison that found nothing.
          disabled={
            !canRun(base) ||
            !canRun(target) ||
            loading ||
            (finished && files.length === 0)
          }
          data-testid="compare-export"
          onClick={async () => {
            const left = base.selected ?? fromManualInput(base.input);
            const right = target.selected ?? fromManualInput(target.input);
            if (repoId === null || !left || !right) {
              return;
            }
            const destination = await save({
              defaultPath: suggestedPatchFilename(left, right),
              filters: [{ name: "Patch", extensions: ["patch", "diff"] }],
            });
            if (typeof destination !== "string") {
              return;
            }
            setExportState(t(translate, "branch-compare-export-saving"));
            try {
              await trackRequest(() => ipc.exportPatch(repoId, left, right, destination));
            } catch (error) {
              setExportState(ta(translate, "branch-compare-export-error", { detail: String(error) }));
            }
          }}
        >
          {t(translate, "branch-compare-export-patch")}
        </button>
      </div>
      <div className="compare__body">
        <div
          className="bottom__files"
          style={{ width: "25%", minWidth: 200 }}
          data-testid="compare-file-list"
        >
          <div style={{ overflowY: "auto" }}>
            {files.length === 0 ? (
              <EmptyState
                message={emptyMessage}
                testId="compare-files-empty"
              />
            ) : (
              <>
                <div
                  className={`file-row${showAll ? " is-selected" : ""}`}
                  data-testid="compare-all-files"
                  onClick={() => setShowAll(true)}
                >
                  <span className="file-row__status" />
                  <span className="file-row__name">
                    {t(translate, "branch-compare-all-files")}
                  </span>
                  <span className="file-row__stat muted">{files.length}</span>
                </div>
              {files.map((file) => {
                const blocks = statBlocks(file.added, file.deleted);
                const error = errors[file.new_path];
                return (
                  <div
                    key={`${file.status}-${file.new_path}`}
                    className={`file-row${selected?.new_path === file.new_path ? " is-selected" : ""}`}
                    data-testid={`compare-file-${file.new_path}`}
                    title={file.path}
                    onClick={() => {
                      // Choosing a file narrows the view to it, which also turns
                      // off the aggregate row.
                      setShowAll(false);
                      setSelected(file);
                    }}
                  >
                    <span
                      className={`file-row__status status-${statusModifier(file.status)}`}
                    >
                      {t(translate, statusKey(file.status))}
                    </span>
                    <span className="file-row__name">{file.path}</span>
                    {/* A file that failed to diff looks like one that has simply
                        not loaded yet unless the reason is in the row. */}
                    {error ? (
                      <span className="file-row__stat status-conflict" title={error}>
                        {error.split("\n")[0]}
                      </span>
                    ) : null}
                    <span className="stat-blocks">
                      <span className="stat-blocks__added" style={{ flex: blocks.added }} />
                      <span className="stat-blocks__deleted" style={{ flex: blocks.deleted }} />
                    </span>
                  </div>
                );
              })}
              </>
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
            {requestError ? (
              // The sentence names the failure; Git's own words survive as the
              // hover text rather than replacing it.
              <span
                className="compare__status compare__status--error"
                data-testid="compare-request-error"
                title={requestError}
              >
                {t(translate, "branch-compare-error")}
              </span>
            ) : null}
            {!requestError && finished && files.length === 0 ? (
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
              disabled={!selected || !documents[selected.new_path]}
              title={t(translate, "diff-copy-tooltip")}
              aria-label={t(translate, "diff-copy-tooltip")}
              onClick={() => {
                const document = selected ? documents[selected.new_path] : undefined;
                if (document) {
                  void writeText(document.copy_text).then(() => {
                    setExportState(t(translate, "branch-compare-copy-success"));
                  });
                }
              }}
            >
              <Icon name="copy" size={12} />
            </button>
          </div>
          <DiffView
            // The aggregate row shows every file; choosing a file shows only
            // that one, which is how the reference narrows the same list.
            sections={
              showAll
                ? sections
                : selected && documents[selected.new_path]
                  ? [{ path: selected.new_path, document: documents[selected.new_path]! }]
                  : []
            }
            layout={diffLayout}
            forceInline={sections.length > 1}
            loading={loading && !finished}
            error={selected && !showAll ? (errors[selected.new_path] ?? null) : null}
            testId="compare-diff"
            showFileHeaders
            header={showAll && sections.length > 0 ? t(translate, "branch-compare-all-files") : undefined}
            emptyMessage={
              requestError
                ? t(translate, "branch-compare-error")
                : loading
                  ? t(translate, "branch-compare-loading")
                  : t(translate, "branch-compare-select-file")
            }
            onCopy={
              sections.length
                ? () => {
                    void writeText(
                      sections
                        .map((entry) => `diff -- ${entry.path}
${entry.document.copy_text}`)
                        .join(""),
                    );
                  }
                : undefined
            }
          />
        </div>
      </div>
    </div>
  );
}

/**
 * The default file name for an exported patch.
 *
 * Every character that is not a letter, a digit, a dot, an underscore, or a
 * hyphen becomes one, because a branch name may contain a slash and a path
 * segment cannot. An endpoint whose name sanitises away entirely falls back to a
 * generic name rather than to something like `-.patch`.
 */
export function suggestedPatchFilename(base: CompareRevision, target: CompareRevision): string {
  const sanitize = (value: string) =>
    [...value]
      .map((character) =>
        /[A-Za-z0-9._-]/.test(character) ? character : "-",
      )
      .join("")
      .replace(/^-+|-+$/g, "");
  const left = sanitize(base.name);
  const right = sanitize(target.name);
  if (!left || !right) {
    return "comparison.patch";
  }
  return `${left}-to-${right}.patch`;
}

/** The catalog key naming a revision's kind. */
function kindKey(kind: CompareRevision["kind"]): string {
  switch (kind) {
    case "local":
      return "branch-compare-local";
    case "remote":
      return "branch-compare-remote";
    case "tag":
      return "branch-compare-tag";
    default:
      return "branch-compare-commit";
  }
}

/**
 * The suggestion label the reference builds: a kind prefix, the name, and the
 * commit subject when there is one, so a commit is recognisable in the list.
 */
function optionLabel(
  option: CompareRevision,
  subject: string | undefined,
  translate: (key: string) => string,
): string {
  const prefix = translate(kindKey(option.kind));
  return subject
    ? `${prefix} · ${option.name} · ${subject}`
    : `${prefix} · ${option.name}`;
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
  subjects,
  onChange,
}: {
  label: string;
  endpoint: Endpoint;
  options: CompareRevision[];
  /** Commit subjects by object id, so a commit is recognisable in the list. */
  subjects: Map<string, string>;
  onChange: (next: Endpoint) => void;
}) {
  const subjectFor = (option: CompareRevision) => subjects.get(option.full_name);
  const translate = useStore((state) => state.t);
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  // Free-form entry: the suggestions are suppressed while it is on.
  const [manualOnly, setManualOnly] = useState(false);
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
  const unavailable = isRevisionUnavailable(endpoint.selected, options);

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
    document.addEventListener("mousedown", onPointerDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
    };
  }, [open]);

  // The highlight follows what is typed, so it is never past the end.
  useEffect(() => {
    setHighlighted(0);
  }, [query]);

  /**
   * The picker's own keys.
   *
   * On the root rather than on the document, so the behaviour is the same
   * whether or not the list is open when the key is pressed, and so it cannot
   * run twice for one keystroke.
   */
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      setOpen(false);
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (manualOnly) {
        return;
      }
      // Prevented so the caret does not jump to the ends of the field.
      event.preventDefault();
      setOpen(true);
      setHighlighted((current) => {
        if (filtered.length === 0) {
          return 0;
        }
        const step = event.key === "ArrowDown" ? 1 : -1;
        return (current + step + filtered.length) % filtered.length;
      });
      return;
    }
    // Enter takes the highlighted entry, which is why a list opened with the
    // arrows is usable without ever touching the pointer.
    if (event.key === "Enter") {
      if (manualOnly) {
        return;
      }
      const choice = filtered[highlighted];
      if (choice) {
        event.preventDefault();
        onChange({ input: choice.name, selected: choice });
        setOpen(false);
      }
    }
  };

  return (
    <div className="compare__picker" ref={rootRef} onKeyDown={onKeyDown}>
      <span className="compare__picker-label">{label}</span>
      <div className="compare__picker-input">
        <TextInput
          value={query}
          size="small"
          monospace
          testId={`compare-input-${label}`}
          onChange={(value) => onChange({ input: value, selected: null })}
          onSubmit={() => setOpen(true)}
          placeholder={t(translate, "branch-compare-revision-placeholder")}
        />
        {/* Free-form entry, for a revision no list can offer. The suggestion
            list is suppressed while it is on, because a list that keeps
            changing under a typed object id is not a list. */}
        <label
          className="switch"
          data-testid={`compare-manual-${label}`}
          title={t(translate, "branch-compare-manual-input")}
        >
          <input
            type="checkbox"
            checked={manualOnly}
            onChange={(event) => {
              setManualOnly(event.target.checked);
              setOpen(false);
            }}
          />
          <span>{t(translate, "branch-compare-manual-input")}</span>
        </label>
        {open && !manualOnly ? (
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
                      className={`compare__picker-option${
                        filtered[highlighted] === option ? " is-highlighted" : ""
                      }`}
                      data-testid={`compare-option-${option.kind}-${option.full_name}`}
                      onClick={() => {
                        onChange({ input: option.name, selected: option });
                        setOpen(false);
                      }}
                    >
                      <Icon
                        name={
                          option.kind === "commit"
                            ? "git-commit-horizontal"
                            : "git-branch"
                        }
                        size={11}
                      />
                      {optionLabel(option, subjectFor(option), translate)}
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
                {ta(translate, "branch-compare-use-commit", { sha: manual.name })}
              </button>
            ) : null}
            {invalid ? (
              <div className="compare__picker-error">
                {t(translate, "branch-compare-invalid-revision")}
              </div>
            ) : null}
            {unavailable ? (
              <div
                className="compare__picker-error"
                data-testid={`compare-unavailable-${label}`}
              >
                {t(translate, "branch-compare-revision-unavailable")}
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
