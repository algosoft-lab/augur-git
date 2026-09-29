/**
 * Small presentational primitives shared by every panel.
 *
 * The reference application uses compact ghost buttons with a 12px label and a
 * 14px muted icon, 22px square icon buttons in lists, and dense 22–24px rows.
 * These components encode those measurements so the individual panels stay
 * about behavior rather than styling.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

import { Icon } from './Icon';
import './controls.css';

export type ButtonVariant = 'ghost' | 'primary' | 'danger';

export interface ToolButtonProps {
  label?: string;
  icon?: ReactNode;
  tooltip?: string;
  variant?: ButtonVariant;
  compact?: boolean;
  disabled?: boolean;
  active?: boolean;
  children?: ReactNode;
  onClick?: () => void;
  title?: string;
  testId?: string;
}

/** A labeled toolbar button: 14px muted icon plus a 12px label. */
export function ToolButton({
  label,
  icon,
  tooltip,
  variant = 'ghost',
  compact = false,
  disabled = false,
  active = false,
  children,
  onClick,
  title,
  testId
}: ToolButtonProps) {
  return (
    <button
      type="button"
      data-testid={testId}
      className={[
        'tool-button',
        `tool-button--${variant}`,
        compact ? 'tool-button--compact' : '',
        active ? 'is-active' : '',
        disabled ? 'is-disabled' : ''
      ]
        .filter(Boolean)
        .join(' ')}
      disabled={disabled}
      title={title ?? tooltip}
      aria-label={tooltip ?? label}
      onClick={onClick}
    >
      {icon ? <span className="tool-button__icon">{icon}</span> : null}
      {label ? <span className="tool-button__label">{label}</span> : null}
      {children}
    </button>
  );
}

export interface IconButtonProps {
  icon: ReactNode;
  tooltip: string;
  disabled?: boolean;
  visible?: boolean;
  onClick?: (event: React.MouseEvent) => void;
  testId?: string;
}

/** A 22px square icon button used inside list rows. */
export function IconButton({
  icon,
  tooltip,
  disabled = false,
  visible = true,
  onClick,
  testId
}: IconButtonProps) {
  return (
    <span className={`icon-button-slot${visible ? '' : ' is-hidden'}`}>
      <button
        type="button"
        data-testid={testId}
        className="icon-button"
        title={tooltip}
        aria-label={tooltip}
        disabled={disabled}
        onMouseDown={(event) => {
          // Keep the row's own click from also firing.
          event.preventDefault();
          event.stopPropagation();
        }}
        onClick={(event) => {
          event.stopPropagation();
          onClick?.(event);
        }}
      >
        {icon}
      </button>
    </span>
  );
}

export interface SpinnerProps {
  size?: number;
  color?: string;
  label?: string;
  rhythm?: 'continuous' | 'two-turn-pause';
}

/** The animated busy indicator used in the toolbar and the status bar. */
export function Spinner({ size = 13, color, label, rhythm = 'continuous' }: SpinnerProps) {
  return (
    <span
      className={`spinner${rhythm === 'two-turn-pause' ? ' spinner--two-turn-pause' : ''}`}
      role="status"
      aria-label={label}
      style={{ width: size, height: size, borderTopColor: color }}
    />
  );
}

export interface TextInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  monospace?: boolean;
  onSubmit?: () => void;
  onEscape?: () => void;
  /**
   * The Up and Down arrows, for a control that opens a list. `preventDefault` is
   * applied so the caret does not jump, which is what makes the list usable from
   * the keyboard alone.
   */
  onArrow?: (direction: 'up' | 'down') => void;
  testId?: string;
  prefix?: ReactNode;
  cleanable?: boolean;
  size?: 'small' | 'normal';
  disabled?: boolean;
}

