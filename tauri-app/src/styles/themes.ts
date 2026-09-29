/**
 * Bundled theme definitions and runtime CSS variables.
 *
 * Every color becomes a CSS custom property, which keeps the stylesheets free
 * of theme conditionals.
 */

import type { ThemePreference } from '../bridge/types';
import { ADDITIONAL_THEME_NAMES } from './theme-catalog';
import { additionalThemeColors } from './popular-theme-seeds';

export interface ThemeDefinition {
  mode: 'light' | 'dark';
  colors: Record<string, string>;
}

const EXISTING_THEMES = {
  'github-dark': {
    mode: 'dark',
    colors: {
      background: '#0D1117',
      foreground: '#E6EDF3',
      border: '#30363D',
      'tab_bar.background': '#161B22',
      'title_bar.background': '#161B22',
      'input.border': '#21262D',
      'list.hover.background': '#21262D',
      'list.active.background': '#264F78',
      'muted.foreground': '#8B949E',
      'table.head.foreground': '#8B949E',
      'base.blue': '#2F81F7',
      'accent.background': '#2F81F7',
      'accent.foreground': '#0D1117',
      'base.green': '#3FB950',
      'base.red': '#F85149',
      'warning.background': '#D29922',
      'drag.border': '#388BFD',
      'primary.background': '#2F81F7',
      'primary.foreground': '#FFFFFF',
      'switch.background': '#3D444D',
      'switch.thumb.background': '#FFFFFF'
    }
  },
  'catppuccin-latte': {
    mode: 'light',
    colors: {
      background: '#eff1f5',
      foreground: '#4c4f69',
      border: '#ccd0da',
      'tab_bar.background': '#e6e9ef',
      'title_bar.background': '#e6e9ef',
      'input.border': '#ccd0da',
      'list.hover.background': '#ccd0da',
      'list.active.background': '#bcc0cc',
      'muted.foreground': '#9ca0b0',
      'table.head.foreground': '#6c6f85',
      'base.blue': '#1e66f5',
      'accent.background': '#1e66f5',
      'accent.foreground': '#ffffff',
      'base.green': '#40a02b',
      'base.red': '#d20f39',
      'warning.background': '#df8e1d',
      'drag.border': '#1e66f5',
      'primary.background': '#1e66f5',
      'primary.foreground': '#ffffff',
      'switch.background': '#ccd0da',
      'switch.thumb.background': '#ffffff'
    }
  },
  'catppuccin-frappe': {
    mode: 'dark',
    colors: {
      background: '#303446',
      foreground: '#c6d0f5',
      border: '#414559',
      'tab_bar.background': '#292c3c',
      'title_bar.background': '#292c3c',
      'input.border': '#414559',
      'list.hover.background': '#414559',
      'list.active.background': '#51576d',
      'muted.foreground': '#737994',
      'table.head.foreground': '#a5adce',
      'base.blue': '#8caaee',
      'accent.background': '#8caaee',
      'accent.foreground': '#303446',
      'base.green': '#a6d189',
      'base.red': '#e78284',
      'warning.background': '#e5c890',
      'drag.border': '#8caaee',
      'primary.background': '#8caaee',
      'primary.foreground': '#303446',
      'switch.background': '#737994',
      'switch.thumb.background': '#c6d0f5'
    }
  },
  'catppuccin-macchiato': {
    mode: 'dark',
    colors: {
      background: '#24273a',
      foreground: '#cad3f5',
      border: '#363a4f',
      'tab_bar.background': '#1e2030',
      'title_bar.background': '#1e2030',
      'input.border': '#363a4f',
      'list.hover.background': '#363a4f',
      'list.active.background': '#494d64',
      'muted.foreground': '#6e738d',
      'table.head.foreground': '#a5adcb',
      'base.blue': '#8aadf4',
      'accent.background': '#8aadf4',
      'accent.foreground': '#24273a',
      'base.green': '#a6da95',
      'base.red': '#ed8796',
      'warning.background': '#eed49f',
      'drag.border': '#8aadf4',
      'primary.background': '#8aadf4',
      'primary.foreground': '#24273a',
      'switch.background': '#6e738d',
      'switch.thumb.background': '#cad3f5'
    }
  },
  'catppuccin-mocha': {
    mode: 'dark',
    colors: {
      background: '#1e1e2e',
      foreground: '#cdd6f4',
      border: '#313244',
      'tab_bar.background': '#181825',
      'title_bar.background': '#181825',
      'input.border': '#313244',
      'list.hover.background': '#313244',
      'list.active.background': '#45475a',
      'muted.foreground': '#6c7086',
      'table.head.foreground': '#a6adc8',
      'base.blue': '#89b4fa',
      'accent.background': '#89b4fa',
      'accent.foreground': '#1e1e2e',
      'base.green': '#a6e3a1',
      'base.red': '#f38ba8',
      'warning.background': '#f9e2af',
      'drag.border': '#89b4fa',
      'primary.background': '#89b4fa',
      'primary.foreground': '#1e1e2e',
      'switch.background': '#6c7086',
      'switch.thumb.background': '#cdd6f4'
    }
  }
};

