import type { SettingsSection } from '../../bridge/types';
import type { Translator } from '../../i18n';
import { fuzzyScore } from '../../utils/fuzzy';

export interface SettingsSearchEntry {
  section: SettingsSection;
  labelKey: string;
  targetTestId: string;
  hintKey?: string;
}

export interface SettingsSearchResult extends SettingsSearchEntry {
  label: string;
  sectionLabel: string;
  score: number;
}

export const SETTINGS_SEARCH_ENTRIES: SettingsSearchEntry[] = [
  { section: 'general', labelKey: 'language-title', targetTestId: 'settings-field-language' },
  {
    section: 'general',
    labelKey: 'auto-refresh-title',
    targetTestId: 'settings-field-auto-refresh'
  },
  {
    section: 'general',
    labelKey: 'cli-title',
    hintKey: 'cli-description',
    targetTestId: 'settings-cli'
  },
  {
    section: 'general',
    labelKey: 'settings-store-location',
    targetTestId: 'settings-field-store-location'
  },
  { section: 'appearance', labelKey: 'theme-title', targetTestId: 'settings-field-theme' },
  { section: 'appearance', labelKey: 'ui-font-title', targetTestId: 'settings-field-ui-font' },
  {
    section: 'appearance',
    labelKey: 'mono-font-title',
    targetTestId: 'settings-field-mono-font'
  },
  {
    section: 'appearance',
    labelKey: 'ui-font-size-title',
    hintKey: 'ui-font-size-description',
    targetTestId: 'settings-field-ui-font-size'
  },
  {
    section: 'appearance',
    labelKey: 'diff-font-size-title',
    hintKey: 'diff-font-size-description',
    targetTestId: 'settings-field-diff-font-size'
  },
  { section: 'layout', labelKey: 'diff-layout-title', targetTestId: 'settings-field-diff-layout' },
  {
    section: 'layout',
    labelKey: 'graph-history-title',
    targetTestId: 'settings-field-graph-history'
  },
  { section: 'layout', labelKey: 'commit-title', targetTestId: 'settings-field-commit-action' },
  { section: 'layout', labelKey: 'pull-action-title', targetTestId: 'settings-field-pull-action' },
  {
    section: 'shortcuts',
    labelKey: 'settings-shortcuts',
    targetTestId: 'settings-shortcuts-section'
  }
];

const SECTION_LABEL_KEYS: Record<SettingsSection, string> = {
  general: 'settings-general',
  appearance: 'settings-appearance',
  layout: 'settings-layout',
  shortcuts: 'settings-shortcuts'
};

export function searchSettings(
  entries: SettingsSearchEntry[],
  translate: Translator,
  query: string
): SettingsSearchResult[] {
  if (!query.trim()) {
    return [];
  }

  return entries
    .flatMap((entry, index) => {
      const label = translate(entry.labelKey);
      const sectionLabel = translate(SECTION_LABEL_KEYS[entry.section]);
      const score = Math.max(
        fuzzyScore(query, label) ?? -Infinity,
        fuzzyScore(query, entry.hintKey ? translate(entry.hintKey) : '') ?? -Infinity,
        fuzzyScore(query, sectionLabel) ?? -Infinity
      );
      return Number.isFinite(score) ? [{ ...entry, label, sectionLabel, score, index }] : [];
    })
    .sort((left, right) => right.score - left.score || left.index - right.index)
    .map(({ index: _index, ...result }) => result);
}
