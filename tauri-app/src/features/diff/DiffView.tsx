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

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { EmptyState, Spinner } from '../../components/controls';
import { MeasuredVirtualList } from '../../components/MeasuredVirtualList';
import type {
  CharRange,
  DiffPayload,
  DiffRow,
  ImagePreview,
  ImagePreviewTarget
} from '../../bridge/types';
import * as ipc from '../../bridge/ipc';
import { useStore } from '../../app/store';
import { tokenize } from './highlight';
import { t } from '../../i18n/strings';
import { canPreviewImage, isSvgPreview } from './imagePreview';

/** Every row in the viewer is this tall, headers included. */
export const DIFF_ROW_HEIGHT = 22;
/** Below this width the all-files view forces the inline layout. */
export const NARROW_WIDTH = 900;

/** One document in a multi-document view. */
export interface DiffSection {
  path: string;
  document: DiffPayload;
  imagePreview?: {
    repoId: number;
    target: ImagePreviewTarget;
    key: string;
  };
}

/** A flattened entry in the virtualized list. */
type Item =
  | { kind: 'header'; path: string }
  | { kind: 'binary'; path: string }
  | { kind: 'empty'; path: string }
  | { kind: 'image'; section: DiffSection }
  | { kind: 'svg-toggle'; key: string; mode: 'image' | 'diff' }
  | { kind: 'row'; document: DiffPayload; row: DiffRow };

interface CachedPreview {
  preview: ImagePreview;
  size: number;
}

const PREVIEW_CACHE_LIMIT = 24 * 1024 * 1024;

