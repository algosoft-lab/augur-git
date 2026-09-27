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