/** A text field that reports Enter and Escape so dialogs can bind them. */
export function TextInput({
  value,
  onChange,
  placeholder,
  autoFocus,
  monospace,
  onSubmit,
  onEscape,
  onArrow,
  testId,
  prefix,
  cleanable,
  size = 'normal',
  disabled
}: TextInputProps) {
  return (
    <span
      className={`text-input${size === 'small' ? ' text-input--small' : ''}${
        monospace ? ' text-input--mono' : ''
      }`}
    >
      {prefix ? <span className="text-input__prefix">{prefix}</span> : null}
      <input
        data-testid={testId}
        type="text"
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        autoFocus={autoFocus}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && onSubmit) {
            event.preventDefault();
            onSubmit();
          } else if (event.key === 'Escape' && onEscape) {
            event.preventDefault();
            event.stopPropagation();
            onEscape();
          } else if (onArrow && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
            event.preventDefault();
            onArrow(event.key === 'ArrowDown' ? 'down' : 'up');
          }
        }}
      />
      {cleanable && value ? (
        <button
          type="button"
          className="text-input__clear"
          aria-label="clear"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => onChange('')}
        >
          ×
        </button>
      ) : null}
    </span>
  );
}

export interface TextAreaProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  minRows?: number;
  maxRows?: number;
  onSubmit?: () => void;
  onEscape?: () => void;
  /**
   * The Up and Down arrows, for a control that opens a list. `preventDefault` is
   * applied so the caret does not jump, which is what makes the list usable from
   * the keyboard alone.
   */
  onArrow?: (direction: 'up' | 'down') => void;
  testId?: string;
}

/** The commit message editor: Enter submits, Shift+Enter inserts a newline. */
export function TextArea({
  value,
  onChange,
  placeholder,
  disabled,
  minRows = 2,
  maxRows = 5,
  onSubmit,
  onEscape,
  testId
}: TextAreaProps) {
  return (
    <textarea
      data-testid={testId}
      className="text-area"
      value={value}
      placeholder={placeholder}
      disabled={disabled}
      rows={minRows}
      spellCheck={false}
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && !event.shiftKey && onSubmit) {
          event.preventDefault();
          onSubmit();
        } else if (event.key === 'Escape' && onEscape) {
          event.preventDefault();
          event.stopPropagation();
          onEscape();
        }
      }}
      style={{ minHeight: `${minRows * 20}px`, maxHeight: `${maxRows * 20}px` }}
    />
  );
}

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
  testId?: string;
}

export function Checkbox({ checked, onChange, label, disabled, testId }: CheckboxProps) {
  return (
    <label className={`checkbox${disabled ? ' is-disabled' : ''}`}>
      <input
        data-testid={testId}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span>{label}</span>
    </label>
  );
}

/** A stable test id for an option, derived from its visible label. */
function slug(label: string): string {
  return label.trim().toLowerCase().replace(/\s+/g, '-');
}

export interface SelectOption<T> {
  value: T;
  label: string;
  group?: string;
}

/** A compact select matching the settings controls. */
export function Select<T extends string | number | boolean>({
  value,
  options,
  onChange,
  searchable = false,
  searchPlaceholder,
  testId,
  allowCustomValue = false
}: {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  searchable?: boolean;
  /** What the filter field says, since a list of fonts needs saying. */
  searchPlaceholder?: string;
  testId?: string;
  /** Save the exact search text when no installed option matches. */
  allowCustomValue?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.value === value);

  useEffect(() => {
    if (!open) {
      setQuery('');
      return;
    }
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', onPointerDown);
    return () => document.removeEventListener('mousedown', onPointerDown);
  }, [open]);

  const filtered = query
    ? options.filter((option) =>
        `${option.group ?? ''} ${option.label}`.toLowerCase().includes(query.toLowerCase())
      )
    : options;
  const groupedOptions = filtered.reduce<Array<{ name: string; options: SelectOption<T>[] }>>(
    (groups, option) => {
      const name = option.group ?? '';
      const last = groups[groups.length - 1];
      if (last?.name === name) {
        last.options.push(option);
      } else {
        groups.push({ name, options: [option] });
      }
      return groups;
    },
    []
  );

  return (
    <div className="select" ref={rootRef}>
      <button
        type="button"
        data-testid={testId}
        className="select__trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <span className="select__value">
          {selected?.label ?? (typeof value === 'string' ? value : '')}
        </span>
        <span className="select__caret">▾</span>
      </button>
      {open ? (
        <div className="select__menu" role="listbox">
          {searchable ? (
            <div className="select__search">
              <input
                type="text"
                value={query}
                autoFocus
                aria-label={searchPlaceholder}
                placeholder={searchPlaceholder ?? ''}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (
                    event.key === 'Enter' &&
                    allowCustomValue &&
                    query.trim() &&
                    !options.some((option) => option.value === query.trim())
                  ) {
                    event.preventDefault();
                    onChange(query.trim() as T);
                    setOpen(false);
                    setQuery('');
                  }
                }}
              />
            </div>
          ) : null}
          {groupedOptions.map((group, groupIndex) => (
            <div
              key={`${group.name}-${groupIndex}`}
              role={group.name ? 'group' : undefined}
              aria-label={group.name || undefined}
            >
              {group.name ? (
                <div className="select__group-label" role="presentation">
                  {group.name}
                </div>
              ) : null}
              {group.options.map((option) => (
                <button
                  key={String(option.value)}
                  type="button"
                  role="option"
                  data-testid={`select-option-${slug(option.label)}`}
                  aria-selected={option.value === value}
                  className={`select__option${option.value === value ? ' is-selected' : ''}`}
                  onClick={() => {
                    onChange(option.value);
                    setOpen(false);
                    setQuery('');
                  }}
                >
                  {option.label}
                </button>
              ))}
            </div>
          ))}
          {filtered.length === 0 ? <div className="select__empty">—</div> : null}
        </div>
      ) : null}
    </div>
  );
}

