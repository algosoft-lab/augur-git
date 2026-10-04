/**
 * The settings window.
 *
 * A native standalone window, like About: the backend opens it once and focuses
 * it on repeat requests. Four sections cover everything the non-AI build
 * exposes: language, themes and fonts, the default diff and history behavior,
 * and shortcut overrides. Every change is written through the backend
 * immediately so it survives a crash, and the native menu is rebuilt by the
 * backend when the language changes.
 */

import { useEffect, useRef, useState } from 'react';

import { CliSettings } from './CliSettings';

import { Icon } from '../../components/Icon';
import { Select, Slider, TextInput } from '../../components/controls';
import * as ipc from '../../bridge/ipc';
import type {
  DiffLayoutPreference,
  GraphHistoryPreference,
  LanguagePreference,
  SettingsSection
} from '../../bridge/types';
import { useStore } from '../../app/store';
import { findShortcutConflict, SHORTCUT_COMMANDS } from '../../app/keyboard';
import { t, ta } from '../../i18n/strings';
import { THEME_GROUPS } from '../../styles/theme-catalog';
import { IS_MACOS, WindowControls } from '../shell/WindowControls';
import { handleTitleBarMouseDown } from '../shell/titleBarDrag';

type Section = SettingsSection;

const SECTIONS: { id: Section; key: string }[] = [
  { id: 'general', key: 'settings-general' },
  { id: 'appearance', key: 'settings-appearance' },
  { id: 'layout', key: 'settings-layout' },
  { id: 'shortcuts', key: 'settings-shortcuts' }
];

const LANGUAGES: { value: LanguagePreference; key: string }[] = [
  { value: 'system', key: 'language-system' },
  { value: 'zh-CN', key: 'language-chinese' },
  { value: 'en-US', key: 'language-english' }
];

const DIFF_LAYOUTS: { value: DiffLayoutPreference; key: string }[] = [
  { value: 'side-by-side', key: 'diff-layout-side-by-side' },
  { value: 'inline', key: 'diff-layout-inline' }
];

const HISTORIES: { value: GraphHistoryPreference; key: string }[] = [
  { value: 'all-branches', key: 'graph-history-all' },
  { value: 'current-branch', key: 'graph-history-current' }
];

function focusThemeSelector(): void {
  document.querySelector<HTMLButtonElement>('[data-testid="settings-theme"]')?.focus();
}

export function SettingsWindow({ initialSection = 'general' }: { initialSection?: Section }) {
  const translate = useStore((state) => state.t);
  const [section, setSection] = useState<Section>(initialSection);
  const [fonts, setFonts] = useState<string[]>([]);
  const focusThemeAfterNavigation = useRef(initialSection === 'appearance');

  useEffect(() => {
    if (section === 'appearance' && focusThemeAfterNavigation.current) {
      focusThemeAfterNavigation.current = false;
      focusThemeSelector();
    }
  }, [section]);

  useEffect(() => {
    let cancelled = false;
    let cleanup: () => void = () => {};
    void ipc
      .onSettingsNavigate((nextSection) => {
        setSection(nextSection);
        if (nextSection === 'appearance') {
          focusThemeAfterNavigation.current = true;
          window.setTimeout(() => {
            if (!cancelled) focusThemeSelector();
          }, 0);
        }
      })
      .then((unlisten) => {
        if (cancelled) unlisten();
        else cleanup = unlisten;
      });
    return () => {
      cancelled = true;
      cleanup();
    };
  }, []);

  useEffect(() => {
    void ipc
      .listFontFamilies()
      .then(setFonts)
      .catch(() => setFonts([]));
  }, []);

  return (
    <div className="window-page" data-testid="settings-window">
      <div
        className={`window-titlebar${IS_MACOS ? ' window-titlebar--macos' : ''}`}
        onMouseDown={handleTitleBarMouseDown}
      >
        <span className="settings__title" data-testid="settings-title">
          {t(translate, 'settings-title')}
        </span>
        <div
          className="window-titlebar__drag"
          {...(IS_MACOS ? { 'data-tauri-drag-region': true } : {})}
        />
        <WindowControls maximize={false} />
      </div>
      <div className="settings">
        <nav className="settings__nav">
          {SECTIONS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              className={`settings__nav-item${section === entry.id ? ' is-active' : ''}`}
              data-testid={`settings-nav-${entry.id}`}
              onClick={() => setSection(entry.id)}
            >
              {t(translate, entry.key)}
            </button>
          ))}
        </nav>
        <div className="settings__content" data-testid={`settings-${section}`}>
          {section === 'general' ? <GeneralSection /> : null}
          {section === 'appearance' ? <AppearanceSection fonts={fonts} /> : null}
          {section === 'layout' ? <LayoutSection /> : null}
          {section === 'shortcuts' ? <ShortcutsSection /> : null}
        </div>
      </div>
    </div>
  );
}

