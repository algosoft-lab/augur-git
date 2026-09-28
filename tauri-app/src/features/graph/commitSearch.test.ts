import { describe, expect, it } from 'vitest';

import { matches, filterCommits } from './commitSearch';
import type { LogRow } from '../../bridge/types';

function row(subject: string, message: string): LogRow {
  return {
    oid: 'a'.repeat(40),
    short: 'aaaaaaa',
    author: 'Author',
    date: '2026-01-01 00:00',
    timestamp: 0,
    subject,
    message,
    decorations: '',
    parents: []
  };
}

describe('commit search', () => {
  it('ignores case, whitespace, underscores, and dashes', () => {
    const commit = row('Fix Login', 'Fix Login\n\nAllow SSO');
    expect(matches(commit, 'fix_login', 'subject')).toBe(true);
    expect(matches(commit, 'FIX-LOGIN', 'subject')).toBe(true);
    expect(matches(commit, 'fix login', 'subject')).toBe(true);
  });

  it('does not correct a typo', () => {
    expect(matches(row('Fix Login', 'Fix Login'), 'fix_lgoin', 'subject')).toBe(false);
  });

  it('only searches the body in full-message mode', () => {
    const commit = row('Release', 'Release\n\nEnable SSO login');
    expect(matches(commit, 'sso_login', 'subject')).toBe(false);
    expect(matches(commit, 'sso_login', 'full')).toBe(true);
  });

  it('treats an empty query as no filter', () => {
    const rows = [row('One', 'One'), row('Two', 'Two')];
    expect(filterCommits(rows, '', 'subject')).toHaveLength(2);
    expect(filterCommits(rows, '___', 'subject')).toHaveLength(2);
  });

  it('preserves the Git order of the matches', () => {
    const rows = [row('alpha', ''), row('beta', ''), row('gamma', '')];
    expect(filterCommits(rows, 'a', 'subject').map((r) => r.subject)).toEqual([
      'alpha',
      'beta',
      'gamma'
    ]);
  });
});