export interface SliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  testId?: string;
}

export function Slider({ value, min, max, step = 1, onChange, testId }: SliderProps) {
  return (
    <input
      data-testid={testId}
      className="slider"
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(event) => onChange(Number(event.target.value))}
    />
  );
}

export interface MenuItemSpec {
  id: string;
  label: string;
  children?: MenuItemSpec[];
  icon?: ReactNode;
  disabled?: boolean;
  danger?: boolean;
  /**
   * Whether this item is the current choice of a set.
   *
   * A menu that offers two modes has to say which one is in effect; without the
   * mark, opening the menu and reading it tells you nothing about the setting.
   */
  checked?: boolean;
  separatorBefore?: boolean;
  onSelect?: () => void;
}

export interface MenuProps {
  items: MenuItemSpec[];
  align?: 'start' | 'end';
  testId?: string;
  children?: ReactNode;
}

/** A dropdown menu. The trigger is supplied as the single child. */
export function Menu({ items, align = 'start', testId, children }: MenuProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{ x: number; y: number } | null>(null);
  const [openPath, setOpenPath] = useState<string[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const close = useCallback(() => {
    setOpen(false);
    setOpenPath([]);
  }, []);

  const toggle = () => {
    if (open) {
      close();
      return;
    }
    const rect = rootRef.current?.getBoundingClientRect();
    if (!rect) {
      return;
    }
    setOpenPath([]);
    setOpen(true);
    setPosition({ x: rect.left, y: rect.bottom + 2 });
  };

  useEffect(() => {
    if (!open) {
      return;
    }
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !menuRef.current?.contains(target)) {
        close();
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        if (openPath.length > 0) {
          setOpenPath((current) => current.slice(0, -1));
        } else {
          close();
        }
      }
    };
    // The list is fixed-positioned once portaled out of the trigger, so any
    // scroll or resize would leave it anchored to a stale spot; closing is
    // simpler and matches the context menu.
    const reposition = () => close();
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    window.addEventListener('scroll', reposition, true);
    window.addEventListener('resize', reposition);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('scroll', reposition, true);
      window.removeEventListener('resize', reposition);
    };
  }, [open, openPath.length, close]);

  // The list's real size is only known once it has mounted, so alignment and
  // the viewport clamp run after layout: keep the aligned edge on the trigger,
  // flip up at the bottom edge, and pull the left edge inside the window.
  // Derive everything from the live trigger rect rather than the stored
  // position: positioning the end-aligned list from the stored value would
  // subtract the list width again on every rerun and walk the menu to the
  // window's left edge.
  useLayoutEffect(() => {
    if (!position || !menuRef.current) {
      return;
    }
    const rect = menuRef.current.getBoundingClientRect();
    const trigger = rootRef.current?.getBoundingClientRect();
    if (!trigger) {
      return;
    }
    const margin = 8;
    let x = align === 'end' ? trigger.right - rect.width : trigger.left;
    let y = trigger.bottom + 2;
    if (y + rect.height > window.innerHeight - margin) {
      y = Math.max(margin, trigger.top - rect.height - 6);
    }
    if (x + rect.width > window.innerWidth - margin) {
      x = window.innerWidth - rect.width - margin;
    }
    x = Math.max(margin, x);
    if (x !== position.x || y !== position.y) {
      setPosition({ x, y });
    }
  }, [position, align]);

  const renderItems = (entries: MenuItemSpec[], prefix: string, parents: string[] = []) =>
    entries.map((item) => (
      <div
        key={item.id}
        className={`menu__group${item.children ? ' menu__group--submenu' : ''}`}
        onMouseEnter={() => {
          if (item.children) {
            setOpenPath((current) => [...current.slice(0, parents.length), item.id]);
          } else {
            setOpenPath((current) => current.slice(0, parents.length));
          }
        }}
      >
        {item.separatorBefore ? <div className="menu__separator" /> : null}
        {item.children ? (
          <>
            <button
              type="button"
              role="menuitem"
              aria-haspopup="menu"
              aria-expanded={openPath[parents.length] === item.id}
              data-testid={`${prefix}-${item.id}`}
              className="menu__item menu__item--submenu"
              disabled={item.disabled}
              onClick={() =>
                setOpenPath((current) => [...current.slice(0, parents.length), item.id])
              }
              onKeyDown={(event) => {
                if (event.key === 'ArrowRight') {
                  event.preventDefault();
                  setOpenPath((current) => [...current.slice(0, parents.length), item.id]);
                } else if (event.key === 'ArrowLeft') {
                  event.preventDefault();
                  setOpenPath((current) => current.slice(0, parents.length));
                }
              }}
            >
              {item.icon ? <span className="menu__icon">{item.icon}</span> : null}
              <span>{item.label}</span>
              <Icon name="chevron-right" size={11} className="menu__submenu-arrow" />
            </button>
            {openPath[parents.length] === item.id ? (
              <div
                className="menu__submenu"
                role="menu"
                data-testid={`${prefix}-${item.id}-submenu`}
              >
                {renderItems(item.children, prefix, [...parents, item.id])}
              </div>
            ) : null}
          </>
        ) : (
          <button
            type="button"
            role="menuitem"
            data-testid={`${prefix}-${item.id}`}
            className={[
              'menu__item',
              item.disabled ? 'is-disabled' : '',
              item.danger ? 'is-danger' : ''
            ]
              .filter(Boolean)
              .join(' ')}
            disabled={item.disabled}
            onClick={() => {
              setOpen(false);
              setOpenPath([]);
              item.onSelect?.();
            }}
          >
            {item.icon ? <span className="menu__icon">{item.icon}</span> : null}
            <span>{item.label}</span>
            {item.checked ? <Icon name="check" size={12} className="menu__check" /> : null}
          </button>
        )}
      </div>
    ));

  return (
    <div className={`menu${align === 'end' ? ' menu--end' : ''}`} ref={rootRef}>
      <span
        onClick={toggle}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            toggle();
          }
        }}
        role="button"
        tabIndex={0}
        aria-haspopup="menu"
        aria-expanded={open}
        data-testid={testId ? `${testId}-trigger` : undefined}
      >
        {children}
      </span>
      {open && position
        ? createPortal(
            // Portaled out of the trigger because hosts like the toolbar are
            // scroll containers, and a scroll container clips every absolute
            // descendant: the menu would open into an invisible strip.
            <div
              className="menu__list menu__list--fixed"
              role="menu"
              data-testid={testId}
              ref={menuRef}
              style={{ left: position.x, top: position.y }}
            >
              {renderItems(items, testId ?? 'menu')}
            </div>,
            document.body
          )
        : null}
    </div>
  );
}