function GeneralSection() {
  const translate = useStore((state) => state.t);
  const language = useStore((state) => state.config.language);
  const autoRefresh = useStore((state) => state.config.view.auto_refresh);
  const setLanguage = useStore((state) => state.setLanguage);
  const setView = useStore((state) => state.setView);

  return (
    <>
      <div className="settings__heading">{t(translate, 'settings-general')}</div>
      <p className="settings__description">{t(translate, 'settings-description')}</p>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'language-title')}</span>
        <Select
          value={language}
          testId="settings-language"
          options={LANGUAGES.map((entry) => ({
            value: entry.value,
            label: t(translate, entry.key)
          }))}
          onChange={(value) => void setLanguage(value)}
        />
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'auto-refresh-title')}</span>
        <Select
          value={autoRefresh}
          testId="settings-auto-refresh"
          options={[
            { value: true, label: t(translate, 'setting-enabled') },
            { value: false, label: t(translate, 'setting-disabled') }
          ]}
          onChange={(value) => void setView({ auto_refresh: value })}
        />
      </div>
      <CliSettings />
      <StoreLocation />
    </>
  );
}

function StoreLocation() {
  const translate = useStore((state) => state.t);
  const paths = useStore((state) => state.storePaths);
  if (paths.length === 0) {
    return null;
  }
  return (
    <div className="settings__field">
      <span className="settings__label">{t(translate, 'settings-store-location')}</span>
      {paths.map((path) => (
        <div key={path} className="settings__hint mono">
          {path}
        </div>
      ))}
    </div>
  );
}

function AppearanceSection({ fonts }: { fonts: string[] }) {
  const translate = useStore((state) => state.t);
  const theme = useStore((state) => state.config.theme);
  const typography = useStore((state) => state.config.typography);
  const setTheme = useStore((state) => state.setTheme);
  const setTypography = useStore((state) => state.setTypography);

  const fontOptions = [
    { value: '', label: t(translate, 'font-system-default') },
    ...[typography.ui_font_family, typography.mono_font_family]
      .filter(
        (family): family is string =>
          family !== null && family.length > 0 && !fonts.includes(family)
      )
      .map((family) => ({ value: family, label: family })),
    ...fonts.map((family) => ({ value: family, label: family }))
  ];

  return (
    <>
      <div className="settings__heading">{t(translate, 'settings-appearance')}</div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'theme-title')}</span>
        <Select
          value={theme}
          testId="settings-theme"
          searchable
          searchPlaceholder={t(translate, 'theme-search-placeholder')}
          options={THEME_GROUPS.flatMap((group) =>
            group.themes.map((entry) => ({
              value: entry.value,
              label: entry.labelKey ? t(translate, entry.labelKey) : entry.name,
              group: group.name
            }))
          )}
          onChange={(value) => void setTheme(value)}
        />
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'ui-font-title')}</span>
        <Select
          searchable
          allowCustomValue
          searchPlaceholder={t(translate, 'font-search-placeholder')}
          value={typography.ui_font_family ?? ''}
          testId="settings-ui-font"
          options={fontOptions}
          onChange={(value) => void setTypography({ ui_font_family: value || null })}
        />
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'mono-font-title')}</span>
        <Select
          searchable
          allowCustomValue
          searchPlaceholder={t(translate, 'font-search-placeholder')}
          value={typography.mono_font_family ?? ''}
          testId="settings-mono-font"
          options={fontOptions}
          onChange={(value) => void setTypography({ mono_font_family: value || null })}
        />
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'ui-font-size-title')}</span>
        <div className="settings__row">
          <Slider
            value={typography.ui_font_size}
            min={12}
            max={20}
            testId="settings-ui-font-size"
            onChange={(value) => void setTypography({ ui_font_size: value })}
          />
          <span className="settings__value">{typography.ui_font_size} px</span>
        </div>
        <div className="settings__hint">{t(translate, 'ui-font-size-description')}</div>
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'diff-font-size-title')}</span>
        <div className="settings__row">
          <Slider
            value={typography.diff_font_size}
            min={12}
            max={20}
            testId="settings-diff-font-size"
            onChange={(value) => void setTypography({ diff_font_size: value })}
          />
          <span className="settings__value">{typography.diff_font_size} px</span>
        </div>
        <div className="settings__hint">{t(translate, 'diff-font-size-description')}</div>
      </div>
    </>
  );
}

