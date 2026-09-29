/**
 * The commit graph.
 *
 * Rows are virtualized because a long history can hold thousands of commits.
 * The lane layout comes from the backend so the drawing matches the reference
 * application exactly, and ref decorations are parsed there too.
 */

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { writeText } from '@tauri-apps/plugin-clipboard-manager';

import { Icon } from '../../components/Icon';
import { ContextMenu, EmptyState, Menu, TextInput, VirtualList } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import type { GraphRow, LogRow, RefLabel } from '../../bridge/types';
import { useStore, type RepoState } from '../../app/store';
import { LANE_COLORS } from '../../styles/themes';
import { t, ta } from '../../i18n/strings';
import { COL_WIDTH, GRAPH_LEFT_PAD, GraphSvg, ROW_HEIGHT, type LaneGeometry } from './GraphSvg';
import { filterCommits, type CommitSearchField } from './commitSearch';
import { hasOpenPopup, keysForCommand, matchesShortcut } from '../../app/keyboard';

/** Rows from the end of the list that trigger the next page request. */
const LOAD_AHEAD_ROWS = 30;

/** Width of the lane area for a graph with this many lanes. */
export function laneAreaWidth(laneCount: number): number {
  return GRAPH_LEFT_PAD + laneCount * COL_WIDTH + 8;
}