type ExistingThemePreference = keyof typeof EXISTING_THEMES;
type AddedThemePreference = Exclude<ThemePreference, ExistingThemePreference>;

const LEGACY_GRAPH_LANES = [
  '#a371f7',
  '#39c5cf',
  '#e06c9f',
  '#7dba00',
  '#d19a66',
  '#4993f0'
] as const;

function existingGraphLanes(colors: {
  'base.blue': string;
  'base.green': string;
  'warning.background': string;
  'base.red': string;
}): string[] {
  return [
    colors['base.blue'],
    colors['base.green'],
    colors['warning.background'],
    colors['base.red'],
    ...LEGACY_GRAPH_LANES
  ];
}

function readableForeground(color: string): string {
  return relativeLuminance(color) > 0.179 ? '#000000' : '#ffffff';
}

const existingThemesWithGraphColors = Object.fromEntries(
  Object.entries(EXISTING_THEMES).map(([key, definition]) => [
    key,
    {
      ...definition,
      colors: {
        ...definition.colors,
        'base.purple': '#a371f7',
        ...Object.fromEntries(
          existingGraphLanes(definition.colors).map((color, index) => [
            `graph.lane.${index + 1}`,
            color
          ])
        ),
        ...Object.fromEntries(
          existingGraphLanes(definition.colors).map((color, index) => [
            `graph.lane.text.${index + 1}`,
            readableForeground(color)
          ])
        )
      }
    }
  ])
) as unknown as Record<ExistingThemePreference, ThemeDefinition>;

type SourceTheme = (typeof additionalThemeColors)[keyof typeof additionalThemeColors];

function hexToRgb(value: string): [number, number, number] {
  const hex = value.slice(1);
  return [
    Number.parseInt(hex.slice(0, 2), 16),
    Number.parseInt(hex.slice(2, 4), 16),
    Number.parseInt(hex.slice(4, 6), 16)
  ];
}

function rgbToHex(channels: [number, number, number]): string {
  return `#${channels
    .map((channel) =>
      Math.round(Math.min(255, Math.max(0, channel)))
        .toString(16)
        .padStart(2, '0')
    )
    .join('')}`;
}

function mix(from: string, toward: string, weight: number): string {
  const start = hexToRgb(from);
  const end = hexToRgb(toward);
  return rgbToHex([
    start[0] + (end[0] - start[0]) * weight,
    start[1] + (end[1] - start[1]) * weight,
    start[2] + (end[2] - start[2]) * weight
  ]);
}

function isDark(color: string): boolean {
  const [red, green, blue] = hexToRgb(color);
  return (red * 299 + green * 587 + blue * 114) / 1000 < 128;
}

function relativeLuminance(color: string): number {
  const channels = hexToRgb(color).map((value) => value / 255);
  const linear = (value: number) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  return (
    0.2126 * linear(channels[0]!) + 0.7152 * linear(channels[1]!) + 0.0722 * linear(channels[2]!)
  );
}

function graphLaneColors(colors: SourceTheme, dark: boolean): string[] {
  const ansi = colors.ansi!;
  const indices = [4, 5, 2, 6, 1, 3, 12, 13, 10, 14];
  const toward = dark ? '#ffffff' : '#000000';
  const lanes: string[] = [];
  for (const index of indices) {
    const source = ansi[index]!;
    let lane = source;
    for (let step = 1; lanes.includes(lane) && step <= 9; step += 1) {
      lane = mix(source, toward, step / 10);
    }
    lanes.push(lane);
  }
  return lanes;
}

