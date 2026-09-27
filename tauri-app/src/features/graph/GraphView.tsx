/**
 * The commit graph.
 *
 * Rows are virtualized because a long history can hold thousands of commits.
 * The lane layout comes from the backend so the drawing matches the reference
 * application exactly, and ref decorations are parsed there too.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { Icon } from "../../components/Icon";
import {
  ContextMenu,
  EmptyState,
  Menu,
  TextInput,
  VirtualList,
} from "../../components/controls";
import * as ipc from "../../bridge/ipc";
import type {
  GraphRow,
  LogRow,
  RefLabel,
} from "../../bridge/types";
import { useStore, type RepoState } from "../../app/store";
import { LANE_COLORS } from "../../styles/themes";
import { t, ta } from "../../i18n/strings";
import { GraphSvg, ROW_HEIGHT, type LaneGeometry } from "./GraphSvg";
import { filterCommits, type CommitSearchField } from "./commitSearch";

/** Rows from the end of the list that trigger the next page request. */
const LOAD_AHEAD_ROWS = 30;
const HASH_COL_WIDTH = 60;
const AUTHOR_COL_WIDTH = 140;
const DATE_COL_WIDTH = 120;
const MESSAGE_MIN_WIDTH = 120;
const COL_GAP = 8;
const ROW_PAD_RIGHT = 8;

export function GraphView({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const selectCommit = useStore((state) => state.selectCommit);
  const clearCommit = useStore((state) => state.clearCommit);
  const runAction = useStore((state) => state.runAction);
  const setMessage = useStore((state) => state.setMessage);
  const [query, setQuery] = useState("");
  const [field, setField] = useState<CommitSearchField>("subject");
  const [layout, setLayout] = useState<{
    graph: GraphRow[];
    labels: Record<string, RefLabel[]>;
  }>({ graph: [], labels: {} });
  const [hovered, setHovered] = useState<string | null>(null);
  const [showMessageDialog, setShowMessageDialog] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(900);
  const requestedPages = useRef(new Set<string>());

  // The layout is recomputed whenever the visible rows change. Filtering is
  // local, so the layout only has to follow what is displayed.
  const visibleRows = useMemo(
    () => filterCommits(repo.logRows, query, field),
    [repo.logRows, query, field],
  );

  useEffect(() => {
    let cancelled = false;
    void ipc
      .graphLayout(visibleRows, repo.refs.remotes)
      .then((result) => {
        if (!cancelled) {
          setLayout(result);
        }
      })
      .catch(() => {
        // The repository closed while the page was in flight.
      });
    return () => {
      cancelled = true;
    };
  }, [visibleRows, repo.refs.remotes]);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) {
      return;
    }
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    setWidth(element.clientWidth);
    return () => observer.disconnect();
  }, []);

  // Ask for the next page when the visible window reaches the end.
  const onViewportChange = (range: { start: number; end: number }) => {
    if (!repo.hasMore) {
      return;
    }
    if (range.end < visibleRows.length - LOAD_AHEAD_ROWS) {
      return;
    }
    const marker = `${visibleRows.length}:${repo.logRows.length}`;
    if (requestedPages.current.has(marker)) {
      return;
    }
    requestedPages.current.add(marker);
    void ipc.loadMoreLogPage(repo.id);
  };

  const treeWidth = 12 + (maxLanes(layout.graph) + 1) * 24 + 8;
  const showsAuthor = width >= treeWidth + HASH_COL_WIDTH + AUTHOR_COL_WIDTH + DATE_COL_WIDTH + 4 * COL_GAP + ROW_PAD_RIGHT + MESSAGE_MIN_WIDTH;
  const showsMessage = width >= treeWidth + HASH_COL_WIDTH + DATE_COL_WIDTH + 3 * COL_GAP + ROW_PAD_RIGHT + MESSAGE_MIN_WIDTH;

  const fieldItems = [
    {
      id: "subject",
      label: t(translate, "commit-search-subject"),
      onSelect: () => setField("subject"),
    },
    {
      id: "full",
      label: t(translate, "commit-search-full-message"),
      onSelect: () => setField("full"),
    },
  ];

  const dialogRow = showMessageDialog
    ? visibleRows.find((row) => row.oid === showMessageDialog)
    : null;
  const dialogMessage = dialogRow ? repo.commitMessages[dialogRow.oid] : undefined;

  if (repo.status === "error") {
    return (
      <div className="graph">
        <EmptyState
          message={repo.errorMessage ?? t(translate, "err-unknown")}
          testId="graph-error"
        />
      </div>
    );
  }

  return (
    <div className="graph" data-testid="graph">
      <div className="graph__search">
        <TextInput
          value={query}
          onChange={setQuery}
          placeholder={t(translate, "commit-search-placeholder")}
          cleanable
          size="small"
          prefix={<Icon name="search" size={11} />}
          testId="commit-search"
        />
        <Menu items={fieldItems} testId="commit-search-field" align="end">
          <button type="button" className="tool-button tool-button--compact">
            {field === "subject"
              ? t(translate, "commit-search-subject")
              : t(translate, "commit-search-full-message")}
            <Icon name="chevron-down" size={10} />
          </button>
        </Menu>
        {query ? (
          <span className="graph__search-results" data-testid="commit-search-results">
            {ta(translate, "commit-search-results", {
              matches: visibleRows.length,
              total: repo.logRows.length,
            })}
          </span>
        ) : null}
      </div>
      <div className="graph__rows" ref={containerRef}>
        <VirtualList
          items={visibleRows}
          rowHeight={ROW_HEIGHT}
          onViewportChange={onViewportChange}
          testId="graph-list"
          empty={
            <EmptyState
              icon={<Icon name="git-commit-horizontal" size={24} />}
              message={
                query
                  ? t(translate, "commit-search-no-results")
                  : t(translate, "graph-empty")
              }
              testId="graph-empty"
            />
          }
          renderRow={(row, index) => {
            const graphRow = layout.graph[index];
            const selected = repo.selected?.oid === row.oid;
            return (
              <GraphRowView
                repo={repo}
                row={row}
                graphRow={graphRow}
                labels={layout.labels[row.oid] ?? []}
                laneWidth={treeWidth}
                selected={selected}
                showsAuthor={showsAuthor}
                showsMessage={showsMessage}
                onSelect={() => {
                  void selectCommit(repo.id, row.oid, row.short, row.subject);
                }}
                onHover={(value) => setHovered(value)}
                onClear={() => clearCommit(repo.id)}
                onCheckout={() => {
                  void runAction(repo.id, {
                    action: "checkout",
                    target: { kind: "commit", commit: row.oid },
                  });
                }}
                onCopyOid={() => {
                  void writeText(row.oid).then(() => {
                    setMessage(repo.id, t(translate, "context-copied"), true);
                  });
                }}
                onShowMessage={() => {
                  setShowMessageDialog(row.oid);
                  void ipc.requestCommitMessage(repo.id, row.oid);
                }}
                hovered={hovered === row.oid}
              />
            );
          }}
        />
      </div>
      {dialogRow ? (
        <CommitMessageDialog
          row={dialogRow}
          message={dialogMessage}
          onClose={() => setShowMessageDialog(null)}
        />
      ) : null}
    </div>
  );
}