export function GraphView({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const selectCommit = useStore((state) => state.selectCommit);
  const clearCommit = useStore((state) => state.clearCommit);
  const runAction = useStore((state) => state.runAction);
  const setMessage = useStore((state) => state.setMessage);
  const [query, setQuery] = useState('');
  const [field, setField] = useState<CommitSearchField>('subject');
  const [layout, setLayout] = useState<{
    graph: GraphRow[];
    labels: Record<string, RefLabel[]>;
  }>({ graph: [], labels: {} });
  const [hovered, setHovered] = useState<{ oid: string; x: number; y: number } | null>(null);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const [showMessageDialog, setShowMessageDialog] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(900);
  const requestedPages = useRef(new Set<string>());
  // The layout is recomputed whenever the visible rows change. Filtering is
  // local, so the layout only has to follow what is displayed.
  const visibleRows = useMemo(
    () => filterCommits(repo.logRows, query, field),
    [repo.logRows, query, field]
  );

  // A filter that hides the selected commit clears the selection, because the
  // diff panel would otherwise keep showing a row the list no longer contains.
  useEffect(() => {
    if (!repo.selected) {
      return;
    }
    if (!visibleRows.some((row) => row.oid === repo.selected?.oid)) {
      clearCommit(repo.id);
    }
  }, [repo.id, repo.selected, visibleRows]);

  useEffect(() => {
    if (!repo.selected) {
      setActiveIndex(null);
      return;
    }
    const index = visibleRows.findIndex((row) => row.oid === repo.selected?.oid);
    setActiveIndex(index >= 0 ? index : null);
  }, [repo.selected, visibleRows]);

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

  const selectGraphIndex = (index: number, focus = false) => {
    const row = visibleRows[index];
    if (!row) return;
    setActiveIndex(index);
    void selectCommit(repo.id, row.oid, row.short, row.subject);
    const list = containerRef.current?.querySelector<HTMLElement>('[data-testid="graph-list"]');
    if (focus) list?.focus();
    if (list) list.scrollTop = index * ROW_HEIGHT;
  };

  const onGraphKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (hasOpenPopup()) return;
    const target = event.target;
    if (!(target instanceof HTMLElement) || !target.closest('[data-testid="graph-list"]')) {
      return;
    }
    const shortcuts = useStore.getState().shortcuts.resolved;
    const next = matchesShortcut(event.nativeEvent, keysForCommand(shortcuts, 'list.next'));
    const previous = matchesShortcut(event.nativeEvent, keysForCommand(shortcuts, 'list.previous'));
    if (next || previous) {
      if (visibleRows.length === 0) return;
      event.preventDefault();
      const current = activeIndex ?? -1;
      const index =
        current < 0
          ? next
            ? 0
            : visibleRows.length - 1
          : Math.max(0, Math.min(visibleRows.length - 1, current + (next ? 1 : -1)));
      selectGraphIndex(index, true);
      return;
    }
    if (matchesShortcut(event.nativeEvent, keysForCommand(shortcuts, 'commits.checkout'))) {
      event.preventDefault();
      const index = activeIndex;
      const row = index === null ? null : visibleRows[index];
      if (!event.repeat && row && !repo.busy) {
        void runAction(repo.id, {
          action: 'checkout',
          target: { kind: 'commit', commit: row.oid }
        });
      }
    }
  };

  // The thresholds belong to the backend, so a column appears at exactly the
  // width it does in the reference application. The answer is only needed when
  // the width or the lane count actually changes.
  const lanes = maxLanes(layout.graph);
  const laneWidth = laneAreaWidth(lanes);
  const columns = useColumnVisibility(width, laneWidth);
  const showsAuthor = columns.author;
  const showsMessage = columns.message;

  // Marked rather than implied by the trigger's own label, so the list says
  // what the search is matching on rather than only what it is called.
  const fieldItems = [
    {
      id: 'subject',
      label: t(translate, 'commit-search-subject'),
      checked: field === 'subject',
      onSelect: () => setField('subject')
    },
    {
      id: 'full',
      label: t(translate, 'commit-search-full-message'),
      checked: field === 'full',
      onSelect: () => setField('full')
    }
  ];

  const dialogRow = showMessageDialog
    ? visibleRows.find((row) => row.oid === showMessageDialog)
    : null;
  const dialogMessage = dialogRow ? repo.commitMessages[dialogRow.oid] : undefined;

  if (repo.status === 'error') {
    return (
      <div className="graph">
        <EmptyState
          message={repo.errorMessage ?? t(translate, 'err-unknown')}
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
          placeholder={t(translate, 'commit-search-placeholder')}
          cleanable
          size="small"
          prefix={<Icon name="search" size={11} />}
          testId="commit-search"
        />
        <Menu items={fieldItems} testId="commit-search-field" align="end">
          <button type="button" className="tool-button tool-button--compact">
            {field === 'subject'
              ? t(translate, 'commit-search-subject')
              : t(translate, 'commit-search-full-message')}
            <Icon name="chevron-down" size={10} />
          </button>
        </Menu>
        {query ? (
          <span className="graph__search-results" data-testid="commit-search-results">
            {ta(translate, 'commit-search-results', {
              matches: visibleRows.length,
              total: repo.logRows.length
            })}
          </span>
        ) : null}
      </div>
      {/* The header uses the same widths as the rows, so a label always sits
          over the column it names, and it hides the same columns. */}
      <div className="graph-header" data-testid="graph-header">
        <div
          className="graph-header__graph"
          style={{ width: laneWidth }}
          data-testid="graph-header-graph"
        >
          {t(translate, 'col-graph')}
        </div>
        <div className="graph-header__label" style={{ width: 60 }} data-testid="graph-header-hash">
          <span className="graph-header__divider" />
          {t(translate, 'col-hash')}
        </div>
        {showsMessage ? (
          <div className="graph-header__message" data-testid="graph-header-message">
            <span className="graph-header__divider" />
            {t(translate, 'col-message')}
          </div>
        ) : null}
        {showsAuthor ? (
          <div
            className="graph-header__label"
            style={{ width: 140 }}
            data-testid="graph-header-author"
          >
            <span className="graph-header__divider" />
            {t(translate, 'col-author')}
          </div>
        ) : null}
        <div className="graph-header__label" style={{ width: 120 }} data-testid="graph-header-date">
          <span className="graph-header__divider" />
          {t(translate, 'col-date')}
        </div>
      </div>
      <div className="graph__rows" ref={containerRef}>
        <VirtualList
          items={visibleRows}
          rowHeight={ROW_HEIGHT}
          onViewportChange={onViewportChange}
          testId="graph-list"
          focusable
          role="listbox"
          aria-activedescendant={
            activeIndex === null ? undefined : `commit-row-${visibleRows[activeIndex]?.oid}`
          }
          onKeyDown={onGraphKeyDown}
          empty={
            <EmptyState
              icon={<Icon name="git-commit-horizontal" size={24} />}
              message={
                query ? t(translate, 'commit-search-no-results') : t(translate, 'graph-empty')
              }
              testId={query ? 'commit-search-no-results' : 'graph-empty'}
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
                laneWidth={laneWidth}
                selected={selected}
                showsAuthor={showsAuthor}
                showsMessage={showsMessage}
                onHover={(oid, x, y) =>
                  setHovered(
                    oid === null || x === undefined || y === undefined ? null : { oid, x, y }
                  )
                }
                onCheckout={() => {
                  void runAction(repo.id, {
                    action: 'checkout',
                    target: { kind: 'commit', commit: row.oid }
                  });
                }}
                onCopyMessage={() => {
                  void runAction(repo.id, {
                    action: 'copyCommitMessage',
                    oid: row.oid
                  });
                }}
                onCopyOid={() => {
                  void writeText(row.oid).then(() => {
                    setMessage(repo.id, ta(translate, 'context-copied', { name: row.short }), true);
                  });
                }}
                onShowMessage={() => {
                  setShowMessageDialog(row.oid);
                  void ipc.requestCommitMessage(repo.id, row.oid);
                }}
                hovered={hovered?.oid === row.oid}
                anchor={hovered?.oid === row.oid ? { x: hovered.x, y: hovered.y } : null}
                onSelect={() => selectGraphIndex(index, true)}
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
  onCheckout,
  onCopyOid,
  onCopyMessage,
  onShowMessage,
  hovered,
  anchor
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
  onHover: (oid: string | null, x?: number, y?: number) => void;
  onCheckout: () => void;
  onCopyOid: () => void;
  onCopyMessage: () => void;
  onShowMessage: () => void;
  hovered: boolean;
  anchor: { x: number; y: number } | null;
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
        inputLanes: graphRow.input_lanes.map((lane) => ({
          oid: lane.oid,
          colorIndex: lane.color_index
        })),
        outputLanes: graphRow.output_lanes.map((lane) => ({
          oid: lane.oid,
          colorIndex: lane.color_index
        }))
      }
    : null;

  const relative = useMemo(
    () => relativeTime(row.timestamp, translate),
    [row.timestamp, translate]
  );

  const entries = [
    {
      id: 'checkout',
      label: t(translate, 'context-checkout'),
      icon: <Icon name="git-branch" size={12} />,
      disabled: repo.busy,
      onSelect: onCheckout
    },
    {
      id: 'copy-oid',
      label: t(translate, 'context-copy-commit'),
      icon: <Icon name="copy" size={12} />,
      onSelect: onCopyOid
    },
    {
      id: 'copy-message',
      label: t(translate, 'context-copy-commit-message'),
      icon: <Icon name="copy" size={12} />,
      disabled: repo.busy,
      // Goes through the worker rather than the cached message, because the
      // clipboard wants the message as Git renders it.
      onSelect: onCopyMessage
    },
    {
      id: 'show-message',
      label: t(translate, 'context-show-commit-message'),
      icon: <Icon name="file" size={12} />,
      onSelect: onShowMessage
    }
  ];

  return (
    <ContextMenu
      testId={`graph-row-${row.oid}`}
      entries={entries}
      onOpenChange={(menuOpen) => {
        // The menu anchors where the preview does, so opening it retires the
        // preview instead of letting the two stack at the cursor.
        if (menuOpen) {
          onHover(null);
        }
      }}
    >
      <div
        id={`commit-row-${row.oid}`}
        className={`graph-row${selected ? ' is-selected' : ''}`}
        role="option"
        aria-selected={selected}
        data-keyboard-list-item
        data-testid={`graph-row-${row.short}`}
        onClick={onSelect}
        onMouseEnter={(event) => {
          // No preview while a menu is up: rows crossed on the way to a menu
          // item would each flash one behind it.
          if (hasOpenPopup()) {
            return;
          }
          onHover(row.oid, event.clientX, event.clientY);
        }}
        onMouseLeave={() => onHover(null)}
      >
        {geometry ? (
          <GraphSvg geometry={geometry} laneColors={LANE_COLORS} width={laneWidth} />
        ) : (
          <span className="graph-row__lanes" style={{ width: laneWidth }} />
        )}
        <span className="graph-row__hash">{row.short}</span>
        {/*
         * The order matters: the subject takes the remaining space, the ref
         * chips follow it, and the author and date are fixed at the right. The
         * date is always shown, because the threshold for the message column
         * already accounts for it.
         */}
        {showsMessage ? <span className="graph-row__subject">{row.subject}</span> : null}
        {labels.length ? (
          <span className="graph-row__chips">
            {labels.map((label) => (
              <span
                key={`${label.kind}-${label.name}`}
                className={`ref-label ref-label--${label.kind}`}
              >
                {label.name}
              </span>
            ))}
          </span>
        ) : null}
        {showsAuthor ? <span className="graph-row__author">{row.author}</span> : null}
        <span className="graph-row__date">{relative}</span>
      </div>
      {hovered && anchor ? (
        <CommitHoverPreview
          row={row}
          anchor={anchor}
          message={repo.commitMessages[row.oid]}
          onRequest={() => {
            void ipc.requestCommitMessage(repo.id, row.oid);
          }}
        />
      ) : null}
    </ContextMenu>
  );
}

/**
 * The preview shown while a row is hovered.
 *
 * It appears on the first hover rather than the second: the hover is what asks
 * the backend for the message, and until it arrives the preview says it is
 * loading rather than showing nothing at all.
 *
 * The preview portals to the document body, like the context menu: rows live
 * inside the virtual list's transformed window, and a transformed ancestor is
 * the containing block for positioned descendants, which used to displace the
 * preview by the mounted depth instead of anchoring it at the cursor. The row
 * carries no `title` either, so the webview tooltip cannot double the preview.
 *
 * The context menu anchors where the preview does, so it takes priority: an
 * opening menu retires the preview, and no new one appears until the menu is
 * gone and the row is hovered afresh.
 */
function CommitHoverPreview({
  row,
  anchor,
  message,
  onRequest
}: {
  row: LogRow;
  anchor: { x: number; y: number };
  message: NonNullable<RepoState['commitMessages'][string]> | undefined;
  onRequest: () => void;
}) {
  const translate = useStore((state) => state.t);
  const previewRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState(anchor);
  useEffect(() => {
    if (!message) {
      onRequest();
    }
  }, [message, onRequest]);

  // The preview's real size is only known once it has mounted, so the clamp
  // runs after layout: pull the right edge in and flip up at the bottom edge,
  // the way the context menu does.
  useLayoutEffect(() => {
    if (!previewRef.current) {
      return;
    }
    const rect = previewRef.current.getBoundingClientRect();
    const margin = 8;
    const x =
      anchor.x + rect.width > window.innerWidth - margin
        ? Math.max(margin, window.innerWidth - rect.width - margin)
        : anchor.x;
    const y =
      anchor.y + rect.height > window.innerHeight - margin
        ? Math.max(margin, window.innerHeight - rect.height - margin)
        : anchor.y;
    if (x !== position.x || y !== position.y) {
      setPosition({ x, y });
    }
  }, [anchor, position]);

  return createPortal(
    // Portaled so the virtual list's transformed window cannot become this
    // fixed preview's containing block and displace it.
    <div
      className="commit-preview"
      data-testid="commit-preview"
      ref={previewRef}
      style={{ left: position.x, top: position.y }}
    >
      <div className="commit-preview__label" data-testid="commit-preview-label">
        {t(translate, 'commit-message-preview')}
      </div>
      <div className="commit-preview__ident">
        <span className="commit-preview__hash mono">{row.short}</span>
        {row.decorations ? (
          <span className="muted" title={row.decorations}>
            {row.decorations}
          </span>
        ) : null}
      </div>
      <div className="commit-preview__subject">{row.subject}</div>
      {message ? (
        <>
          {message.body ? (
            <pre className="commit-preview__body" data-testid="commit-preview-body">
              {message.body}
            </pre>
          ) : null}
          {message.co_authors.length ? (
            <div className="commit-preview__authors">
              {message.co_authors.map((author) => (
                <span key={`${author.name}-${author.email}`}>
                  {author.email ? `${author.name} <${author.email}>` : author.name}
                </span>
              ))}
            </div>
          ) : null}
        </>
      ) : (
        <div className="muted" data-testid="commit-preview-loading">
          {t(translate, 'commit-message-loading')}
        </div>
      )}
      <div className="commit-preview__meta">
        <span className="muted">{ta(translate, 'commit-author', { author: row.author })}</span>
        <span className="muted">{ta(translate, 'commit-date', { date: row.date })}</span>
      </div>
    </div>,
    document.body
  );
}

function CommitMessageDialog({
  row,
  message,
  onClose
}: {
  row: LogRow;
  message:
    { subject: string; body: string; co_authors: { name: string; email: string }[] } | undefined;
  onClose: () => void;
}) {
  const translate = useStore((state) => state.t);
  const [full, setFull] = useState(message?.body ?? '');
  useEffect(() => {
    if (message) {
      setFull([message.subject, message.body].filter(Boolean).join('\n\n'));
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
      <div className="dialog" role="dialog" aria-modal="true" data-testid="commit-message-dialog">
        <div className="dialog__title">{t(translate, 'commit-message-dialog-title')}</div>
        <div className="dialog__body">
          {/* The hash and the decorations come first, as in the reference: they
              identify the commit, and a body of text without them is not
              identifiable. */}
          <div className="commit-preview__ident">
            <span className="commit-preview__hash mono">{row.short}</span>
            {row.decorations ? (
              <span className="muted" title={row.decorations}>
                {row.decorations}
              </span>
            ) : null}
          </div>
          {message ? (
            <>
              <div className="commit-preview__subject">{row.subject}</div>
              <pre className="commit-preview__body" data-testid="commit-message-body">
                {full}
              </pre>
              {message.co_authors.length ? (
                <div data-testid="commit-message-coauthors">
                  <div className="muted">{t(translate, 'commit-coauthors')}</div>
                  {message.co_authors.map((author) => (
                    <div key={`${author.name}-${author.email}`}>
                      {author.email ? `${author.name} <${author.email}>` : author.name}
                    </div>
                  ))}
                </div>
              ) : null}
              <div className="commit-preview__meta">
                <span className="muted" data-testid="commit-message-author">
                  {ta(translate, 'commit-author', { author: row.author })}
                </span>
                <span className="muted" data-testid="commit-message-date">
                  {ta(translate, 'commit-date', { date: row.date })}
                </span>
              </div>
            </>
          ) : (
            <div className="muted" data-testid="commit-message-loading">
              {t(translate, 'commit-message-loading')}
            </div>
          )}
        </div>
        <div className="dialog__footer">
          <button
            type="button"
            className="tool-button tool-button--primary"
            onClick={onClose}
            data-testid="commit-message-close"
          >
            {t(translate, 'dialog-cancel')}
          </button>
        </div>
      </div>
    </div>
  );
}

/** The widest lane count in the layout, one lane when the graph is empty. */
export function maxLanes(rows: GraphRow[]): number {
  let max = 1;
  for (const row of rows) {
    if (row.lane_count > max) {
      max = row.lane_count;
    }
  }
  return max;
}

/**
 * Ask the backend which columns fit, once per distinct measurement.
 *
 * The first answer is the pessimistic one so a narrow window does not briefly
 * render columns it has no room for.
 */
function useColumnVisibility(
  totalWidth: number,
  laneWidth: number
): { author: boolean; message: boolean } {
  const [visibility, setVisibility] = useState({ author: false, message: false });
  useEffect(() => {
    let cancelled = false;
    void ipc
      .columnVisibility(totalWidth, laneWidth)
      .then((result) => {
        if (!cancelled) {
          setVisibility(result);
        }
      })
      .catch(() => {
        // The repository closed while the answer was in flight.
      });
    return () => {
      cancelled = true;
    };
  }, [totalWidth, laneWidth]);
  return visibility;
}

/**
 * Relative time, using the same thresholds and keys as the reference app.
 *
 * The reference in Unix seconds, as a parameter rather than a clock read, so a
 * test can state the expected answer instead of freezing the clock.
 */
export function relativeTime(
  timestamp: number,
  translate: (key: string, args?: Record<string, string | number>) => string,
  now: number = Math.floor(Date.now() / 1000)
): string {
  const minutes = Math.max(0, Math.floor((now - timestamp) / 60));
  if (minutes < 1) {
    return translate('rel-now');
  }
  if (minutes < 60) {
    return translate('rel-min', { n: minutes });
  }
  if (minutes < 60 * 24) {
    return translate('rel-hour', { n: Math.floor(minutes / 60) });
  }
  if (minutes < 60 * 24 * 7) {
    return translate('rel-day', { n: Math.floor(minutes / (60 * 24)) });
  }
  if (minutes < 60 * 24 * 30) {
    return translate('rel-week', { n: Math.floor(minutes / (60 * 24 * 7)) });
  }
  if (minutes < 60 * 24 * 365) {
    return translate('rel-month', { n: Math.floor(minutes / (60 * 24 * 30)) });
  }
  return translate('rel-year', { n: Math.floor(minutes / (60 * 24 * 365)) });
}
