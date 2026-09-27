/**
 * The diff viewer.
 *
 * One or more documents are flattened into a single row list with a file header
 * between them, which is how the reference application shows every file of a
 * commit at once. The list is virtualized because a large commit can be tens of
 * thousands of lines.
 *
 * Two layouts share that list: inline pairs a deleted line with its replacement,
 * side by side aligns the two runs so both panes stay the same height. The
 * character-level ranges computed by the backend are painted as marks inside the
 * line, which is what makes a one-character change visible in a long line.
 */

import { useMemo } from "react";

import { EmptyState, Spinner, VirtualList } from "../../components/controls";
import type { CharRange, DiffPayload, DiffRow } from "../../bridge/types";
import { useStore } from "../../app/store";
import { tokenize } from "./highlight";
import { t } from "../../i18n/strings";

/** Every row in the viewer is this tall, headers included. */
export const DIFF_ROW_HEIGHT = 22;
/** Below this width the all-files view forces the inline layout. */
export const NARROW_WIDTH = 900;

/** One document in a multi-document view. */
export interface DiffSection {
  path: string;
  document: DiffPayload;
}

/** A flattened entry in the virtualized list. */
type Item =
  | { kind: "header"; path: string }
  | { kind: "binary"; path: string }
  | { kind: "empty"; path: string }
  | { kind: "row"; document: DiffPayload; row: DiffRow };

export interface DiffViewProps {
  sections: DiffSection[];
  layout: "inline" | "side-by-side";
  /** Override the layout, used to force inline when the panel is narrow. */
  forceInline?: boolean;
  loading?: boolean;
  error?: string | null;
  /**
   * What to say while loading, and above an error.
   *
   * Passed in rather than hardcoded, because the panel and the comparison window
   * load the same way but mean different things by it, and a spinner with no
   * label reads as a frozen view.
   */
  loadingMessage?: string;
  errorLabel?: string;
  testId?: string;
  emptyMessage?: string;
  /** Title of the toolbar above a multi-document view. */
  header?: string;
  /**
   * Whether each document gets its own path header.
   *
   * The comparison window always shows them, even for one file, because its
   * file list is a selection over a set rather than a navigation between
   * documents.
   */
  showFileHeaders?: boolean;
  onCopy?: () => void;
}

/** The two overlapping sheets of the copy icon. */
function CopyGlyph() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="9" y="9" width="12" height="12" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}

export function DiffView({
  sections,
  layout,
  forceInline = false,
  loading,
  error,
  loadingMessage,
  errorLabel,
  testId,
  emptyMessage,
  header,
  showFileHeaders = false,
  onCopy,
}: DiffViewProps) {
  const translate = useStore((state) => state.t);
  const effective = forceInline ? "inline" : layout;

  const withHeaders = showFileHeaders || sections.length > 1;
  const items = useMemo<Item[]>(() => {
    const out: Item[] = [];
    for (const section of sections) {
      if (withHeaders) {
        out.push({ kind: "header", path: section.path });
      }
      if (section.document.binary) {
        out.push({ kind: "binary", path: section.path });
        continue;
      }
      const rows =
        effective === "side-by-side"
          ? (section.document.aligned_rows ?? section.document.rows)
          : section.document.rows;
      if (rows.length === 0) {
        out.push({ kind: "empty", path: section.path });
        continue;
      }
      for (const row of rows) {
        out.push({ kind: "row", document: section.document, row });
      }
    }
    return out;
  }, [sections, effective, withHeaders]);

  if (error) {
    return (
      <div
        className="diff"
        data-testid={testId}
        style={{ alignItems: "center", justifyContent: "center" }}
      >
        {/* A heading and a reason, as the reference splits them: the heading
            says what failed and the reason says why, and Git's own words are
            only useful once the reader knows which step they belong to. */}
        {errorLabel ? (
          <div className="diff-state-label" data-testid="diff-error-label">
            {errorLabel}
          </div>
        ) : null}
        <EmptyState message={error} testId="diff-error" />
      </div>
    );
  }
  if (loading && items.length === 0) {
    return (
      <div
        className="diff"
        data-testid={testId}
        style={{ alignItems: "center", justifyContent: "center" }}
      >
        <Spinner size={16} />
        {loadingMessage ? (
          <div className="muted" data-testid="diff-loading-label">
            {loadingMessage}
          </div>
        ) : null}
      </div>
    );
  }
  if (items.length === 0) {
    return (
      <div className="diff" data-testid={testId}>
        <EmptyState
          message={emptyMessage ?? t(translate, "diff-no-output")}
          testId="diff-empty"
        />
      </div>
    );
  }

  return (
    <div className="diff" data-testid={testId}>
      {header ? (
        <div className="bottom__toolbar">
          <span className="bottom__toolbar-title">{header}</span>
          <span className="bottom__toolbar-spacer" />
          {onCopy ? (
            <button
              type="button"
              className="icon-button"
              title={t(translate, "diff-copy-tooltip")}
              aria-label={t(translate, "diff-copy-tooltip")}
              data-testid="diff-copy"
              onClick={onCopy}
            >
              <CopyGlyph />
            </button>
          ) : null}
        </div>
      ) : null}
      <div style={{ flex: 1, minHeight: 0 }}>
        <VirtualList
          items={items}
          rowHeight={DIFF_ROW_HEIGHT}
          testId="diff-rows"
          className={effective === "side-by-side" ? "diff--split" : "diff--inline"}
          renderRow={(item) => <DiffItem item={item} layout={effective} />}
        />
      </div>
    </div>
  );
}