export interface DiffViewProps {
  sections: DiffSection[];
  layout: 'inline' | 'side-by-side';
  softWrap?: boolean;
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
  /** A non-blocking refresh state shown while an older diff remains visible. */
  statusMessage?: { kind: 'refreshing' | 'warning'; text: string } | null;
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
  softWrap = false,
  forceInline = false,
  loading,
  error,
  loadingMessage,
  errorLabel,
  statusMessage,
  testId,
  emptyMessage,
  header,
  showFileHeaders = false,
  onCopy
}: DiffViewProps) {
  const translate = useStore((state) => state.t);
  const effective = forceInline ? 'inline' : layout;
  const [width, setWidth] = useState(0);
  const viewRef = useRef<HTMLDivElement>(null);
  const [svgModes, setSvgModes] = useState<Record<string, 'image' | 'diff'>>({});
  const previewCache = useRef(new Map<string, CachedPreview>());
  const pendingPreviews = useRef(new Map<string, Promise<ImagePreview>>());

  useEffect(() => {
    const element = viewRef.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    setWidth(element.clientWidth);
    return () => observer.disconnect();
  }, []);

  const loadPreview = useCallback(async (preview: NonNullable<DiffSection['imagePreview']>) => {
    const cache = previewCache.current;
    const cached = cache.get(preview.key);
    if (cached) {
      cache.delete(preview.key);
      cache.set(preview.key, cached);
      return cached.preview;
    }
    let request = pendingPreviews.current.get(preview.key);
    if (!request) {
      request = ipc.loadImagePreview(preview.repoId, preview.target);
      pendingPreviews.current.set(preview.key, request);
    }
    try {
      const result = await request;
      const size =
        (result.old.status === 'available' ? result.old.data.length * 0.75 : 0) +
        (result.new.status === 'available' ? result.new.data.length * 0.75 : 0);
      cache.set(preview.key, { preview: result, size });
      let cachedSize = [...cache.values()].reduce((total, entry) => total + entry.size, 0);
      while (cachedSize > PREVIEW_CACHE_LIMIT && cache.size > 1) {
        const oldestKey = cache.keys().next().value as string | undefined;
        if (oldestKey === undefined) break;
        const removed = cache.get(oldestKey);
        cache.delete(oldestKey);
        cachedSize -= removed?.size ?? 0;
      }
      return result;
    } finally {
      if (pendingPreviews.current.get(preview.key) === request) {
        pendingPreviews.current.delete(preview.key);
      }
    }
  }, []);

  const withHeaders = showFileHeaders || sections.length > 1;
  const items = useMemo<Item[]>(() => {
    const out: Item[] = [];
    for (const section of sections) {
      if (withHeaders) {
        out.push({ kind: 'header', path: section.path });
      }
      const preview = section.imagePreview;
      const hasPreview = preview !== undefined && canPreviewImage(preview.target);
      const svg = hasPreview && isSvgPreview(preview.target);
      const previewKey = preview?.key ?? '';
      const showSvgDiff =
        svg &&
        !section.document.binary &&
        section.document.rows.length > 0 &&
        svgModes[previewKey] === 'diff';
      if (hasPreview && svg && !section.document.binary && section.document.rows.length > 0) {
        out.push({ kind: 'svg-toggle', key: previewKey, mode: showSvgDiff ? 'diff' : 'image' });
      }
      if (hasPreview && !showSvgDiff) {
        out.push({ kind: 'image', section });
        continue;
      }
      if (section.document.binary) {
        out.push({ kind: 'binary', path: section.path });
        continue;
      }
      const rows =
        effective === 'side-by-side'
          ? (section.document.aligned_rows ?? section.document.rows)
          : section.document.rows;
      if (rows.length === 0) {
        out.push({ kind: 'empty', path: section.path });
        continue;
      }
      for (const row of rows) {
        out.push({ kind: 'row', document: section.document, row });
      }
    }
    return out;
  }, [sections, effective, withHeaders, svgModes]);

  const compactPreview = width < 620;
  const previewHeight = compactPreview ? 480 : 320;
  const updateSvgMode = useCallback((key: string, mode: 'image' | 'diff') => {
    setSvgModes((current) => ({ ...current, [key]: mode }));
  }, []);

  if (error) {
    return (
      <div
        className="diff"
        ref={viewRef}
        data-testid={testId}
        style={{ alignItems: 'center', justifyContent: 'center' }}
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
        ref={viewRef}
        data-testid={testId}
        style={{ alignItems: 'center', justifyContent: 'center' }}
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
      <div className="diff" ref={viewRef} data-testid={testId}>
        <EmptyState message={emptyMessage ?? t(translate, 'diff-no-output')} testId="diff-empty" />
      </div>
    );
  }

  return (
    <div className="diff" ref={viewRef} data-testid={testId}>
      {statusMessage ? (
        <div
          className={`diff__status diff__status--${statusMessage.kind}`}
          data-testid={`diff-${statusMessage.kind === 'warning' ? 'refresh-error' : 'refreshing'}`}
        >
          {statusMessage.text}
        </div>
      ) : null}
      {header ? (
        <div className="bottom__toolbar">
          <span className="bottom__toolbar-title">{header}</span>
          <span className="bottom__toolbar-spacer" />
          {onCopy ? (
            <button
              type="button"
              className="icon-button"
              title={t(translate, 'diff-copy-tooltip')}
              aria-label={t(translate, 'diff-copy-tooltip')}
              data-testid="diff-copy"
              onClick={onCopy}
            >
              <CopyGlyph />
            </button>
          ) : null}
        </div>
      ) : null}
      <div style={{ flex: 1, minHeight: 0 }}>
        <MeasuredVirtualList
          items={items}
          estimateRowHeight={DIFF_ROW_HEIGHT}
          testId="diff-rows"
          className={`${effective === 'side-by-side' ? 'diff--split' : 'diff--inline'}${softWrap ? ' diff--soft-wrap' : ''}`}
          renderRow={(item) => (
            <DiffItem
              item={item}
              layout={effective}
              compactPreview={compactPreview}
              previewHeight={previewHeight}
              loadPreview={loadPreview}
              onSvgModeChange={updateSvgMode}
            />
          )}
        />
      </div>
    </div>
  );
}

