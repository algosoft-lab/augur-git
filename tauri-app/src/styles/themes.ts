/**
 * The five bundled themes.
 *
 * The values are a transcription of the reference application's theme catalog so
 * the two products look identical. Every color becomes a CSS custom property,
 * which keeps the stylesheets free of theme conditionals.
 */

import type { ThemePreference } from "../bridge/types";

export interface ThemeDefinition {
  mode: "light" | "dark";
  colors: Record<string, string>;
}

export const THEMES: Record<ThemePreference, ThemeDefinition> = {
  "github-dark": {
    "mode": "dark",
    "colors": {
      "background": "#0D1117",
      "foreground": "#E6EDF3",
      "border": "#30363D",
      "tab_bar.background": "#161B22",
      "title_bar.background": "#161B22",
      "input.border": "#21262D",
      "list.hover.background": "#21262D",
      "list.active.background": "#264F78",
      "muted.foreground": "#8B949E",
      "table.head.foreground": "#8B949E",
      "base.blue": "#2F81F7",
      "accent.background": "#2F81F7",
      "accent.foreground": "#0D1117",
      "base.green": "#3FB950",
      "base.red": "#F85149",
      "warning.background": "#D29922",
      "drag.border": "#388BFD",
      "primary.background": "#2F81F7",
      "primary.foreground": "#FFFFFF",
      "switch.background": "#3D444D",
      "switch.thumb.background": "#FFFFFF"
    }
  },
  "catppuccin-latte": {
    "mode": "light",
    "colors": {
      "background": "#eff1f5",
      "foreground": "#4c4f69",
      "border": "#ccd0da",
      "tab_bar.background": "#e6e9ef",
      "title_bar.background": "#e6e9ef",
      "input.border": "#ccd0da",
      "list.hover.background": "#ccd0da",
      "list.active.background": "#bcc0cc",
      "muted.foreground": "#9ca0b0",
      "table.head.foreground": "#6c6f85",
      "base.blue": "#1e66f5",
      "accent.background": "#1e66f5",
      "accent.foreground": "#ffffff",
      "base.green": "#40a02b",
      "base.red": "#d20f39",
      "warning.background": "#df8e1d",
      "drag.border": "#1e66f5",
      "primary.background": "#1e66f5",
      "primary.foreground": "#ffffff",
      "switch.background": "#ccd0da",
      "switch.thumb.background": "#ffffff"
    }
  },
  "catppuccin-frappe": {
    "mode": "dark",
    "colors": {
      "background": "#303446",
      "foreground": "#c6d0f5",
      "border": "#414559",
      "tab_bar.background": "#292c3c",
      "title_bar.background": "#292c3c",
      "input.border": "#414559",
      "list.hover.background": "#414559",
      "list.active.background": "#51576d",
      "muted.foreground": "#737994",
      "table.head.foreground": "#a5adce",
      "base.blue": "#8caaee",
      "accent.background": "#8caaee",
      "accent.foreground": "#303446",
      "base.green": "#a6d189",
      "base.red": "#e78284",
      "warning.background": "#e5c890",
      "drag.border": "#8caaee",
      "primary.background": "#8caaee",
      "primary.foreground": "#303446",
      "switch.background": "#737994",
      "switch.thumb.background": "#c6d0f5"
    }
  },
  "catppuccin-macchiato": {
    "mode": "dark",
    "colors": {
      "background": "#24273a",
      "foreground": "#cad3f5",
      "border": "#363a4f",
      "tab_bar.background": "#1e2030",
      "title_bar.background": "#1e2030",
      "input.border": "#363a4f",
      "list.hover.background": "#363a4f",
      "list.active.background": "#494d64",
      "muted.foreground": "#6e738d",
      "table.head.foreground": "#a5adcb",
      "base.blue": "#8aadf4",
      "accent.background": "#8aadf4",
      "accent.foreground": "#24273a",
      "base.green": "#a6da95",
      "base.red": "#ed8796",
      "warning.background": "#eed49f",
      "drag.border": "#8aadf4",
      "primary.background": "#8aadf4",
      "primary.foreground": "#24273a",
      "switch.background": "#6e738d",
      "switch.thumb.background": "#cad3f5"
    }
  },
  "catppuccin-mocha": {
    "mode": "dark",
    "colors": {
      "background": "#1e1e2e",
      "foreground": "#cdd6f4",
      "border": "#313244",
      "tab_bar.background": "#181825",
      "title_bar.background": "#181825",
      "input.border": "#313244",
      "list.hover.background": "#313244",
      "list.active.background": "#45475a",
      "muted.foreground": "#6c7086",
      "table.head.foreground": "#a6adc8",
      "base.blue": "#89b4fa",
      "accent.background": "#89b4fa",
      "accent.foreground": "#1e1e2e",
      "base.green": "#a6e3a1",
      "base.red": "#f38ba8",
      "warning.background": "#f9e2af",
      "drag.border": "#89b4fa",
      "primary.background": "#89b4fa",
      "primary.foreground": "#1e1e2e",
      "switch.background": "#6c7086",
      "switch.thumb.background": "#cdd6f4"
    }
  }
}

/**
 * CSS variable name for one catalog key.
 *
 * The catalog spells nesting with a dot and words with an underscore, while the
 * stylesheets address everything with dashes, so both separators are folded.
 */
export function cssVariable(key: string): string {
  return `--${key.replace(/[._\s]/g, "-")}`;
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
  },
): void {
  const definition = THEMES[theme];
  for (const [key, value] of Object.entries(definition.colors)) {
    root.style.setProperty(cssVariable(key), value);
  }
  root.style.setProperty("--ui-font-family", typography.uiFontFamily ?? "system-ui");
  root.style.setProperty("--mono-font-family", typography.monoFontFamily ?? "ui-monospace");
  root.style.setProperty("--ui-font-size", `${typography.uiFontSize}px`);
  root.style.setProperty("--diff-font-size", `${typography.diffFontSize}px`);
  root.dataset.mode = definition.mode;
  root.style.colorScheme = definition.mode;
}

/** The theme used before the preferences have been read. */
export const DEFAULT_THEME: ThemePreference = "catppuccin-mocha";

/** The typography used before the preferences have been read. */
export const DEFAULT_TYPOGRAPHY = {
  uiFontFamily: null,
  monoFontFamily: null,
  uiFontSize: 16,
  diffFontSize: 16,
};

/** The ten lane colors the commit graph cycles through. */
export const LANE_COLORS = [
  "var(--base-blue)",
  "var(--base-green)",
  "var(--warning-background)",
  "var(--base-red)",
  "#a371f7",
  "#39c5cf",
  "#e06c9f",
  "#7dba00",
  "#d19a66",
  "#4993f0",
];