export interface DialogCardProps {
  title: ReactNode;
  icon?: ReactNode;
  body: ReactNode;
  footer: ReactNode;
  onBackdrop?: () => void;
  testId?: string;
  width?: number;
}

/** A centered card used by every overlay dialog. */
export function DialogCard({
  title,
  icon,
  body,
  footer,
  onBackdrop,
  testId,
  width
}: DialogCardProps) {
  return (
    <div
      className="overlay"
      data-testid={testId ? `${testId}-overlay` : undefined}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onBackdrop?.();
        }
      }}
    >
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        data-testid={testId}
        style={width ? { width } : undefined}
      >
        <div className="dialog__title">
          {icon ? <span className="dialog__icon">{icon}</span> : null}
          <span>{title}</span>
        </div>
        <div className="dialog__body">{body}</div>
        <div className="dialog__footer">{footer}</div>
      </div>
    </div>
  );
}

export interface ContextMenuEntry {
  id: string;
  label: string;
  icon?: ReactNode;
  disabled?: boolean;
  separatorBefore?: boolean;
  onSelect: () => void;
}

export interface ContextMenuProps {
  entries: ContextMenuEntry[];
  children: ReactNode;
  testId?: string;
  /** Reports each open and close, so a host can retire overlays that share the menu's cursor anchor. */
  onOpenChange?: (open: boolean) => void;
}

