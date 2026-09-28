import { describe, expect, it } from 'vitest';

import { createTranslator, resolveLocale } from './index';

const CATALOG = {
  'toolbar-refresh': 'Refresh',
  'rel-min': '{ $n } minutes ago',
  'merge-warning': 'Rebasing { $branch } rewrites { $count } commits'
};

describe('translator', () => {
  it('returns the catalog entry', () => {
    expect(createTranslator(CATALOG)('toolbar-refresh')).toBe('Refresh');
  });

  it('returns the key when the catalog does not have it', () => {
    expect(createTranslator(CATALOG)('missing-key')).toBe('missing-key');
  });

  it('substitutes named arguments', () => {
    const t = createTranslator(CATALOG);
    expect(t('rel-min', { n: 5 })).toBe('5 minutes ago');
    expect(t('merge-warning', { branch: 'main', count: 3 })).toBe(
      'Rebasing main rewrites 3 commits'
    );
  });

  it('leaves a placeholder alone when no value is supplied', () => {
    expect(createTranslator(CATALOG)('rel-min')).toBe('{ $n } minutes ago');
  });

  it('substitutes every occurrence of a name', () => {
    const t = createTranslator({ pair: '{ $x } and { $x }' });
    expect(t('pair', { x: 'a' })).toBe('a and a');
  });
});

describe('locale resolution', () => {
  it('honours an explicit preference', () => {
    expect(resolveLocale('en-US', 'zh-CN')).toBe('en-US');
    expect(resolveLocale('zh-CN', 'en-US')).toBe('zh-CN');
  });

  it('maps any Chinese system language to the Chinese catalog', () => {
    expect(resolveLocale('system', 'zh')).toBe('zh-CN');
    expect(resolveLocale('system', 'zh-Hans-CN')).toBe('zh-CN');
    expect(resolveLocale('system', 'zh-TW')).toBe('zh-CN');
  });

  it('falls back to English for every other system language', () => {
    expect(resolveLocale('system', 'en-GB')).toBe('en-US');
    expect(resolveLocale('system', 'de-DE')).toBe('en-US');
    expect(resolveLocale('system', '')).toBe('en-US');
  });
});