function DiffItem({
  item,
  layout,
}: {
  item: Item;
  layout: "inline" | "side-by-side";
}) {
  if (item.kind === "header") {
    return (
      <div className="diff__file-header" data-testid="diff-file-header">
        <span>{item.path}</span>
      </div>
    );
  }
  if (item.kind === "binary") {
    return (
      <div className="diff__note" data-testid="diff-binary">
        {useStore.getState().t("bottom-bin")}
      </div>
    );
  }
  if (item.kind === "empty") {
    return (
      <div className="diff__note" data-testid="diff-empty-row">
        {useStore.getState().t("diff-no-output")}
      </div>
    );
  }
  return <DiffRowView row={item.row} document={item.document} layout={layout} />;
}

function DiffRowView({
  row,
  document,
  layout,
}: {
  row: DiffRow;
  document: DiffPayload;
  layout: "inline" | "side-by-side";
}) {
  const ranges = {
    old: document.inline_old,
    new: document.inline_new,
  };

  if (row.kind === "hunk") {
    return (
      <div className="diff__row diff__row--hunk" data-testid="diff-hunk">
        <span className="diff__gutter" />
        <span className="diff__gutter" />
        <span className="diff__marker" />
        <span className="diff__text">{row.hunk_header}</span>
      </div>
    );
  }

  if (layout === "side-by-side") {
    return (
      <div className="diff__row" data-testid="diff-row">
        <span className="diff__gutter">{row.old_no ?? ""}</span>
        <span
          className={`diff__text diff__text--old${row.old_text === null ? " is-empty" : ""}`}
        >
          {row.old_text === null
            ? ""
            : highlight(row.old_text, document.language, oldRanges(ranges, row))}
        </span>
        <span className="diff__gutter">{row.new_no ?? ""}</span>
        <span
          className={`diff__text diff__text--new${row.new_text === null ? " is-empty" : ""}`}
        >
          {row.new_text === null
            ? ""
            : highlight(row.new_text, document.language, newRanges(ranges, row))}
        </span>
      </div>
    );
  }

  const newSide = row.kind === "add" || (row.kind === "context" && row.new_text !== null);
  const text = newSide ? row.new_text : row.old_text;
  const modifier =
    row.kind === "add" ? "add" : row.kind === "del" ? "del" : "context";
  return (
    <div className={`diff__row diff__row--${modifier}`} data-testid="diff-row">
      <span className="diff__gutter">{row.old_no ?? ""}</span>
      <span className="diff__gutter">{row.new_no ?? ""}</span>
      <span
        className={`diff__marker${newSide ? " diff__marker--new" : " diff__marker--old"}`}
      >
        {row.kind === "add" ? "+" : row.kind === "del" ? "-" : " "}
      </span>
      <span className="diff__text">
        {text === null || text === undefined
          ? ""
          : highlight(
              text,
              document.language,
              newSide ? newRanges(ranges, row) : oldRanges(ranges, row),
            )}
      </span>
    </div>
  );
}

function oldRanges(
  ranges: { old: CharRange[][]; new: CharRange[][] },
  row: DiffRow,
): CharRange[] | null {
  if (row.old_line_index === null) {
    return null;
  }
  return ranges.old[row.old_line_index] ?? null;
}

function newRanges(
  ranges: { old: CharRange[][]; new: CharRange[][] },
  row: DiffRow,
): CharRange[] | null {
  if (row.new_line_index === null) {
    return null;
  }
  return ranges.new[row.new_line_index] ?? null;
}

/**
 * Render one line as highlighted tokens with change ranges marked.
 *
 * The two overlays are combined into one pass so a mark can wrap a token
 * boundary, which is what the reference application does.
 */
function highlight(
  text: string,
  language: string | null,
  ranges: CharRange[] | null,
): React.ReactNode {
  const tokens = tokenize(text, language);
  let segments: { start: number; end: number; kind: string | null; mark: boolean }[];

  if (tokens) {
    segments = [];
    let index = 0;
    for (const token of tokens) {
      if (token.start > index) {
        segments.push({ start: index, end: token.start, kind: null, mark: false });
      }
      segments.push({
        start: token.start,
        end: token.end,
        kind: `token-${token.kind}`,
        mark: false,
      });
      index = token.end;
    }
    if (index < text.length) {
      segments.push({ start: index, end: text.length, kind: null, mark: false });
    }
  } else {
    segments = [{ start: 0, end: text.length, kind: null, mark: false }];
  }

  // Split segments at change-range boundaries so a mark can cover part of a
  // token.
  if (ranges && ranges.length) {
    const refined: typeof segments = [];
    for (const segment of segments) {
      let cursor = segment.start;
      for (const range of ranges) {
        const start = Math.max(range.start, segment.start);
        const end = Math.min(range.end, segment.end);
        if (start >= end) {
          continue;
        }
        if (start > cursor) {
          refined.push({ ...segment, start: cursor, end: start, mark: false });
        }
        refined.push({ ...segment, start, end, mark: true });
        cursor = end;
      }
      if (cursor < segment.end) {
        refined.push({ ...segment, start: cursor, end: segment.end, mark: false });
      }
    }
    segments = refined;
  }

  return (
    <>
      {segments.map((segment, index) => {
        const content = text.slice(segment.start, segment.end);
        if (segment.mark) {
          return (
            <mark key={index} className={segment.kind ?? undefined}>
              {content}
            </mark>
          );
        }
        return segment.kind ? (
          <span key={index} className={segment.kind}>
            {content}
          </span>
        ) : (
          <span key={index}>{content}</span>
        );
      })}
    </>
  );
}
