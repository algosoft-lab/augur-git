import type { ThemePreference } from '../bridge/types';

export interface ThemeChoice {
  value: ThemePreference;
  name: string;
  labelKey?: string;
  sourceName?: string;
}

export interface ThemeGroup {
  id: string;
  name: string;
  themes: ThemeChoice[];
}

export const THEME_GROUPS: ThemeGroup[] = [
  {
    id: 'github',
    name: 'GitHub',
    themes: [
      { value: 'github-dark', name: 'GitHub Dark', labelKey: 'theme-github-dark' },
      {
        value: 'github-dark-default',
        name: 'GitHub Dark Default',
        sourceName: 'GitHub Dark Default'
      },
      {
        value: 'github-light-default',
        name: 'GitHub Light Default',
        sourceName: 'GitHub Light Default'
      }
    ]
  },
  {
    id: 'catppuccin',
    name: 'Catppuccin',
    themes: [
      { value: 'catppuccin-latte', name: 'Catppuccin Latte', labelKey: 'theme-catppuccin-latte' },
      {
        value: 'catppuccin-frappe',
        name: 'Catppuccin Frappé',
        labelKey: 'theme-catppuccin-frappe'
      },
      {
        value: 'catppuccin-macchiato',
        name: 'Catppuccin Macchiato',
        labelKey: 'theme-catppuccin-macchiato'
      },
      { value: 'catppuccin-mocha', name: 'Catppuccin Mocha', labelKey: 'theme-catppuccin-mocha' }
    ]
  },
  {
    id: 'dracula',
    name: 'Dracula',
    themes: [{ value: 'dracula', name: 'Dracula', sourceName: 'Dracula' }]
  },
  {
    id: 'tokyo-night',
    name: 'Tokyo Night',
    themes: [
      { value: 'tokyo-night', name: 'Tokyo Night', sourceName: 'Tokyo Night' },
      { value: 'tokyo-night-storm', name: 'Tokyo Night Storm', sourceName: 'Tokyo Night Storm' },
      { value: 'tokyo-night-light', name: 'Tokyo Night Light', sourceName: 'Tokyo Night Light' }
    ]
  },
  {
    id: 'gruvbox',
    name: 'Gruvbox',
    themes: [
      { value: 'gruvbox-dark', name: 'Gruvbox Dark', sourceName: 'Gruvbox Dark' },
      { value: 'gruvbox-light', name: 'Gruvbox Light', sourceName: 'Gruvbox Light' }
    ]
  },
  { id: 'nord', name: 'Nord', themes: [{ value: 'nord', name: 'Nord', sourceName: 'Nord' }] },
  {
    id: 'solarized',
    name: 'Solarized',
    themes: [
      { value: 'solarized-dark', name: 'Solarized Dark', sourceName: 'Solarized Dark' },
      { value: 'solarized-light', name: 'Solarized Light', sourceName: 'Solarized Light' }
    ]
  },
  {
    id: 'rose-pine',
    name: 'Rosé Pine',
    themes: [
      { value: 'rose-pine', name: 'Rosé Pine', sourceName: 'Rosé Pine' },
      { value: 'rose-pine-moon', name: 'Rosé Pine Moon', sourceName: 'Rosé Pine Moon' },
      { value: 'rose-pine-dawn', name: 'Rosé Pine Dawn', sourceName: 'Rosé Pine Dawn' }
    ]
  },
  {
    id: 'ayu',
    name: 'Ayu',
    themes: [
      { value: 'ayu-dark', name: 'Ayu Dark', sourceName: 'Ayu Dark' },
      { value: 'ayu-mirage', name: 'Ayu Mirage', sourceName: 'Ayu Mirage' },
      { value: 'ayu-light', name: 'Ayu Light', sourceName: 'Ayu Light' }
    ]
  },
  {
    id: 'kanagawa',
    name: 'Kanagawa',
    themes: [
      { value: 'kanagawa-wave', name: 'Kanagawa Wave', sourceName: 'Kanagawa Wave' },
      { value: 'kanagawa-lotus', name: 'Kanagawa Lotus', sourceName: 'Kanagawa Lotus' }
    ]
  },
  {
    id: 'atom-one',
    name: 'Atom One',
    themes: [
      { value: 'atom-one-dark', name: 'Atom One Dark', sourceName: 'Atom One Dark' },
      { value: 'atom-one-light', name: 'Atom One Light', sourceName: 'Atom One Light' }
    ]
  },
  {
    id: 'everforest',
    name: 'Everforest',
    themes: [
      { value: 'everforest-dark', name: 'Everforest Dark', sourceName: 'Everforest Dark' },
      { value: 'everforest-light', name: 'Everforest Light', sourceName: 'Everforest Light' }
    ]
  },
  {
    id: 'night-owl',
    name: 'Night Owl',
    themes: [
      { value: 'night-owl', name: 'Night Owl', sourceName: 'Night Owl' },
      { value: 'light-owl', name: 'Light Owl', sourceName: 'Light Owl' }
    ]
  },
  {
    id: 'claude',
    name: 'Claude',
    themes: [
      { value: 'claude-dark', name: 'Claude Dark', sourceName: 'Claude Dark' },
      { value: 'claude-light', name: 'Claude Light', sourceName: 'Claude Light' }
    ]
  }
];

export const THEME_PREFERENCES = THEME_GROUPS.flatMap((group) =>
  group.themes.map((theme) => theme.value)
);

export const ADDITIONAL_THEME_NAMES = THEME_GROUPS.flatMap((group) => group.themes)
  .filter((theme): theme is ThemeChoice & { sourceName: string } => !!theme.sourceName)
  .map(({ value, sourceName }) => [value, sourceName] as const);