function LayoutSection() {
  const translate = useStore((state) => state.t);
  const diffLayout = useStore((state) => state.config.view.diff_layout);
  const history = useStore((state) => state.config.view.graph_history);
  const commitAction = useStore((state) => state.config.view.commit_action);
  const pullAction = useStore((state) => state.config.view.pull_action);
  const setDiffLayout = useStore((state) => state.setDiffLayout);
  const setView = useStore((state) => state.setView);

  return (
    <>
      <div className="settings__heading">{t(translate, 'settings-layout')}</div>
      <p className="settings__description">{t(translate, 'layout-persistence-description')}</p>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'diff-layout-title')}</span>
        <Select
          value={diffLayout}
          testId="settings-diff-layout"
          options={DIFF_LAYOUTS.map((entry) => ({
            value: entry.value,
            label: t(translate, entry.key)
          }))}
          onChange={(value) => void setDiffLayout(value)}
        />
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'graph-history-title')}</span>
        <Select
          value={history}
          testId="settings-graph-history"
          options={HISTORIES.map((entry) => ({
            value: entry.value,
            label: t(translate, entry.key)
          }))}
          onChange={(value) => void setView({ graph_history: value })}
        />
        <div className="settings__hint">{t(translate, 'graph-history-description')}</div>
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'commit-title')}</span>
        <Select
          value={commitAction}
          testId="settings-commit-action"
          options={[
            { value: 'commit' as const, label: t(translate, 'commit-action-commit') },
            { value: 'amend' as const, label: t(translate, 'commit-action-amend') }
          ]}
          onChange={(value) => void setView({ commit_action: value })}
        />
      </div>
      <div className="settings__field">
        <span className="settings__label">{t(translate, 'pull-action-title')}</span>
        <Select
          value={pullAction}
          testId="settings-pull-action"
          options={[
            { value: 'merge' as const, label: t(translate, 'pull-action-merge') },
            { value: 'rebase' as const, label: t(translate, 'pull-action-rebase') }
          ]}
          onChange={(value) => void setView({ pull_action: value })}
        />
        <div className="settings__hint">{t(translate, 'pull-action-description')}</div>
      </div>
    </>
  );
}

function ShortcutsSection() {
  const translate = useStore((state) => state.t);
  const shortcuts = useStore((state) => state.shortcuts);
  const setShortcut = useStore((state) => state.setShortcut);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  const defaultKeys = (command: string) => {
    const resolved = shortcuts.defaults.find((entry) => entry.command === command);
    return resolved?.keys.join(', ') ?? '';
  };

  const currentKeys = (command: string) =>
    shortcuts.resolved.find((entry) => entry.command === command)?.keys.join(', ') ?? '';

  const commit = async (command: string) => {
    const value = draft[command];
    if (value === undefined) {
      return;
    }
    try {
      const keys = await ipc.validateShortcut(value);
      const proposed = shortcuts.resolved.some((entry) => entry.command === command)
        ? shortcuts.resolved.map((entry) =>
            entry.command === command ? { ...entry, keys } : entry
          )
        : [...shortcuts.resolved, { command, keys }];
      if (findShortcutConflict(proposed)) {
        setErrors((current) => ({ ...current, [command]: t(translate, 'shortcut-conflict') }));
        return;
      }
      setErrors((current) => ({ ...current, [command]: '' }));
      await setShortcut(command, keys);
      setDraft((current) => {
        const next = { ...current };
        delete next[command];
        return next;
      });
    } catch (failure) {
      const failureInfo = ipc.describeError(failure);
      const messageKey =
        failureInfo.key === 'err-shortcut-conflict'
          ? 'shortcut-conflict'
          : 'shortcut-invalid-combo';
      setErrors((current) => ({ ...current, [command]: t(translate, messageKey) }));
    }
  };

  return (
    <>
      <div className="settings__heading">{t(translate, 'settings-shortcuts')}</div>
      <p className="settings__description">{t(translate, 'shortcut-edit-description')}</p>
      {SHORTCUT_COMMANDS.map(({ command, label }) => {
        const value = draft[command] ?? currentKeys(command);
        return (
          <div key={command} className="settings__shortcut-row">
            <span className="settings__label" style={{ width: 120 }}>
              {t(translate, label)}
            </span>
            <TextInput
              value={value}
              monospace
              testId={`shortcut-${command}`}
              onChange={(next) => setDraft((current) => ({ ...current, [command]: next }))}
              onSubmit={() => void commit(command)}
            />
            <button
              type="button"
              className="tool-button tool-button--compact"
              data-testid={`shortcut-reset-${command}`}
              onClick={() => {
                setDraft((current) => {
                  const next = { ...current };
                  delete next[command];
                  return next;
                });
                setErrors((current) => ({ ...current, [command]: '' }));
                void setShortcut(command, null);
              }}
            >
              {t(translate, 'shortcut-reset')}
            </button>
            {/* The shipped binding, so an override is a choice rather than a
                guess at what it replaced. */}
            <span className="settings__hint" data-testid={`shortcut-default-${command}`}>
              {ta(translate, 'shortcut-default-hint', { keys: defaultKeys(command) })}
            </span>
          </div>
        );
      })}
      {Object.entries(errors)
        .filter(([, message]) => message)
        .map(([command, message]) => (
          <div key={command} className="status-conflict" data-testid="shortcut-error">
            {message}
          </div>
        ))}
    </>
  );
}

export { Icon };
