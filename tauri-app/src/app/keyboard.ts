import type { ResolvedShortcut } from '../bridge/types';

export interface ShortcutCommandDefinition {
  command: string;
  label: string;
}

export const SHORTCUT_COMMANDS: ShortcutCommandDefinition[] = [
  { command: 'app.quit', label: 'shortcut-app-quit' },
  { command: 'app.palette', label: 'shortcut-app-palette' },
  { command: 'repo.pull', label: 'shortcut-repo-pull' },
  { command: 'repo.push', label: 'shortcut-repo-push' },
  { command: 'repo.fetch', label: 'shortcut-repo-fetch' },
  { command: 'repo.refresh', label: 'shortcut-repo-refresh' },
  { command: 'commit.focus', label: 'shortcut-commit-focus' },
  { command: 'refs.checkout', label: 'shortcut-refs-checkout' },
  { command: 'commits.checkout', label: 'shortcut-commits-checkout' },
  { command: 'changes.toggle-stage', label: 'shortcut-changes-toggle-stage' },
  { command: 'list.next', label: 'shortcut-list-next' },
  { command: 'list.previous', label: 'shortcut-list-previous' },
  { command: 'graph.search', label: 'shortcut-graph-search' },
  { command: 'diff.font-increase', label: 'shortcut-diff-font-increase' },
  { command: 'diff.font-decrease', label: 'shortcut-diff-font-decrease' },
  { command: 'diff.font-reset', label: 'shortcut-diff-font-reset' }
];

/** Match a physical key event against either the current or legacy shortcut spelling. */
export function matchesShortcut(
  event: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>,
  keys: string[]
): boolean {
  const actual = eventCombo(event);
  return keys.some((key) => {
    const normalized = normalizeCombo(key);
    if (normalized === actual) return true;
    if (!normalized?.startsWith('cmdorctrl-')) return false;
    const actualPrimary = actual.startsWith('ctrl-')
      ? 'ctrl-'
      : actual.startsWith('cmd-')
        ? 'cmd-'
        : '';
    return actualPrimary !== '' && normalized.replace(/^cmdorctrl-/, actualPrimary) === actual;
  });
}

export function keysForCommand(shortcuts: ResolvedShortcut[], command: string): string[] {
  return shortcuts.find((shortcut) => shortcut.command === command)?.keys ?? [];
}

export function hasOpenPopup(): boolean {
  return (
    document.querySelector('[role="menu"], [role="listbox"]:not([data-testid="graph-list"])') !==
    null
  );
}

export function findShortcutConflict(
  shortcuts: ResolvedShortcut[]
): { left: string; right: string; key: string } | null {
  for (let index = 0; index < shortcuts.length; index += 1) {
    const left = shortcuts[index]!;
    for (const right of shortcuts.slice(index + 1)) {
      if (!contextsOverlap(left.command, right.command)) continue;
      for (const key of left.keys) {
        const normalized = normalizeCombo(key);
        if (
          normalized &&
          right.keys.some((candidate) => normalizeCombo(candidate) === normalized)
        ) {
          return { left: left.command, right: right.command, key: normalized };
        }
      }
    }
  }
  return null;
}

/** Focus the next item in a non-virtual list while preserving native input behavior. */
export function moveListFocus(
  event: React.KeyboardEvent<HTMLElement>,
  shortcuts: ResolvedShortcut[]
): boolean {
  if (hasOpenPopup()) {
    return false;
  }
  const target = event.target;
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  const command = matchesShortcut(event.nativeEvent, keysForCommand(shortcuts, 'list.next'))
    ? 'next'
    : matchesShortcut(event.nativeEvent, keysForCommand(shortcuts, 'list.previous'))
      ? 'previous'
      : null;
  if (!command) {
    return false;
  }
  const list = target.closest<HTMLElement>('[data-keyboard-list]');
  if (!list) {
    return false;
  }
  const items = Array.from(list.querySelectorAll<HTMLElement>('[data-keyboard-list-item]')).filter(
    (item) => item.getClientRects().length > 0 && !item.hasAttribute('disabled')
  );
  if (items.length === 0) {
    return false;
  }
  const current = target.closest<HTMLElement>('[data-keyboard-list-item]');
  const currentIndex = current ? items.indexOf(current) : -1;
  const nextIndex =
    currentIndex < 0
      ? command === 'next'
        ? 0
        : items.length - 1
      : Math.max(0, Math.min(items.length - 1, currentIndex + (command === 'next' ? 1 : -1)));
  event.preventDefault();
  items[nextIndex]?.focus();
  return true;
}

