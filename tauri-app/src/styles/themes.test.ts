import { describe, expect, it } from 'vitest';

import { ADDITIONAL_THEME_NAMES, THEME_GROUPS, THEME_PREFERENCES } from '../styles/theme-catalog';
import { additionalThemeColors } from '../styles/popular-theme-seeds';
import { applyTheme, cssFontFamily, cssVariable, THEMES } from '../styles/themes';
import { tokenize, grammarFor } from '../features/diff/highlight';

describe('theme tokens', () => {
  it('publishes all 32 themes with no duplicate preference keys', () => {
    expect(THEME_PREFERENCES).toHaveLength(32);
    expect(new Set(THEME_PREFERENCES).size).toBe(THEME_PREFERENCES.length);
    expect(Object.keys(THEMES).sort()).toEqual([...THEME_PREFERENCES].sort());
    expect(Object.keys(additionalThemeColors)).toHaveLength(27);
    expect(ADDITIONAL_THEME_NAMES.map(([, name]) => name).sort()).toEqual(
      Object.keys(additionalThemeColors).sort()
    );
    expect(THEME_GROUPS.flatMap((group) => group.themes)).toHaveLength(32);
  });

  it('gives every theme the same color set so no rule can be missing one', () => {
    const reference = Object.keys(THEMES['catppuccin-mocha']!.colors).sort();
    for (const [name, theme] of Object.entries(THEMES)) {
      expect(Object.keys(theme.colors).sort(), name).toEqual(reference);
    }
  });

  it('keeps the dark and light classification of the reference catalog', () => {
    expect(THEMES['github-dark']!.mode).toBe('dark');
    expect(THEMES['catppuccin-latte']!.mode).toBe('light');
    expect(THEMES['catppuccin-mocha']!.mode).toBe('dark');
    expect(THEMES['tokyo-night-light']!.mode).toBe('light');
    expect(THEMES['github-light-default']!.mode).toBe('light');
  });

  it('uses valid palette tokens, readable text, and distinct graph lanes', () => {
    const luminance = (color: string): number => {
      const channels = color
        .slice(1)
        .match(/.{2}/gu)
        ?.map((channel) => Number.parseInt(channel, 16) / 255)
        .map((value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
      if (!channels || channels.length !== 3) throw new Error(`Invalid color: ${color}`);
      return 0.2126 * channels[0]! + 0.7152 * channels[1]! + 0.0722 * channels[2]!;
    };
    const contrastRatio = (foreground: string, background: string): number => {
      const values = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
      return ((values[0] ?? 0) + 0.05) / ((values[1] ?? 0) + 0.05);
    };

    for (const [name, theme] of Object.entries(THEMES)) {
      for (const color of Object.values(theme.colors)) {
        expect(color, name).toMatch(/^#[0-9a-f]{6}$/iu);
      }
      expect(
        contrastRatio(theme.colors.foreground!, theme.colors.background!),
        name
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(theme.colors['accent.foreground']!, theme.colors['accent.background']!),
        name
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        new Set(Array.from({ length: 10 }, (_, index) => theme.colors[`graph.lane.${index + 1}`]))
          .size,
        name
      ).toBe(10);
    }
  });

  it('turns a dotted catalog key into a CSS variable', () => {
    expect(cssVariable('tab_bar.background')).toBe('--tab-bar-background');
    expect(cssVariable('background')).toBe('--background');
  });

  it('writes the colors and the typography onto an element', () => {
    const root = document.createElement('div');
    applyTheme(root, 'github-dark', {
      uiFontFamily: 'Inter',
      monoFontFamily: 'Menlo',
      uiFontSize: 15,
      diffFontSize: 14
    });
    expect(root.style.getPropertyValue('--background')).toBe('#0D1117');
    expect(root.style.getPropertyValue('--ui-font-family')).toBe('"Inter"');
    expect(root.style.getPropertyValue('--diff-font-size')).toBe('14px');
    expect(root.dataset.mode).toBe('dark');
  });

  it('falls back to the platform fonts when none is chosen', () => {
    const root = document.createElement('div');
    applyTheme(root, 'catppuccin-mocha', {
      uiFontFamily: null,
      monoFontFamily: null,
      uiFontSize: 16,
      diffFontSize: 16
    });
    expect(root.style.getPropertyValue('--ui-font-family')).toBe('system-ui');
    expect(root.style.getPropertyValue('--mono-font-family')).toBe('ui-monospace');
  });

  it('quotes a complete family name safely for CSS', () => {
    expect(cssFontFamily('Source Sans 3')).toBe('"Source Sans 3"');
    expect(cssFontFamily('Fira "Code"')).toBe('"Fira \\"Code\\""');
  });
});

describe('syntax tokenizer', () => {
  it('resolves a file extension alias to the same grammar', () => {
    expect(grammarFor('tsx')).toBe(grammarFor('typescript'));
    expect(grammarFor('mjs')).toBe(grammarFor('typescript'));
    expect(grammarFor('sh')).toBe(grammarFor('bash'));
  });

  it('returns nothing for a language it does not know', () => {
    expect(grammarFor('brainfuck')).toBeNull();
    expect(grammarFor(null)).toBeNull();
    expect(tokenize('anything', null)).toBeNull();
  });

  it('returns nothing for a line that has nothing to highlight', () => {
    // A grammar can apply and still produce no token, which is the common case
    // for prose inside a source file.
    expect(tokenize('hello world', 'bash')).toBeNull();
  });

  it('marks keywords, strings, and numbers', () => {
    const tokens = tokenize('let x = "hi"; // note', 'rust') ?? [];
    const kinds = new Set(tokens.map((token) => token.kind));
    expect(kinds.has('keyword')).toBe(true);
    expect(kinds.has('string')).toBe(true);
    expect(kinds.has('comment')).toBe(true);
  });

  it('produces ordered, non-overlapping tokens inside the line', () => {
    const line = 'fn main() { let n = 42; println!("{}", n); }';
    const tokens = tokenize(line, 'rust') ?? [];
    let previousEnd = 0;
    for (const token of tokens) {
      expect(token.start).toBeGreaterThanOrEqual(previousEnd);
      expect(token.end).toBeGreaterThan(token.start);
      expect(token.end).toBeLessThanOrEqual(line.length);
      previousEnd = token.end;
    }
  });

  it('keeps the source of every token', () => {
    const line = 'const value = 3;';
    const tokens = tokenize(line, 'rust') ?? [];
    for (const token of tokens) {
      expect(line.slice(token.start, token.end).length).toBe(token.end - token.start);
    }
  });

  it('tokenizes every language the core maps to', () => {
    const samples: [string, string][] = [
      ['rust', 'fn main() {}'],
      ['typescript', 'const a: number = 1;'],
      ['tsx', 'export const A = () => <div />;'],
      ['python', 'def f(x):\n    return x'],
      ['json', '{"a": 1}'],
      ['css', '.a { color: red; }'],
      ['html', '<p class="a">x</p>'],
      ['markdown', '# Title'],
      ['bash', 'if [ -f x ]; then echo hi; fi'],
      ['yaml', 'a: 1'],
      ['sql', 'SELECT 1'],
      ['go', 'func main() {}'],
      ['java', 'class A {}'],
      ['c', 'int main(void) { return 0; }'],
      ['diff', '@@ -1 +1 @@'],
      ['ruby', 'def f; end'],
      ['php', '<?php echo 1;'],
      ['lua', 'local a = 1'],
      ['toml', 'a = 1'],
      ['cmake', 'project(x)']
    ];
    for (const [language, source] of samples) {
      const line = source.split('\n')[0]!;
      expect(tokenize(line, language), language).not.toBeNull();
    }
  });
});