function DiffItem({
  item,
  layout,
  compactPreview,
  previewHeight,
  loadPreview,
  onSvgModeChange
}: {
  item: Item;
  layout: 'inline' | 'side-by-side';
  compactPreview: boolean;
  previewHeight: number;
  loadPreview: (preview: NonNullable<DiffSection['imagePreview']>) => Promise<ImagePreview>;
  onSvgModeChange: (key: string, mode: 'image' | 'diff') => void;
}) {
  if (item.kind === 'header') {
    return (
      <div className="diff__file-header" data-testid="diff-file-header">
        <span>{item.path}</span>
      </div>
    );
  }
  if (item.kind === 'binary') {
    return (
      <div className="diff__note" data-testid="diff-binary">
        {useStore.getState().t('bottom-bin')}
      </div>
    );
  }
  if (item.kind === 'svg-toggle') {
    return (
      <div className="diff__image-toolbar" data-testid="svg-preview-toolbar">
        <button
          type="button"
          className={`tool-button tool-button--compact${item.mode === 'image' ? ' is-active' : ''}`}
          data-testid="svg-preview-image"
          aria-pressed={item.mode === 'image'}
          onClick={() => onSvgModeChange(item.key, 'image')}
        >
          {useStore.getState().t('diff-image-mode')}
        </button>
        <button
          type="button"
          className={`tool-button tool-button--compact${item.mode === 'diff' ? ' is-active' : ''}`}
          data-testid="svg-preview-diff"
          aria-pressed={item.mode === 'diff'}
          onClick={() => onSvgModeChange(item.key, 'diff')}
        >
          {useStore.getState().t('diff-text-mode')}
        </button>
      </div>
    );
  }
  if (item.kind === 'image') {
    return (
      <ImagePreviewRow
        section={item.section}
        compact={compactPreview}
        height={previewHeight}
        loadPreview={loadPreview}
      />
    );
  }
  if (item.kind === 'empty') {
    return (
      <div className="diff__note" data-testid="diff-empty-row">
        {useStore.getState().t('diff-no-output')}
      </div>
    );
  }
  return <DiffRowView row={item.row} document={item.document} layout={layout} />;
}

function ImagePreviewRow({
  section,
  compact,
  height,
  loadPreview
}: {
  section: DiffSection;
  compact: boolean;
  height: number;
  loadPreview: (preview: NonNullable<DiffSection['imagePreview']>) => Promise<ImagePreview>;
}) {
  const translate = useStore((state) => state.t);
  const [preview, setPreview] = useState<ImagePreview | null>(null);
  const [failedSides, setFailedSides] = useState<Record<'old' | 'new', boolean>>({
    old: false,
    new: false
  });
  const [loadFailed, setLoadFailed] = useState(false);
  const descriptor = section.imagePreview;

  useEffect(() => {
    if (!descriptor) return;
    let current = true;
    setPreview(null);
    setLoadFailed(false);
    setFailedSides({ old: false, new: false });
    void loadPreview(descriptor)
      .then((result) => {
        if (current) setPreview(result);
      })
      .catch(() => {
        if (current) setLoadFailed(true);
      });
    return () => {
      current = false;
    };
  }, [descriptor, loadPreview]);

  const renderSide = (version: 'old' | 'new') => {
    const label = t(translate, version === 'old' ? 'diff-image-before' : 'diff-image-after');
    if (preview === null) {
      if (loadFailed) {
        return (
          <div
            className="diff__image-placeholder"
            data-testid={`diff-image-${version}-unavailable`}
          >
            {t(translate, 'diff-image-unavailable')}
          </div>
        );
      }
      return (
        <div className="diff__image-placeholder" data-testid={`diff-image-${version}-loading`}>
          <Spinner size={14} /> {t(translate, 'diff-image-loading')}
        </div>
      );
    }
    const side = preview[version];
    if (side.status === 'absent') {
      return (
        <div className="diff__image-placeholder" data-testid={`diff-image-${version}-absent`}>
          {t(translate, 'diff-image-absent')}
        </div>
      );
    }
    if (side.status === 'unavailable' || failedSides[version]) {
      const reason = side.status === 'unavailable' ? side.reason : 'unreadable';
      const key =
        reason === 'tooLarge'
          ? 'diff-image-too-large'
          : reason === 'unsupported'
            ? 'diff-image-unsupported'
            : 'diff-image-unavailable';
      return (
        <div className="diff__image-placeholder" data-testid={`diff-image-${version}-unavailable`}>
          {t(translate, key)}
        </div>
      );
    }
    return (
      <img
        className="diff__image"
        data-testid={`diff-image-${version}`}
        src={`data:${side.mimeType};base64,${side.data}`}
        alt={`${section.path} ${label.toLowerCase()}`}
        draggable={false}
        onError={() => setFailedSides((current) => ({ ...current, [version]: true }))}
      />
    );
  };

  return (
    <div
      className={`diff__image-preview${compact ? ' diff__image-preview--stacked' : ''}`}
      style={{ height }}
      data-testid="diff-image-preview"
    >
      {(['old', 'new'] as const).map((version) => (
        <div className="diff__image-side" key={version}>
          <div className="diff__image-label">
            {t(translate, version === 'old' ? 'diff-image-before' : 'diff-image-after')}
          </div>
          {renderSide(version)}
        </div>
      ))}
    </div>
  );
}