/**
 * A right-click menu. Long-press opens it too, for touch input.
 *
 * The menu portals to the document body: rows live inside the virtual list's
 * transformed window, and a transformed ancestor is the containing block for
 * `position: fixed`, which used to displace the menu by the scroll depth.
 */
export function ContextMenu({ entries, children, testId, onOpenChange }: ContextMenuProps) {
  const [position, setPosition] = useState<{ x: number; y: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const holdTimer = useRef<number | null>(null);
  // Kept in a ref so `close` stays referentially stable for the effect that
  // subscribes the document listeners; hosts pass a fresh closure every render.
  const openChangeRef = useRef(onOpenChange);
  openChangeRef.current = onOpenChange;

  const close = useCallback(() => {
    setPosition(null);
    openChangeRef.current?.(false);
  }, []);

  useEffect(() => {
    if (!position) {
      return;
    }
    const onPointerDown = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        close();
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        close();
      }
    };
    const onScroll = () => close();
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onScroll);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onScroll);
    };
  }, [position, close]);

  const open = (x: number, y: number) => {
    setPosition({ x, y });
    openChangeRef.current?.(true);
  };

  // The menu's real size is only known once it has mounted, so the clamp runs
  // after layout: shift at the right edge and flip up at the bottom edge, the
  // way a native menu does.
  useLayoutEffect(() => {
    if (!position || !menuRef.current) {
      return;
    }
    const rect = menuRef.current.getBoundingClientRect();
    const margin = 8;
    const x =
      position.x + rect.width > window.innerWidth - margin
        ? Math.max(margin, window.innerWidth - rect.width - margin)
        : position.x;
    const y =
      position.y + rect.height > window.innerHeight - margin
        ? Math.max(margin, window.innerHeight - rect.height - margin)
        : position.y;
    if (x !== position.x || y !== position.y) {
      setPosition({ x, y });
    }
  }, [position]);

  return (
    <>
      <div
        data-testid={testId}
        onContextMenu={(event) => {
          event.preventDefault();
          open(event.clientX, event.clientY);
        }}
        onTouchStart={(event) => {
          const touch = event.touches[0];
          if (!touch) {
            return;
          }
          holdTimer.current = window.setTimeout(() => open(touch.clientX, touch.clientY), 500);
        }}
        onTouchEnd={() => {
          if (holdTimer.current !== null) {
            window.clearTimeout(holdTimer.current);
            holdTimer.current = null;
          }
        }}
        onTouchMove={() => {
          if (holdTimer.current !== null) {
            window.clearTimeout(holdTimer.current);
            holdTimer.current = null;
          }
        }}
      >
        {children}
      </div>
      {position
        ? createPortal(
            // Portaled so the virtual list's transformed window cannot become
            // this fixed menu's containing block and displace it.
            <div
              className="context-menu"
              role="menu"
              ref={menuRef}
              style={{ left: position.x, top: position.y }}
            >
              {entries.map((entry) => (
                <div key={entry.id}>
                  {entry.separatorBefore ? <div className="menu__separator" /> : null}
                  <button
                    type="button"
                    role="menuitem"
                    data-testid={`context-${entry.id}`}
                    className={`menu__item${entry.disabled ? ' is-disabled' : ''}`}
                    disabled={entry.disabled}
                    onClick={() => {
                      close();
                      entry.onSelect();
                    }}
                  >
                    {entry.icon ? <span className="menu__icon">{entry.icon}</span> : null}
                    <span>{entry.label}</span>
                  </button>
                </div>
              ))}
            </div>,
            document.body
          )
        : null}
    </>
  );
}

