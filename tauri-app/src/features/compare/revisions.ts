/**
 * Revision-selection rules shared by the compare window.
 *
 * The reference tracks an "unavailable" flag on its revision picker: a chosen
 * named ref that has dropped out of the repository's revision list (a branch
 * deleted elsewhere, a tag pruned) is not an invalid entry, it is a real
 * revision this repository can no longer resolve. Commits are exempt, because
 * a typed object id never comes from the list in the first place.
 */

import type { CompareRevision } from "../../bridge/types";

/** Filter refs case-insensitively and rank close name matches first. */
export function filterRevisions(
  options: CompareRevision[],
  query: string,
): CompareRevision[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return options.filter((option) => option.kind !== "commit");
  }

  const matches = options.flatMap((option, index) => {
    if (option.kind === "commit") {
      return [];
    }
    const score = Math.min(
      fuzzyMatchScore(option.name, needle),
      fuzzyMatchScore(option.full_name, needle),
    );
    return Number.isFinite(score) ? [{ option, index, score }] : [];
  });

  return matches
    .sort((left, right) => left.score - right.score || left.index - right.index)
    .map((match) => match.option);
}

function fuzzyMatchScore(candidate: string, needle: string): number {
  const value = candidate.toLowerCase();
  if (value === needle) {
    return 0;
  }
  if (value.startsWith(needle)) {
    return 1;
  }
  const substringIndex = value.indexOf(needle);
  if (substringIndex >= 0) {
    return 2 + substringIndex / Math.max(1, value.length);
  }

  let queryIndex = 0;
  let firstMatch = -1;
  let lastMatch = -1;
  for (let index = 0; index < value.length && queryIndex < needle.length; index += 1) {
    if (value[index] === needle[queryIndex]) {
      firstMatch = firstMatch < 0 ? index : firstMatch;
      lastMatch = index;
      queryIndex += 1;
    }
  }
  if (queryIndex !== needle.length) {
    return Number.POSITIVE_INFINITY;
  }
  const gaps = lastMatch - firstMatch + 1 - needle.length;
  return 3 + gaps / Math.max(1, value.length) + firstMatch / Math.max(1, value.length);
}

export function isRevisionUnavailable(
  selected: CompareRevision | null,
  options: CompareRevision[],
): boolean {
  return (
    selected !== null &&
    selected.kind !== "commit" &&
    !options.some((option) => option.full_name === selected.full_name)
  );
}
