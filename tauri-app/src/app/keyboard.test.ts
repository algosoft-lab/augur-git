import { describe, expect, it } from 'vitest';

import { findShortcutConflict, matchesShortcut } from './keyboard';
import type { ResolvedShortcut } from '../bridge/types';

function keyboardEvent(
  key: string,
  options: Partial<Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>> = {}
): KeyboardEvent {
  return {
    key,
    code: options.code ?? '',
    ctrlKey: options.ctrlKey ?? false,
    metaKey: options.metaKey ?? false,
    altKey: options.altKey ?? false,
    shiftKey: options.shiftKey ?? false
  } as KeyboardEvent;
}

describe('keyboard shortcuts', () => {
  it('distinguishes lowercase pull from uppercase push', () => {
    expect(matchesShortcut(keyboardEvent('p'), ['p'])).toBe(true);
    expect(matchesShortcut(keyboardEvent('P', { shiftKey: true }), ['shift-p'])).toBe(true);
    expect(matchesShortcut(keyboardEvent('p'), ['shift-p'])).toBe(false);
  });

  it('matches plus keys across keyboard layouts and platform modifiers', () => {
    expect(matchesShortcut(keyboardEvent('=', { ctrlKey: true }), ['ctrl-plus'])).toBe(true);
    expect(
      matchesShortcut(keyboardEvent('+', { code: 'Equal', ctrlKey: true, shiftKey: true }), [
        'ctrl-plus'
      ])
    ).toBe(true);
    expect(
      matchesShortcut(keyboardEvent('+', { metaKey: true, shiftKey: true }), ['cmd-plus'])
    ).toBe(true);
    expect(
      matchesShortcut(keyboardEvent('Q', { ctrlKey: true, shiftKey: true }), ['CmdOrCtrl+Shift+Q'])
    ).toBe(true);
    const primaryModifier = /Mac|iPhone|iPad|iPod/.test(navigator.platform)
      ? keyboardEvent('q', { metaKey: true })
      : keyboardEvent('q', { ctrlKey: true });
    expect(matchesShortcut(primaryModifier, ['CmdOrCtrl+Q'])).toBe(true);
  });

  it('allows Space in disjoint list contexts and rejects overlapping bindings', () => {
    const shortcuts: ResolvedShortcut[] = [
      { command: 'refs.checkout', keys: ['space'] },
      { command: 'commits.checkout', keys: ['space'] },
      { command: 'changes.toggle-stage', keys: ['space'] }
    ];
    expect(findShortcutConflict(shortcuts)).toBeNull();
    expect(findShortcutConflict([...shortcuts, { command: 'repo.pull', keys: ['space'] }])).toEqual(
      { left: 'refs.checkout', right: 'repo.pull', key: 'space' }
    );
  });

  it('detects conflicts between platform aliases and their active modifier', () => {
    const primaryModifier = /Mac|iPhone|iPad|iPod/.test(navigator.platform) ? 'cmd' : 'ctrl';
    expect(
      findShortcutConflict([
        { command: 'app.quit', keys: ['CmdOrCtrl+Q'] },
        { command: 'repo.pull', keys: [`${primaryModifier}-q`] }
      ])
    ).toEqual({ left: 'app.quit', right: 'repo.pull', key: `${primaryModifier}-q` });
  });

  it('treats the command palette shortcut as global for conflict checks', () => {
    expect(
      findShortcutConflict([
        { command: 'app.palette', keys: ['ctrl-p'] },
        { command: 'repo.pull', keys: ['ctrl-p'] }
      ])
    ).toEqual({ left: 'app.palette', right: 'repo.pull', key: 'ctrl-p' });
  });
});
