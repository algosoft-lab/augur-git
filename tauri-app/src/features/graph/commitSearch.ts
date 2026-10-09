/**
 * Commit search.
 *
 * Messages match fuzzily: after ignoring case, whitespace, underscores, and
 * dashes, every query character must appear in order (a subsequence), so
 * `fxlgn` finds `Fix Login` while a scrambled `ngolxfi` does not.
 *
 * Queries that are at least four hex characters additionally match commits
 * whose full or short hash starts with the query, the way Git abbreviates
 * hashes. The two modes are combined, so a hex-looking word such as `dead`
 * still fuzzy-matches messages and also prefix-matches hashes.
 */

import type { LogRow } from '../../bridge/types';

const MIN_HASH_QUERY_LENGTH = 4;

export type CommitSearchField = 'subject' | 'full';

export function filterCommits(rows: LogRow[], query: string, field: CommitSearchField): LogRow[] {
  if (normalize(query).length === 0) {
    return rows;
  }
  return rows.filter((row) => matches(row, query, field));
}

export function matches(row: LogRow, query: string, field: CommitSearchField): boolean {
  const haystack = field === 'subject' ? row.subject : row.message;
  const needle = normalize(query);
  if (needle.length === 0) {
    return true;
  }
  return (
    subsequence(normalize(haystack), needle) ||
    (isHashQuery(needle) &&
      (row.oid.toLowerCase().startsWith(needle) || row.short.toLowerCase().startsWith(needle)))
  );
}

function subsequence(haystack: string, needle: string): boolean {
  let from = 0;
  for (const character of needle) {
    from = haystack.indexOf(character, from);
    if (from === -1) {
      return false;
    }
    from += 1;
  }
  return true;
}

function isHashQuery(needle: string): boolean {
  return needle.length >= MIN_HASH_QUERY_LENGTH && /^[0-9a-f]+$/.test(needle);
}

function normalize(value: string): string {
  let out = '';
  for (const character of value) {
    if (/\s/.test(character) || character === '_' || character === '-') {
      continue;
    }
    out += character.toLowerCase();
  }
  return out;
}