function GraphRowView({
  repo,
  row,
  graphRow,
  labels,
  laneWidth,
  selected,
  showsAuthor,
  showsMessage,
  onSelect,
  onHover,
  onClear,
  onCheckout,
  onCopyOid,
  onShowMessage,
  hovered,
}: {
  repo: RepoState;
  row: LogRow;
  graphRow: GraphRow | undefined;
  labels: RefLabel[];
  laneWidth: number;
  selected: boolean;
  showsAuthor: boolean;
  showsMessage: boolean;
  onSelect: () => void;
  onHover: (oid: string | null) => void;
  onClear: () => void;
  onCheckout: () => void;
  onCopyOid: () => void;
  onShowMessage: () => void;
  hovered: boolean;
}) {
  const translate = useStore((state) => state.t);
  const geometry: LaneGeometry | null = graphRow
    ? {
        nodeLane: graphRow.node_lane,
        laneCount: graphRow.lane_count,
        colorIndex: graphRow.node_color,
        isHead: graphRow.is_head,
        isMerge: graphRow.is_merge,
        hasIncoming: graphRow.has_incoming,
        nodeInputLanes: graphRow.node_input_lanes,
        parentLanes: graphRow.parent_lanes,
        inputColors: graphRow.input_lanes.map((lane) => lane.color_index),
        outputColors: graphRow.output_lanes.map((lane) => lane.color_index),
      }
    : null;

  const relative = useMemo(
    () => relativeTime(row.timestamp, translate),
    [row.timestamp, translate],
  );
  void onClear;

  const entries = [
    {
      id: "checkout",
      label: t(translate, "context-checkout"),
      icon: <Icon name="git-branch" size={12} />,
      disabled: repo.busy,
      onSelect: onCheckout,
    },
    {
      id: "copy-oid",
      label: t(translate, "context-copy-commit"),
      icon: <Icon name="copy" size={12} />,
      onSelect: onCopyOid,
    },
    {
      id: "show-message",
      label: t(translate, "context-show-commit-message"),
      icon: <Icon name="file" size={12} />,
      onSelect: onShowMessage,
    },
  ];

  return (
    <ContextMenu testId={`graph-row-${row.oid}`} entries={entries}>
      <div
        className={`graph-row${selected ? " is-selected" : ""}`}
        data-testid={`graph-row-${row.short}`}
        title={row.subject}
        onClick={onSelect}
        onMouseEnter={() => onHover(row.oid)}
        onMouseLeave={() => onHover(null)}
      >
        {geometry ? (
          <GraphSvg geometry={geometry} laneColors={LANE_COLORS} width={laneWidth} />
        ) : (
          <span className="graph-row__lanes" style={{ width: laneWidth }} />
        )}
        <span className="graph-row__hash">{row.short}</span>
        {showsAuthor ? (
          <span className="graph-row__author" title={row.author}>
            {row.author}
          </span>
        ) : null}
        {showsMessage ? (
          <span className="graph-row__date" title={row.date}>
            {relative}
          </span>
        ) : null}
        <span className="graph-row__message">
          <span className="graph-row__subject">{row.subject}</span>
          {labels.map((label) => (
            <span
              key={`${label.kind}-${label.name}`}
              className={`ref-label ref-label--${label.kind}`}
            >
              {label.name}
            </span>
          ))}
        </span>
      </div>
      {hovered && repo.commitMessages[row.oid] ? (
        <CommitHoverPreview row={row} message={repo.commitMessages[row.oid]!} />
      ) : null}
    </ContextMenu>
  );
}

