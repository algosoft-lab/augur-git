import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';

export interface MeasuredVirtualListProps<T> {
  items: T[];
  estimateRowHeight: number;
  overscan?: number;
  renderRow: (item: T, index: number) => ReactNode;
  onViewportChange?: (range: { start: number; end: number }) => void;
  onScrollPosition?: (scrollTop: number) => void;
  className?: string;
  testId?: string;
}

/** A virtual list whose visible rows can grow beyond the estimated height. */
export function MeasuredVirtualList<T>({
  items,
  estimateRowHeight,
  overscan = 8,
  renderRow,
  onViewportChange,
  onScrollPosition,
  className,
  testId
}: MeasuredVirtualListProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const observerRef = useRef<ResizeObserver | null>(null);
  const index = useMemo(
    () => new MeasuredHeightIndex(items.length, estimateRowHeight),
    [items, estimateRowHeight]
  );
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(0);
  const [revision, setRevision] = useState(0);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const viewportObserver = new ResizeObserver(() => setHeight(container.clientHeight));
    viewportObserver.observe(container);
    setHeight(container.clientHeight);
    return () => viewportObserver.disconnect();
  }, []);

  useLayoutEffect(() => {
    const observer = new ResizeObserver((entries) => {
      const container = containerRef.current;
      if (!container) {
        return;
      }
      const anchor = index.findIndex(container.scrollTop);
      let anchorShift = 0;
      let changed = false;
      for (const entry of entries) {
        const row = entry.target as HTMLElement;
        const rowIndex = Number(row.dataset.virtualIndex);
        if (!Number.isInteger(rowIndex) || rowIndex < 0 || rowIndex >= items.length) {
          continue;
        }
        const measuredHeight = Math.max(estimateRowHeight, row.getBoundingClientRect().height);
        const delta = index.setHeight(rowIndex, measuredHeight);
        if (delta !== 0) {
          changed = true;
          if (rowIndex < anchor) {
            anchorShift += delta;
          }
        }
      }
      if (changed) {
        if (anchorShift !== 0) {
          container.scrollTop += anchorShift;
          setScrollTop(container.scrollTop);
        }
        setRevision((value) => value + 1);
      }
    });
    observerRef.current = observer;
    return () => {
      observer.disconnect();
      observerRef.current = null;
    };
  }, [estimateRowHeight, index, items.length]);

  const totalHeight = index.totalHeight;
  const start = items.length ? Math.max(0, index.findIndex(scrollTop) - overscan) : 0;
  let end = start;
  const viewportBottom = scrollTop + height;
  while (end < items.length && index.offsetOf(end) < viewportBottom) {
    end += 1;
  }
  end = Math.min(items.length, end + overscan);

  useLayoutEffect(() => {
    const observer = observerRef.current;
    const container = containerRef.current;
    if (!observer || !container) {
      return;
    }
    observer.disconnect();
    for (const row of container.querySelectorAll<HTMLElement>('[data-virtual-index]')) {
      observer.observe(row);
    }
  }, [start, end, items, revision]);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const maxScrollTop = Math.max(0, totalHeight - container.clientHeight);
    if (container.scrollTop > maxScrollTop) {
      container.scrollTop = maxScrollTop;
      setScrollTop(maxScrollTop);
    }
  }, [height, totalHeight]);

  useEffect(() => {
    onViewportChange?.({ start, end });
  }, [start, end, onViewportChange]);

  return (
    <div
      className={`virtual-list${className ? ` ${className}` : ''}`}
      ref={containerRef}
      data-testid={testId}
      onScroll={(event) => {
        const next = event.currentTarget.scrollTop;
        setScrollTop(next);
        onScrollPosition?.(next);
      }}
    >
      <div className="virtual-list__sizer" style={{ height: totalHeight }}>
        <div
          className="virtual-list__window"
          style={{ transform: `translateY(${index.offsetOf(start)}px)` }}
        >
          {items.slice(start, end).map((item, offset) => {
            const itemIndex = start + offset;
            return (
              <div
                key={itemIndex}
                className="virtual-list__measured-row"
                data-virtual-index={itemIndex}
                style={{ minHeight: estimateRowHeight }}
              >
                {renderRow(item, itemIndex)}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

class MeasuredHeightIndex {
  private readonly values: number[];
  private readonly tree: number[];

  constructor(
    private readonly size: number,
    private readonly estimate: number
  ) {
    this.values = new Array(size).fill(estimate);
    this.tree = new Array(size + 1).fill(0);
    for (let index = 1; index <= size; index += 1) {
      this.tree[index] = estimate * (index & -index);
    }
  }

  get totalHeight(): number {
    return this.offsetOf(this.size);
  }

  offsetOf(count: number): number {
    let sum = 0;
    for (let index = Math.min(this.size, count); index > 0; index -= index & -index) {
      sum += this.tree[index]!;
    }
    return sum;
  }

  findIndex(offset: number): number {
    if (this.size === 0) {
      return 0;
    }
    let index = 0;
    let sum = 0;
    let bit = 1;
    while (bit * 2 <= this.size) {
      bit *= 2;
    }
    for (; bit > 0; bit = Math.floor(bit / 2)) {
      const next = index + bit;
      if (next <= this.size && sum + this.tree[next]! <= offset) {
        index = next;
        sum += this.tree[next]!;
      }
    }
    return Math.min(index, this.size - 1);
  }

  setHeight(row: number, height: number): number {
    const next = Math.max(this.estimate, height);
    const delta = next - this.values[row]!;
    if (delta === 0) {
      return 0;
    }
    this.values[row] = next;
    for (let index = row + 1; index <= this.size; index += index & -index) {
      this.tree[index] = this.tree[index]! + delta;
    }
    return delta;
  }
}
