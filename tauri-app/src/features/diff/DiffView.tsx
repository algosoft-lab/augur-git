/**
 * The diff viewer.
 *
 * Two layouts share one virtualized row list: inline pairs a deleted line with
 * its replacement, side by side aligns the two runs so both panes stay the same
 * height. Character-level ranges produced by the backend are painted as marks
 * inside the line, which is what makes a one-character change visible in a long
 * line.
 *
 * The backend also returns aligned rows for the side-by-side layout, so the
 * pairing rule is implemented once.
 */

import { useMemo } from "react";

import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { EmptyState, Spinner, VirtualList } from "../../components/controls";
import type { CharRange, DiffDocument, DiffRow } from "../../bridge/types";
import { useStore } from "../../app/store";
import { t } from "../../i18n/strings";
import { tokenize } from "./highlight";

const ROW_HEIGHT = 18;

/** Character ranges the backend computed for a document. */
export interface InlineRanges {
  old: CharRange[][];
  new: CharRange[][];
}

export interface DiffViewProps {
  document: DiffDocument | null;
  layout: "inline" | "side-by-side";
  /** Inline ranges, requested lazily for the inline layout. */
  ranges?: InlineRanges | null;
  loading?: boolean;
  error?: string | null;
  testId?: string;
  emptyMessage?: string;
}

export function DiffView({
  document,
  layout,
  ranges,
  loading,
  error,
  testId,
  emptyMessage,
}: DiffViewProps) {
  const translate = useStore((state) => state.t);
  const setMessage = useStore((state) => state.setMessage);

  const rows = useMemo<DiffRow[]>(
    () =>
      document
        ? layout === "side-by-side"
          ? document.aligned_rows
          : document.rows
        : [],
    [document, layout],
  );

  const copy = () => {
    if (!document) {
      return;
    }
    void writeText(document.copy_text).then(() => {
      // The status bar belongs to the repository, which the caller knows and
      // this component does not; the copy itself is what matters here.
      setMessage(-1, t(translate, "context-copied"), true);
    });
  };

  if (error) {
    return (
      <div className="diff" data-testid={testId}>
        <EmptyState message={error} testId="diff-error" />
      </div>
    );
  }
  if (loading) {
    return (
      <div
        className="diff"
        data-testid={testId}
        style={{ alignItems: "center", justifyContent: "center" }}
      >
        <Spinner size={16} />
      </div>
    );
  }
  if (!document) {
    return (
      <div className="diff" data-testid={testId}>
        <EmptyState
          icon={undefined}
          message={emptyMessage ?? t(translate, "diff-no-output")}
          testId="diff-empty"
        />
      </div>
    );
  }
  if (document.binary) {
    return (
      <div className="diff" data-testid={testId}>
        <EmptyState message={t(translate, "bottom-bin")} testId="diff-binary" />
      </div>
    );
  }

  return (
    <div className="diff" data-testid={testId}>
      <div className="bottom__toolbar">
        <button
          type="button"
          className="tool-button tool-button--compact"
          data-testid="diff-copy"
          onClick={copy}
        >
          {t(translate, "context-copied")}
        </button>
      </div>
      <div style={{ flex: 1, minHeight: 0 }}>
        <VirtualList
          items={rows}
          rowHeight={ROW_HEIGHT}
          testId="diff-rows"
          className={layout === "side-by-side" ? "diff--split" : "diff--inline"}
          renderRow={(row) => (
            <DiffRowView
              row={row}
              layout={layout}
              language={document.language}
              ranges={ranges}
            />
          )}
        />
      </div>
    </div>
  );
}

function DiffRowView({
  row,
  layout,
  language,
  ranges,
}: {
  row: DiffRow;
  layout: "inline" | "side-by-side";
  language: string | null;
  ranges?: InlineRanges | null;
}) {
  if (row.kind === "hunk") {
    return (
      <div className="diff__row diff__row--hunk" data-testid="diff-hunk">
        <span className="diff__gutter" />
        <span className="diff__text">{row.hunk_header}</span>
      </div>
    );
  }

  if (layout === "side-by-side") {
    return (
      <div className="diff__row" data-testid="diff-row">
        <span className="diff__gutter">{row.old_no ?? ""}</span>
        <span
          className={`diff__text diff__text--del${row.old_text === null ? " is-empty" : ""}`}
        >
          {row.old_text === null ? "" : highlight(row.old_text, language, oldRanges(ranges, row))}
        </span>
        <span className="diff__gutter diff__gutter--new">{row.new_no ?? ""}</span>
        <span
          className={`diff__text diff__text--add${row.new_text === null ? " is-empty" : ""}`}
        >
          {row.new_text === null ? "" : highlight(row.new_text, language, newRanges(ranges, row))}
        </span>
      </div>
    );
  }

  const text = row.kind === "add" ? row.new_text : row.old_text;
  const rangesForLine = row.kind === "add" ? newRanges(ranges, row) : oldRanges(ranges, row);
  const className =
    row.kind === "add" ? "diff__row--add" : row.kind === "del" ? "diff__row--del" : "";
  return (
    <div className={`diff__row ${className}`} data-testid="diff-row">
      <span className="diff__gutter">{row.old_no ?? ""}</span>
      <span className="diff__gutter diff__gutter--new">{row.new_no ?? ""}</span>
      <span className="diff__marker">
        {row.kind === "add" ? "+" : row.kind === "del" ? "-" : " "}
      </span>
      <span className="diff__text">
        {text === null || text === undefined ? "" : highlight(text, language, rangesForLine)}
      </span>
    </div>
  );
}

function oldRanges(ranges: InlineRanges | null | undefined, row: DiffRow): CharRange[] | null {
  if (!ranges || row.old_line_index === null) {
    return null;
  }
  return ranges.old[row.old_line_index] ?? null;
}

function newRanges(ranges: InlineRanges | null | undefined, row: DiffRow): CharRange[] | null {
  if (!ranges || row.new_line_index === null) {
    return null;
  }
  return ranges.new[row.new_line_index] ?? null;
}

/**
 * Render one line as highlighted tokens with change ranges marked.
 *
 * The two overlays are combined into a single pass so a mark can wrap a token
 * boundary, which is what the reference application does.
 */
function highlight(
  text: string,
  language: string | null,
  ranges: CharRange[] | null,
): React.ReactNode {
  const tokens = tokenize(text, language);
  const segments: { start: number; end: number; kind: string | null; mark: boolean }[] = [];

  if (tokens) {
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
    segments.push({ start: 0, end: text.length, kind: null, mark: false });
  }

  // Split segments at change-range boundaries so a mark can be applied to part
  // of a token.
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
    segments.length = 0;
    segments.push(...refined);
  }

  return (
    <>
      {segments.map((segment, index) => {
        const content = text.slice(segment.start, segment.end);
        const className = [segment.kind, segment.mark ? "is-changed" : ""]
          .filter(Boolean)
          .join(" ");
        if (!className) {
          return <span key={index}>{content}</span>;
        }
        return segment.mark ? (
          <mark key={index} className={segment.kind ?? undefined}>
            {content}
          </mark>
        ) : (
          <span key={index} className={className}>
            {content}
          </span>
        );
      })}
    </>
  );
}