function DiffRowView({
  row,
  document,
  layout
}: {
  row: DiffRow;
  document: DiffPayload;
  layout: 'inline' | 'side-by-side';
}) {
  const ranges = {
    old: document.inline_old,
    new: document.inline_new
  };

  if (row.kind === 'hunk') {
    return (
      <div className="diff__row diff__row--hunk" data-testid="diff-hunk">
        <span className="diff__hunk">{row.hunk_header}</span>
      </div>
    );
  }

  if (layout === 'side-by-side') {
    const oldModifier = row.old_text === null ? '' : row.kind === 'del' ? ' diff__side--del' : '';
    const newModifier = row.new_text === null ? '' : row.kind === 'add' ? ' diff__side--add' : '';
    return (
      <div className="diff__row diff__row--split" data-testid="diff-row">
        <div className={`diff__side diff__side--old${oldModifier}`}>
          <span className="diff__gutter">{row.old_no ?? ''}</span>
          <span className="diff__text diff__text--old">
            {row.old_text === null
              ? ''
              : highlight(row.old_text, document.language, oldRanges(ranges, row))}
          </span>
        </div>
        <div className={`diff__side diff__side--new${newModifier}`}>
          <span className="diff__gutter">{row.new_no ?? ''}</span>
          <span className="diff__text diff__text--new">
            {row.new_text === null
              ? ''
              : highlight(row.new_text, document.language, newRanges(ranges, row))}
          </span>
        </div>
      </div>
    );
  }

  const newSide = row.kind === 'add' || (row.kind === 'context' && row.new_text !== null);
  const text = newSide ? row.new_text : row.old_text;
  const modifier = row.kind === 'add' ? 'add' : row.kind === 'del' ? 'del' : 'context';
  return (
    <div className={`diff__row diff__row--${modifier}`} data-testid="diff-row">
      <span className="diff__gutter">{row.old_no ?? ''}</span>
      <span className="diff__gutter">{row.new_no ?? ''}</span>
      <span className={`diff__marker${newSide ? ' diff__marker--new' : ' diff__marker--old'}`}>
        {row.kind === 'add' ? '+' : row.kind === 'del' ? '-' : ' '}
      </span>
      <span className="diff__text">
        {text === null || text === undefined
          ? ''
          : highlight(
              text,
              document.language,
              newSide ? newRanges(ranges, row) : oldRanges(ranges, row)
            )}
      </span>
    </div>
  );
}

function oldRanges(
  ranges: { old: CharRange[][]; new: CharRange[][] },
  row: DiffRow
): CharRange[] | null {
  if (row.old_line_index === null) {
    return null;
  }
  return ranges.old?.[row.old_line_index] ?? null;
}

function newRanges(
  ranges: { old: CharRange[][]; new: CharRange[][] },
  row: DiffRow
): CharRange[] | null {
  if (row.new_line_index === null) {
    return null;
  }
  return ranges.new?.[row.new_line_index] ?? null;
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
  ranges: CharRange[] | null
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
        mark: false
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
