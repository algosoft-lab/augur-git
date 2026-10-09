import { describe, expect, it } from 'vitest';

import { matches, filterCommits } from './commitSearch';
import type { LogRow } from '../../bridge/types';

function row(subject: string, message: string, oid = 'a'.repeat(40), short = 'aaaaaaa'): LogRow {
  return {
    oid,
    short,
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

  it('does not match when query characters are out of order', () => {
    expect(matches(row('Fix Login', 'Fix Login'), 'ngolxfi', 'subject')).toBe(false);
  });

  it('matches a subsequence of the message', () => {
    const commit = row('Fix Login', 'Fix Login');
    expect(matches(commit, 'fxlgn', 'subject')).toBe(true);
    expect(matches(commit, 'fx', 'subject')).toBe(true);
    expect(matches(commit, 'login', 'subject')).toBe(true);
  });

  it('does not match characters missing from the message', () => {
    expect(matches(row('Fix Login', 'Fix Login'), 'fixz', 'subject')).toBe(false);
  });

  it('only searches the body in full-message mode', () => {
    const commit = row('Release', 'Release\n\nEnable SSO login');
    expect(matches(commit, 'sso', 'subject')).toBe(false);
    expect(matches(commit, 'sso', 'full')).toBe(true);
  });

  it('matches a commit by short hash prefix', () => {
    const commit = row('Bump version', 'Bump version', `${'a1b2c3d'.padEnd(40, '0')}`, 'a1b2c3d');
    expect(matches(commit, 'a1b2c3d', 'subject')).toBe(true);
    expect(matches(commit, 'A1B2C3D', 'subject')).toBe(true);
  });

  it('matches a commit by full hash prefix beyond short hash length', () => {
    const oid = 'a1b2c3d4e5'.padEnd(40, '0');
    const commit = row('Bump version', 'Bump version', oid, 'a1b2c3d');
    expect(matches(commit, 'a1b2c3d4e5', 'subject')).toBe(true);
    expect(matches(commit, 'a1b2c3d4e5f', 'subject')).toBe(false);
  });

  it('requires four hex characters before hash matching applies', () => {
    const oid = 'abc'.padEnd(40, '0');
    const commit = row('Unrelated message', 'Unrelated message', oid, 'abc0000');
    expect(matches(commit, 'abc', 'subject')).toBe(false);
  });

  it('does not treat non-hex queries as hashes', () => {
    const oid = 'xyzz'.padEnd(40, '0');
    const commit = row('Unrelated message', 'Unrelated message', oid, 'xyzz0000');
    expect(matches(commit, 'xyz', 'subject')).toBe(false);
  });

  it('combines message and hash matching for hex-looking words', () => {
    const byMessage = row('Handle deadlock safely', 'Handle deadlock safely');
    const byHash = row(
      'Unrelated message',
      'Unrelated message',
      'dead'.padEnd(40, '0'),
      'dead000'
    );
    expect(matches(byMessage, 'dead', 'subject')).toBe(true);
    expect(matches(byHash, 'dead', 'subject')).toBe(true);
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