function CommitHoverPreview({
  row,
  message,
}: {
  row: LogRow;
  message: NonNullable<RepoState["commitMessages"][string]>;
}) {
  const translate = useStore((state) => state.t);
  if (!message.body && message.co_authors.length === 0) {
    return null;
  }
  return (
    <div className="commit-preview" data-testid="commit-preview">
      {message.body ? <pre className="commit-preview__body">{message.body}</pre> : null}
      {message.co_authors.length ? (
        <div className="commit-preview__authors">
          {message.co_authors.map((author) => (
            <span key={`${author.name}-${author.email}`}>
              {author.email ? `${author.name} <${author.email}>` : author.name}
            </span>
          ))}
        </div>
      ) : null}
      <div className="muted">{row.date}</div>
      <div className="sr-only">{t(translate, "commit-message-preview")}</div>
    </div>
  );
}

function CommitMessageDialog({
  row,
  message,
  onClose,
}: {
  row: LogRow;
  message: { subject: string; body: string; co_authors: { name: string; email: string }[] } | undefined;
  onClose: () => void;
}) {
  const translate = useStore((state) => state.t);
  const [full, setFull] = useState(message?.body ?? "");
  useEffect(() => {
    if (message) {
      setFull([message.subject, message.body].filter(Boolean).join("\n\n"));
    }
  }, [message]);

  return (
    <div
      className="overlay"
      data-testid="commit-message-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div className="dialog" role="dialog" aria-modal="true">
        <div className="dialog__title">
          {t(translate, "commit-message-dialog-title")}
        </div>
        <div className="dialog__body">
          {message ? (
            <>
              <div className="muted">
                {row.short} · {row.author} · {row.date}
              </div>
              <pre className="commit-preview__body" data-testid="commit-message-body">
                {full}
              </pre>
              {message.co_authors.length ? (
                <div>
                  <div className="muted">{t(translate, "commit-coauthors")}</div>
                  {message.co_authors.map((author) => (
                    <div key={`${author.name}-${author.email}`}>
                      {author.email ? `${author.name} <${author.email}>` : author.name}
                    </div>
                  ))}
                </div>
              ) : null}
            </>
          ) : (
            <div className="muted">{t(translate, "commit-message-loading")}</div>
          )}
        </div>
        <div className="dialog__footer">
          <button
            type="button"
            className="tool-button tool-button--primary"
            onClick={onClose}
            data-testid="commit-message-close"
          >
            {t(translate, "dialog-cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}

function maxLanes(rows: GraphRow[]): number {
  let max = 0;
  for (const row of rows) {
    if (row.lane_count > max) {
      max = row.lane_count;
    }
  }
  return max;
}

/** Relative time, using the same thresholds and keys as the reference app. */
export function relativeTime(
  timestamp: number,
  translate: (key: string, args?: Record<string, string | number>) => string,
): string {
  const now = Math.floor(Date.now() / 1000);
  const minutes = Math.max(0, Math.floor((now - timestamp) / 60));
  if (minutes < 1) {
    return translate("rel-now");
  }
  if (minutes < 60) {
    return translate("rel-min", { n: minutes });
  }
  if (minutes < 60 * 24) {
    return translate("rel-hour", { n: Math.floor(minutes / 60) });
  }
  if (minutes < 60 * 24 * 7) {
    return translate("rel-day", { n: Math.floor(minutes / (60 * 24)) });
  }
  if (minutes < 60 * 24 * 30) {
    return translate("rel-week", { n: Math.floor(minutes / (60 * 24 * 7)) });
  }
  if (minutes < 60 * 24 * 365) {
    return translate("rel-month", { n: Math.floor(minutes / (60 * 24 * 30)) });
  }
  return translate("rel-year", { n: Math.floor(minutes / (60 * 24 * 365)) });
}