function eventCombo(
  event: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>
): string {
  let key = event.key.toLowerCase();
  if (event.code === 'NumpadAdd' || key === '+' || key === '=') {
    key = 'plus';
  } else if (event.code === 'NumpadSubtract' || key === '-') {
    key = 'minus';
  } else {
    key =
      (
        {
          ' ': 'space',
          arrowup: 'up',
          arrowdown: 'down',
          arrowleft: 'left',
          arrowright: 'right',
          esc: 'escape',
          return: 'enter',
          '/': 'slash'
        } as Record<string, string>
      )[key] ?? key;
  }
  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push('ctrl');
  if (event.altKey) modifiers.push('alt');
  if (event.shiftKey && key !== 'plus') modifiers.push('shift');
  if (event.metaKey) modifiers.push('cmd');
  modifiers.sort((left, right) => modifierOrder(left) - modifierOrder(right));
  modifiers.push(key);
  return modifiers.join('-');
}

function normalizeCombo(combo: string): string | null {
  const value = combo.trim();
  if (!value) return null;
  const legacyPlusForm = value.includes('+');
  const sourceParts = value.includes('+') ? value.split('+') : value.split('-');
  const rawKey = sourceParts.pop();
  if (!rawKey) return null;
  const parts = sourceParts.map((part) => part.toLowerCase());
  const key = rawKey.toLowerCase();
  const modifiers: string[] = [];
  for (const part of parts) {
    const modifier =
      (
        {
          control: 'ctrl',
          option: 'alt',
          command: 'cmd',
          super: 'cmd',
          cmdorctrl: 'cmdorctrl',
          commandorcontrol: 'cmdorctrl'
        } as Record<string, string>
      )[part] ?? part;
    if (!['ctrl', 'alt', 'shift', 'cmd', 'meta', 'cmdorctrl'].includes(modifier)) {
      return null;
    }
    modifiers.push(modifier);
  }
  const normalizedKey =
    (
      {
        spacebar: 'space',
        arrowup: 'up',
        arrowdown: 'down',
        arrowleft: 'left',
        arrowright: 'right',
        esc: 'escape',
        return: 'enter',
        '=': 'plus',
        '+': 'plus',
        '-': 'minus',
        '/': 'slash'
      } as Record<string, string>
    )[key] ?? key;
  if (
    !legacyPlusForm &&
    rawKey.length === 1 &&
    rawKey !== rawKey.toLowerCase() &&
    !modifiers.includes('shift')
  ) {
    modifiers.push('shift');
  }
  modifiers.sort((left, right) => modifierOrder(left) - modifierOrder(right));
  const primary = /Mac|iPhone|iPad|iPod/.test(navigator.platform) ? 'cmd' : 'ctrl';
  const effectiveModifiers = modifiers.map((modifier) =>
    modifier === 'cmdorctrl' ? primary : modifier
  );
  return [...effectiveModifiers, normalizedKey].join('-');
}

function contextsOverlap(left: string, right: string): boolean {
  const global = (command: string) =>
    [
      'app.quit',
      'app.palette',
      'repo.pull',
      'repo.push',
      'repo.fetch',
      'repo.refresh',
      'commit.focus',
      'diff.font-increase',
      'diff.font-decrease',
      'diff.font-reset'
    ].includes(command);
  if (global(left) || global(right)) return true;
  const context = (command: string) =>
    ({
      'refs.checkout': 'refs',
      'commits.checkout': 'graph',
      'graph.search': 'graph',
      'changes.toggle-stage': 'changes',
      'list.next': 'lists',
      'list.previous': 'lists'
    })[command];
  const leftContext = context(left);
  const rightContext = context(right);
  return (
    leftContext === rightContext ||
    (leftContext === 'lists' && ['refs', 'graph', 'changes'].includes(rightContext ?? '')) ||
    (rightContext === 'lists' && ['refs', 'graph', 'changes'].includes(leftContext ?? ''))
  );
}

function modifierOrder(modifier: string): number {
  return (
    ({ ctrl: 0, cmdorctrl: 0, alt: 1, shift: 2, cmd: 3, meta: 4 } as Record<string, number>)[
      modifier
    ] ?? 5
  );
}