export interface VirtualListProps<T> {
  items: T[];
  rowHeight: number;
  overscan?: number;
  renderRow: (item: T, index: number) => ReactNode;
  onViewportChange?: (range: { start: number; end: number }) => void;
  className?: string;
  testId?: string;
  empty?: ReactNode;
  focusable?: boolean;
  role?: React.AriaRole;
  'aria-activedescendant'?: string;
  onKeyDown?: React.KeyboardEventHandler<HTMLDivElement>;
}

/**
 * A fixed-height virtual list.
 *
 * The commit graph can hold thousands of rows and a diff can hold tens of
 * thousands of lines; rendering either in full would lock the webview, so only
 * the visible window plus an overscan margin is mounted.
 */
export function VirtualList<T>({
  items,
  rowHeight,
  overscan = 8,
  renderRow,
  onViewportChange,
  className,
  testId,
  empty,
  focusable = false,
  role,
  'aria-activedescendant': activeDescendant,
  onKeyDown
}: VirtualListProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(0);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) {
      return;
    }
    const observer = new ResizeObserver(() => {
      setHeight(element.clientHeight);
    });
    observer.observe(element);
    setHeight(element.clientHeight);
    return () => observer.disconnect();
  }, []);

  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const visible = Math.ceil(height / rowHeight) + overscan * 2;
  const end = Math.min(items.length, start + visible);

  useEffect(() => {
    onViewportChange?.({ start, end });
  }, [start, end, onViewportChange]);

  if (items.length === 0 && empty) {
    return (
      <div className={`virtual-list${className ? ` ${className}` : ''}`} ref={containerRef}>
        {empty}
      </div>
    );
  }

  return (
    <div
      className={`virtual-list${className ? ` ${className}` : ''}`}
      ref={containerRef}
      data-testid={testId}
      tabIndex={focusable ? 0 : undefined}
      role={role}
      aria-activedescendant={activeDescendant}
      onKeyDown={onKeyDown}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
    >
      <div className="virtual-list__sizer" style={{ height: items.length * rowHeight }}>
        <div
          className="virtual-list__window"
          style={{ transform: `translateY(${start * rowHeight}px)` }}
        >
          {items.slice(start, end).map((item, offset) => (
            <div key={start + offset} style={{ height: rowHeight }}>
              {renderRow(item, start + offset)}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export interface SplitterProps {
  orientation: 'vertical' | 'horizontal';
  onDragStart?: () => void;
  onDrag: (delta: number) => void;
  onDragEnd?: () => void;
  testId?: string;
  label?: string;
}

/**
 * A drag handle between panes.
 *
 * The handle is a 7px hit area over a 1px rule, matching the reference
 * application, and turns the drag border color while it is held.
 */
export function Splitter({
  orientation,
  onDragStart,
  onDrag,
  onDragEnd,
  testId,
  label
}: SplitterProps) {
  const [active, setActive] = useState(false);
  const start = useRef(0);

  return (
    <div
      data-testid={testId}
      className={['splitter', `splitter--${orientation}`, active ? 'is-active' : ''].join(' ')}
      role="separator"
      aria-label={label}
      aria-orientation={orientation === 'vertical' ? 'vertical' : 'horizontal'}
      onPointerDown={(event) => {
        if (event.button !== 0) {
          return;
        }
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        setActive(true);
        start.current = orientation === 'vertical' ? event.clientX : event.clientY;
        onDragStart?.();
      }}
      onPointerMove={(event) => {
        if (!active) {
          return;
        }
        const current = orientation === 'vertical' ? event.clientX : event.clientY;
        onDrag(current - start.current);
      }}
      onPointerUp={() => {
        if (active) {
          setActive(false);
          onDragEnd?.();
        }
      }}
      onPointerCancel={() => {
        if (active) {
          setActive(false);
          onDragEnd?.();
        }
      }}
    />
  );
}

export function EmptyState({
  icon,
  message,
  testId
}: {
  icon?: ReactNode;
  message: string;
  testId?: string;
}) {
  return (
    <div className="empty-state" data-testid={testId}>
      {icon ? <span className="empty-state__icon">{icon}</span> : null}
      <span className="empty-state__message">{message}</span>
    </div>
  );
}
