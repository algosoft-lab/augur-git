import { describe, expect, it } from 'vitest';

import { createTranslator } from '../../i18n';
import { searchSettings, SETTINGS_SEARCH_ENTRIES } from './settingsIndex';

describe('settings search index', () => {
  it('contains a unique jump target for every indexed setting', () => {
    const targets = SETTINGS_SEARCH_ENTRIES.map((entry) => entry.targetTestId);
    expect(new Set(targets).size).toBe(targets.length);
    expect(targets).toEqual([
      'settings-field-language',
      'settings-field-auto-refresh',
      'settings-cli',
      'settings-field-store-location',
      'settings-field-theme',
      'settings-field-ui-font',
      'settings-field-mono-font',
      'settings-field-ui-font-size',
      'settings-field-diff-font-size',
      'settings-field-diff-layout',
      'settings-field-graph-history',
      'settings-field-commit-action',
      'settings-field-pull-action',
      'settings-shortcuts-section'
    ]);
  });

  it('ranks fuzzy translated label matches and searches the active locale', () => {
    const translate = createTranslator({
      'settings-general': 'General',
      'settings-appearance': 'Appearance',
      'settings-layout': 'Layout',
      'settings-shortcuts': 'Shortcuts',
      'ui-font-title': 'UI font',
      'mono-font-title': 'Monospace font',
      'ui-font-size-title': 'UI font size',
      'diff-font-size-title': 'Diff font size',
      'theme-title': 'Theme',
      'language-title': 'Language',
      'auto-refresh-title': 'Auto refresh',
      'cli-title': 'Terminal command',
      'settings-store-location': 'Stored in',
      'diff-layout-title': 'Diff layout',
      'graph-history-title': 'Graph history',
      'commit-title': 'Commit',
      'pull-action-title': 'Pull action'
    });
    const results = searchSettings(SETTINGS_SEARCH_ENTRIES, translate, 'uif');

    expect(results[0]?.targetTestId).toBe('settings-field-ui-font');
    expect(results.some((result) => result.section === 'appearance')).toBe(true);
  });

  it('matches labels translated into the active language', () => {
    const translate = createTranslator({
      'settings-general': '常规',
      'settings-appearance': '外观',
      'settings-layout': '布局',
      'settings-shortcuts': '快捷键',
      'ui-font-title': '界面字体'
    });

    expect(searchSettings(SETTINGS_SEARCH_ENTRIES, translate, '界面字体')[0]?.targetTestId).toBe(
      'settings-field-ui-font'
    );
  });
});