function makeTheme(colors: SourceTheme): ThemeDefinition {
  const dark = isDark(colors.background);
  const accent = colors.accent ?? colors.bright ?? (dark ? '#89b4fa' : '#1e66f5');
  const danger = colors.danger ?? (dark ? '#f38ba8' : '#d20f39');
  const ansi = colors.ansi!;
  const panel = colors.panel ?? mix(colors.background, colors.foreground, 0.03);
  const panelAlt = colors.panelAlt ?? mix(panel, colors.foreground, 0.05);
  const border = colors.border ?? mix(colors.background, colors.foreground, 0.16);
  const borderStrong = colors.borderStrong ?? mix(colors.background, colors.foreground, 0.26);
  const foregroundOnAccent = relativeLuminance(accent) > 0.179 ? '#000000' : '#ffffff';
  const lanes = graphLaneColors(colors, dark);

  return {
    mode: dark ? 'dark' : 'light',
    colors: {
      background: colors.background,
      foreground: colors.foreground,
      border,
      'tab_bar.background': panel,
      'title_bar.background': panel,
      'input.border': panelAlt,
      'list.hover.background': colors.hover ?? panelAlt,
      'list.active.background':
        colors.selection ?? mix(colors.background, accent, dark ? 0.28 : 0.35),
      'muted.foreground': colors.textMuted ?? mix(colors.foreground, colors.background, 0.4),
      'table.head.foreground':
        colors.textDim ?? colors.textMuted ?? mix(colors.foreground, colors.background, 0.12),
      'base.blue': ansi[4]!,
      'base.purple': ansi[5]!,
      'accent.background': accent,
      'accent.foreground': foregroundOnAccent,
      'base.green': ansi[2]!,
      'base.red': danger,
      'warning.background': ansi[3]!,
      'drag.border': accent,
      'primary.background': accent,
      'primary.foreground': foregroundOnAccent,
      'switch.background': borderStrong,
      'switch.thumb.background': colors.foreground,
      ...Object.fromEntries(lanes.map((color, index) => [`graph.lane.${index + 1}`, color])),
      ...Object.fromEntries(
        lanes.map((color, index) => [`graph.lane.text.${index + 1}`, readableForeground(color)])
      )
    }
  };
}

const addedThemes = Object.fromEntries(
  ADDITIONAL_THEME_NAMES.map(([preference, name]) => [
    preference,
    makeTheme(additionalThemeColors[name]!)
  ])
) as Record<AddedThemePreference, ThemeDefinition>;

export const THEMES: Record<ThemePreference, ThemeDefinition> = {
  ...existingThemesWithGraphColors,
  ...addedThemes
};

/**
 * CSS variable name for one catalog key.
 *
 * The catalog spells nesting with a dot and words with an underscore, while the
 * stylesheets address everything with dashes, so both separators are folded.
 */
export function cssVariable(key: string): string {
  return `--${key.replace(/[._\s]/g, '-')}`;
}

/**
 * Write one theme onto an element. The typography settings are applied here too
 * so a single style pass establishes the whole visual contract.
 */
export function applyTheme(
  root: HTMLElement,
  theme: ThemePreference,
  typography: {
    uiFontFamily: string | null;
    monoFontFamily: string | null;
    uiFontSize: number;
    diffFontSize: number;
  }
): void {
  const definition = THEMES[theme];
  for (const [key, value] of Object.entries(definition.colors)) {
    root.style.setProperty(cssVariable(key), value);
  }
  root.style.setProperty(
    '--ui-font-family',
    typography.uiFontFamily ? cssFontFamily(typography.uiFontFamily) : 'system-ui'
  );
  root.style.setProperty(
    '--mono-font-family',
    typography.monoFontFamily ? cssFontFamily(typography.monoFontFamily) : 'ui-monospace'
  );
  root.style.setProperty('--ui-font-size', `${typography.uiFontSize}px`);
  root.style.setProperty('--diff-font-size', `${typography.diffFontSize}px`);
  root.dataset.mode = definition.mode;
  root.style.colorScheme = definition.mode;
}

/** Quote one family name as a CSS string without changing its exact name. */
export function cssFontFamily(value: string): string {
  const escaped = Array.from(value, (character) => {
    const codepoint = character.codePointAt(0)!;
    if (character === '\\') {
      return '\\\\';
    }
    if (character === '"') {
      return '\\"';
    }
    if (codepoint === 0) {
      return '\\fffd ';
    }
    if (codepoint < 0x20 || codepoint === 0x7f) {
      return `\\${codepoint.toString(16)} `;
    }
    return character;
  }).join('');
  return `"${escaped}"`;
}

/** The theme used before the preferences have been read. */
export const DEFAULT_THEME: ThemePreference = 'claude-dark';

/** The typography used before the preferences have been read. */
export const DEFAULT_TYPOGRAPHY = {
  uiFontFamily: null,
  monoFontFamily: null,
  uiFontSize: 16,
  diffFontSize: 16
};

/** The ten lane colors the commit graph cycles through. */
export const LANE_COLORS = Array.from(
  { length: 10 },
  (_, index) => `var(--graph-lane-${index + 1})`
);
