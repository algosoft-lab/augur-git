/**
 * The repository tab bar.
 *
 * A tab's state is derived from its repository: loading while the first status
 * snapshot has not arrived, an error when the repository could not be opened,
 * ready otherwise. The indicator is a 6px dot so the tab label stays readable.
 */

import { useEffect, useRef, useState, type PointerEvent } from 'react';

import { Icon } from '../../components/Icon';
import { useStore } from '../../app/store';
import { t } from '../../i18n/strings';
import { IS_MACOS } from './WindowControls';

interface DropTarget {
  key: string;
  placement: 'before' | 'after';
}

interface DragSession {
  pointerId: number;
  sourceKey: string;
  startX: number;
  startY: number;
  pointerX: number;
  pointerY: number;
  dragging: boolean;
  cancelled: boolean;
  captureTarget: HTMLDivElement;
}

export function TabBar({ onNewTab }: { onNewTab: () => void }) {
  const translate = useStore((state) => state.t);
  const tabs = useStore((state) => state.tabs);
  const activeTabKey = useStore((state) => state.activeTabKey);
  const repos = useStore((state) => state.repos);
  const selectTab = useStore((state) => state.selectTab);
  const closeTab = useStore((state) => state.closeTab);
  const reorderTab = useStore((state) => state.reorderTab);
  const tabBarRef = useRef<HTMLDivElement>(null);
  const dragSessionRef = useRef<DragSession | null>(null);
  const dragFrameRef = useRef<number | null>(null);
  const suppressedClickRef = useRef<{ key: string; timer: number } | null>(null);
  const [dragPreview, setDragPreview] = useState<{
    sourceKey: string;
    target: DropTarget | null;
  } | null>(null);

  const suppressNextClick = (key: string) => {
    if (suppressedClickRef.current) {
      window.clearTimeout(suppressedClickRef.current.timer);
    }
    const timer = window.setTimeout(() => {
      if (suppressedClickRef.current?.key === key) {
        suppressedClickRef.current = null;
      }
    }, 0);
    suppressedClickRef.current = { key, timer };
  };

  const resolveDropTarget = (sourceKey: string, x: number, y: number): DropTarget | null => {
    const bar = tabBarRef.current;
    if (!bar) {
      return null;
    }
    const barBounds = bar.getBoundingClientRect();
    if (x < barBounds.left || x > barBounds.right || y < barBounds.top || y > barBounds.bottom) {
      return null;
    }

    const candidates = Array.from(bar.querySelectorAll<HTMLElement>('.tab')).filter(
      (candidate) => candidate.dataset.tabKey !== sourceKey
    );
    for (const candidate of candidates) {
      const bounds = candidate.getBoundingClientRect();
      if (x < bounds.left + bounds.width / 2) {
        return { key: candidate.dataset.tabKey!, placement: 'before' };
      }
    }
    const last = candidates[candidates.length - 1];
    return last ? { key: last.dataset.tabKey!, placement: 'after' } : null;
  };

  const updateDragPreview = (session: DragSession) => {
    const target = resolveDropTarget(session.sourceKey, session.pointerX, session.pointerY);
    setDragPreview((current) => {
      if (
        current?.sourceKey === session.sourceKey &&
        current.target?.key === target?.key &&
        current.target?.placement === target?.placement
      ) {
        return current;
      }
      return { sourceKey: session.sourceKey, target };
    });
  };

  const stopAutoScroll = () => {
    if (dragFrameRef.current !== null) {
      window.cancelAnimationFrame(dragFrameRef.current);
      dragFrameRef.current = null;
    }
  };

  const startAutoScroll = () => {
    if (dragFrameRef.current !== null) {
      return;
    }
    const tick = () => {
      dragFrameRef.current = null;
      const session = dragSessionRef.current;
      const bar = tabBarRef.current;
      if (!session?.dragging || !bar) {
        return;
      }

      const bounds = bar.getBoundingClientRect();
      let delta = 0;
      let scrolled = false;
      const edgeSize = 36;
      if (
        session.pointerY >= bounds.top &&
        session.pointerY <= bounds.bottom &&
        session.pointerX >= bounds.left &&
        session.pointerX <= bounds.right
      ) {
        if (session.pointerX < bounds.left + edgeSize) {
          delta = -Math.min(
            14,
            Math.max(2, Math.ceil((bounds.left + edgeSize - session.pointerX) / 4))
          );
        } else if (session.pointerX > bounds.right - edgeSize) {
          delta = Math.min(
            14,
            Math.max(2, Math.ceil((session.pointerX - (bounds.right - edgeSize)) / 4))
          );
        }
      }
      if (delta !== 0) {
        const previous = bar.scrollLeft;
        bar.scrollLeft += delta;
        if (bar.scrollLeft !== previous) {
          updateDragPreview(session);
          scrolled = true;
        }
      }
      if (scrolled) {
        dragFrameRef.current = window.requestAnimationFrame(tick);
      }
    };
    dragFrameRef.current = window.requestAnimationFrame(tick);
  };

  const cancelDrag = (releaseCapture: boolean) => {
    const session = dragSessionRef.current;
    dragSessionRef.current = null;
    stopAutoScroll();
    setDragPreview(null);
    if (releaseCapture && session?.captureTarget.hasPointerCapture(session.pointerId)) {
      session.captureTarget.releasePointerCapture(session.pointerId);
    }
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const session = dragSessionRef.current;
      if (event.key === 'Escape' && session?.dragging) {
        event.preventDefault();
        session.cancelled = true;
        stopAutoScroll();
        setDragPreview(null);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      cancelDrag(true);
      if (suppressedClickRef.current) {
        window.clearTimeout(suppressedClickRef.current.timer);
      }
    };
  }, []);

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>, key: string) => {
    if (
      event.button !== 0 ||
      !event.isPrimary ||
      dragSessionRef.current ||
      (event.target instanceof Element && event.target.closest('button'))
    ) {
      return;
    }

    if (suppressedClickRef.current) {
      window.clearTimeout(suppressedClickRef.current.timer);
      suppressedClickRef.current = null;
    }
    dragSessionRef.current = {
      pointerId: event.pointerId,
      sourceKey: key,
      startX: event.clientX,
      startY: event.clientY,
      pointerX: event.clientX,
      pointerY: event.clientY,
      dragging: false,
      cancelled: false,
      captureTarget: event.currentTarget
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) {
      return;
    }
    if (session.cancelled) {
      return;
    }
    session.pointerX = event.clientX;
    session.pointerY = event.clientY;
    if (
      !session.dragging &&
      Math.hypot(event.clientX - session.startX, event.clientY - session.startY) < 5
    ) {
      return;
    }

    session.dragging = true;
    event.preventDefault();
    updateDragPreview(session);
    startAutoScroll();
  };

  const handlePointerUp = (event: PointerEvent<HTMLDivElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) {
      return;
    }
    if (session.cancelled) {
      suppressNextClick(session.sourceKey);
      cancelDrag(false);
      return;
    }
    session.pointerX = event.clientX;
    session.pointerY = event.clientY;
    if (session.dragging) {
      event.preventDefault();
      const target = resolveDropTarget(session.sourceKey, event.clientX, event.clientY);
      if (target) {
        reorderTab(session.sourceKey, target.key, target.placement);
      }
      suppressNextClick(session.sourceKey);
    }
    cancelDrag(false);
  };

  const handlePointerCancel = (event: PointerEvent<HTMLDivElement>) => {
    if (dragSessionRef.current?.pointerId === event.pointerId) {
      cancelDrag(false);
    }
  };

  return (
    <div
      ref={tabBarRef}
      className={`tab-bar${dragPreview ? ' is-reordering' : ''}`}
      role="tablist"
      data-testid="tab-bar"
      {...(IS_MACOS ? { 'data-tauri-drag-region': true } : {})}
    >
      {tabs.map((tab) => {
        const repo = tab.repoId === null ? undefined : repos[tab.repoId];
        const state = repo?.status ?? 'loading';
        // A start page has no repository, so it is named for what it offers.
        const label =
          tab.repoId === null
            ? t(translate, 'tab-new')
            : tab.location.kind === 'wsl'
              ? `${tab.location.distro} · ${basename(tab.path)}`
              : basename(tab.path);
        const isDragging = dragPreview?.sourceKey === tab.key;
        const isDropBefore =
          dragPreview?.target?.key === tab.key && dragPreview.target.placement === 'before';
        const isDropAfter =
          dragPreview?.target?.key === tab.key && dragPreview.target.placement === 'after';
        return (
          <div
            key={tab.key}
            role="tab"
            aria-selected={tab.key === activeTabKey}
            className={[
              'tab',
              tab.key === activeTabKey ? 'is-active' : '',
              isDragging ? 'is-dragging' : '',
              isDropBefore ? 'is-drop-before' : '',
              isDropAfter ? 'is-drop-after' : ''
            ]
              .filter(Boolean)
              .join(' ')}
            data-testid={`tab-${tab.key}`}
            data-tab-key={tab.key}
            onPointerDown={(event) => handlePointerDown(event, tab.key)}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerCancel}
            onLostPointerCapture={handlePointerCancel}
            onClick={() => {
              if (suppressedClickRef.current?.key === tab.key) {
                window.clearTimeout(suppressedClickRef.current.timer);
                suppressedClickRef.current = null;
                return;
              }
              void selectTab(tab.key);
            }}
            onAuxClick={(event) => {
              // Middle click closes, matching the reference application.
              if (event.button === 1) {
                event.preventDefault();
                void closeTab(tab.key);
              }
            }}
            title={tab.repoId === null ? t(translate, 'tab-new') : tab.path}
          >
            <span className={`tab__dot tab__dot--${state}`} />
            <span className="tab__label">{label}</span>
            <button
              type="button"
              className="tab__close"
              // The hover hint says what the button does to this tab; the
              // accessible name stays the short label.
              title={t(translate, 'tab-close-hint')}
              aria-label={t(translate, 'tab-close')}
              data-testid={`tab-close-${tab.key}`}
              onClick={(event) => {
                event.stopPropagation();
                void closeTab(tab.key);
              }}
            >
              <Icon name="x" size={11} />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        className="tab-bar__new"
        title={t(translate, 'tab-new')}
        aria-label={t(translate, 'tab-new')}
        data-testid="tab-new"
        onClick={onNewTab}
      >
        <Icon name="plus" size={13} />
      </button>
    </div>
  );
}

/** Last path segment, with a fallback for a filesystem root. */
function basename(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
