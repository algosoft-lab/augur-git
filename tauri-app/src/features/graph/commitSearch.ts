/**
 * Commit search.
 *
 * Matching is deliberately loose: case, whitespace, underscores, and dashes are
 * ignored, so `fix_login` finds `Fix-Login`. It is not fuzzy and never corrects a
 * typo, which is what the reference application does.
 */

import type { LogRow } from "../../bridge/types";

export type CommitSearchField = "subject" | "full";

export function filterCommits(
  rows: LogRow[],
  query: string,
  field: CommitSearchField,
): LogRow[] {
  if (normalize(query).length === 0) {
    return rows;
  }
  return rows.filter((row) => matches(row, query, field));
}

export function matches(
  row: LogRow,
  query: string,
  field: CommitSearchField,
): boolean {
  const haystack = field === "subject" ? row.subject : row.message;
  const needle = normalize(query);
  if (needle.length === 0) {
    return true;
  }
  return normalize(haystack).includes(needle);
}

function normalize(value: string): string {
  let out = "";
  for (const character of value) {
    if (/\s/.test(character) || character === "_" || character === "-") {
      continue;
    }
    out += character.toLowerCase();
  }
  return out;
}
