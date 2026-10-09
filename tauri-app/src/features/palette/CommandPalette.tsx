import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import { keysForCommand } from '../../app/keyboard';
import { useStore } from '../../app/store';
import { Icon } from '../../components/Icon';
import { t } from '../../i18n/strings';
import { buildPaletteCommands, type PaletteCommand, type PaletteGroup } from './commands';
import { fuzzyMatch } from './fuzzy';

interface SearchResult {
  command: PaletteCommand;
  score: number;
  indices: number[];
}

const GROUP_KEYS: Record<PaletteGroup, string> = {
  repository: 'palette-group-repository',
  git: 'palette-group-git',
  branches: 'palette-group-branches',
  view: 'palette-group-view',
  themes: 'palette-group-themes',
  app: 'palette-group-app'
};

export function CommandPalette() {
  const open = useStore((state) => state.paletteOpen);
  const overlay = useStore((state) => state.overlay);
  const translate = useStore((state) => state.t);
  const shortcuts = useStore((state) => state.shortcuts.resolved);
  const tabs = useStore((state) => state.tabs);
  const activeTabKey = useStore((state) => state.activeTabKey);
  const repos = useStore((state) => state.repos);
  const config = useStore((state) => state.config);
  const workspace = useStore((state) => state.workspace);
  const sidecarUi = useStore((state) => state.sidecarUi);
  const closePalette = useStore((state) => state.closePalette);
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const restoreFocus = useRef(true);

  useLayoutEffect(() => {
    if (!open) {
      if (restoreFocus.current && previousFocus.current?.isConnected) {
        previousFocus.current.focus();
      }
      previousFocus.current = null;
      restoreFocus.current = true;
      return;
    }
    previousFocus.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    restoreFocus.current = true;
    setQuery('');
    setSelectedId(null);
    inputRef.current?.focus();
  }, [open]);

  useEffect(() => {
    if (open && overlay.kind !== 'none') closePalette();
  }, [closePalette, open, overlay.kind]);

  const commands = useMemo(
    () => (open ? buildPaletteCommands() : []),
    [open, activeTabKey, tabs, repos, config, workspace, sidecarUi, shortcuts, translate]
  );
  const results = useMemo(
    () => searchCommands(commands, query, translate),
    [commands, query, translate]
  );
  const enabledResults = results.filter((result) => result.command.enabled);
  const selected =
    enabledResults.find((result) => result.command.id === selectedId) ?? enabledResults[0] ?? null;
  const groups = groupResults(results);
  const shortcut = keysForCommand(shortcuts, 'app.palette')[0];
  const activeDescendant = selected
    ? `palette-option-${encodeURIComponent(selected.command.id)}`
    : undefined;

  useEffect(() => {
    if (!selected) return;
    const option = document.getElementById(
      `palette-option-${encodeURIComponent(selected.command.id)}`
    );
    option?.scrollIntoView?.({ block: 'nearest' });
  }, [query, selected?.command.id]);

  if (!open) return null;

  const dismiss = () => {
    restoreFocus.current = true;
    closePalette();
  };

  const execute = async (command: PaletteCommand) => {
    if (!command.enabled) return;
    restoreFocus.current = false;
    closePalette();
    try {
      await command.run();
    } catch {
      useStore.getState().notify({
        level: 'error',
        message: useStore.getState().t('palette-command-failed')
      });
    }
  };

  const onInputKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (enabledResults.length === 0) return;
      const currentIndex = selected ? enabledResults.indexOf(selected) : -1;
      const direction = event.key === 'ArrowDown' ? 1 : -1;
      const nextIndex =
        currentIndex < 0
          ? direction > 0
            ? 0
            : enabledResults.length - 1
          : (currentIndex + direction + enabledResults.length) % enabledResults.length;
      setSelectedId(enabledResults[nextIndex]!.command.id);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (selected) void execute(selected.command);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      dismiss();
    }
  };

  return (
    <div
      className="palette-backdrop"
      data-testid="palette-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) dismiss();
      }}
    >
      <section
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label={t(translate, 'palette-title')}
        data-testid="palette"
        onKeyDown={(event) => {
          if (event.key === 'Tab') {
            event.preventDefault();
            inputRef.current?.focus();
          }
        }}
      >
        <div className="palette__search">
          <Icon name="search" size={16} />
          <input
            ref={inputRef}
            type="text"
            role="combobox"
            aria-label={t(translate, 'palette-input-label')}
            aria-expanded="true"
            aria-controls="palette-list"
            aria-autocomplete="list"
            aria-activedescendant={activeDescendant}
            autoComplete="off"
            spellCheck={false}
            placeholder={t(translate, 'palette-placeholder')}
            data-testid="palette-input"
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
            onKeyDown={onInputKeyDown}
          />
          <kbd className="palette__dismiss-hint">Esc</kbd>
        </div>
        <div
          className="palette__results"
          id="palette-list"
          role="listbox"
          data-testid="palette-list"
        >
          {groups.length > 0 ? (
            groups.map(({ group, items }) => (
              <div
                className="palette__group"
                key={group}
                role="group"
                aria-labelledby={`palette-group-${group}`}
              >
                <div className="palette__group-title" id={`palette-group-${group}`}>
                  {t(translate, GROUP_KEYS[group])}
                </div>
                {items.map(({ command, indices }) => {
                  const isSelected = command.id === selected?.command.id;
                  return (
                    <button
                      key={command.id}
                      id={`palette-option-${encodeURIComponent(command.id)}`}
                      type="button"
                      role="option"
                      aria-selected={isSelected}
                      aria-disabled={!command.enabled}
                      className={`palette__item${isSelected ? ' is-selected' : ''}${command.enabled ? '' : ' is-disabled'}`}
                      data-testid="palette-item"
                      data-command={command.id}
                      data-selected={isSelected ? 'true' : undefined}
                      disabled={!command.enabled}
                      onMouseDown={(event) => event.preventDefault()}
                      onMouseEnter={() => {
                        if (command.enabled) setSelectedId(command.id);
                      }}
                      onClick={() => void execute(command)}
                    >
                      <span className="palette__item-label">
                        {renderHighlighted(command.label, indices)}
                      </span>
                      {command.active ? (
                        <span
                          className="palette__active-mark"
                          aria-label={t(translate, 'palette-current')}
                        >
                          <Icon name="check" size={14} />
                        </span>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ))
          ) : (
            <div className="palette__empty" data-testid="palette-empty">
              {t(translate, 'palette-no-results')}
            </div>
          )}
        </div>
        <div className="palette__footer">
          <span>
            <kbd>↑</kbd>
            <kbd>↓</kbd> {t(translate, 'palette-navigate-hint')}
          </span>
          <span>
            <kbd>Enter</kbd> {t(translate, 'palette-run-hint')}
          </span>
          <span className="palette__shortcut">
            <kbd>{shortcut ? formatShortcut(shortcut) : primaryShortcutLabel()}</kbd>{' '}
            {t(translate, 'palette-toggle-hint')}
          </span>
        </div>
      </section>
    </div>
  );
}

function searchCommands(
  commands: PaletteCommand[],
  query: string,
  translate: (key: string) => string
): SearchResult[] {
  const tokens = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return commands
    .map((command, order) => {
      let score = 0;
      const indices = new Set<number>();
      const candidates = [
        { text: command.label, isLabel: true },
        { text: t(translate, GROUP_KEYS[command.group]), isLabel: false },
        ...command.keywords.map((text) => ({ text, isLabel: false }))
      ];
      for (const token of tokens) {
        const matches = candidates
          .map((candidate) => ({ ...candidate, match: fuzzyMatch(token, candidate.text) }))
          .filter((candidate) => candidate.match !== null)
          .sort((left, right) => right.match!.score - left.match!.score);
        const best = matches[0];
        if (!best?.match) return null;
        score += best.match.score;
        if (best.isLabel) best.match.indices.forEach((index) => indices.add(index));
      }
      return { command, score, indices: [...indices], order };
    })
    .filter((result): result is SearchResult & { order: number } => result !== null)
    .sort((left, right) => right.score - left.score || left.order - right.order);
}

function groupResults(results: SearchResult[]): { group: PaletteGroup; items: SearchResult[] }[] {
  const groups = new Map<PaletteGroup, SearchResult[]>();
  for (const result of results) {
    const group = groups.get(result.command.group) ?? [];
    group.push(result);
    groups.set(result.command.group, group);
  }
  return [...groups].map(([group, items]) => ({ group, items }));
}

function renderHighlighted(label: string, indices: number[]) {
  if (indices.length === 0) return label;
  const matches = new Set(indices);
  const parts: React.ReactNode[] = [];
  let start = 0;
  while (start < label.length) {
    const highlighted = matches.has(start);
    let end = start + 1;
    while (end < label.length && matches.has(end) === highlighted) end += 1;
    const text = label.slice(start, end);
    parts.push(highlighted ? <mark key={start}>{text}</mark> : <span key={start}>{text}</span>);
    start = end;
  }
  return parts;
}

function primaryShortcutLabel(): string {
  return /Mac|iPhone|iPad|iPod/.test(navigator.platform) ? '⌘P' : 'Ctrl+P';
}

function formatShortcut(shortcut: string): string {
  const isMac = /Mac|iPhone|iPad|iPod/.test(navigator.platform);
  const labels = shortcut
    .replace(/\+/g, '-')
    .split('-')
    .map((part) => {
      const normalized = part.toLowerCase();
      if (normalized === 'cmd' || normalized === 'command') return isMac ? '⌘' : 'Cmd';
      if (normalized === 'ctrl' || normalized === 'control' || normalized === 'cmdorctrl') {
        return isMac ? '⌘' : 'Ctrl';
      }
      if (normalized === 'alt' || normalized === 'option') return isMac ? '⌥' : 'Alt';
      if (normalized === 'shift') return isMac ? '⇧' : 'Shift';
      return part.length === 1 ? part.toUpperCase() : part;
    });
  return labels.join(isMac ? '' : '+');
}
