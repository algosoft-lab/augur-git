/** Helpers for the changed-file lists. */

import type { FileChange, FileChangeStatus } from "../../bridge/types";

/**
 * Split an add/delete count into the five-segment bar.
 *
 * Additions are rounded up so that any addition is still visible, which is what
 * the reference application does.
 */
export function statBlocks(
  added: number | null,
  deleted: number | null,
): { added: number; deleted: number } {
  const TOTAL = 5;
  const add = added ?? 0;
  const del = deleted ?? 0;
  const total = add + del;
  if (total === 0) {
    return { added: 0, deleted: 0 };
  }
  const green = Math.min(TOTAL, Math.ceil((add * TOTAL) / total));
  return { added: green, deleted: TOTAL - green };
}

/** A file with no line counts is binary, matching the parser. */
export function isBinary(file: FileChange): boolean {
  return file.added === null || file.deleted === null;
}

/** The catalog key for a change status. */
export function statusKey(status: FileChangeStatus): string {
  return `status-${statusModifier(status)}`;
}

/**
 * The CSS colour modifier for a change status.
 *
 * The name is derived from the status so the two cannot drift apart, which is
 * what happened when the two mappings were written separately.
 */
export function statusModifier(status: FileChangeStatus): string {
  switch (status) {
    case "added":
      return "add";
    case "deleted":
      return "del";
    case "modified":
      return "mod";
    case "renamed":
      return "ren";
    case "copied":
      return "cpy";
    case "unmerged":
      return "conflict";
    default:
      return "unknown";
  }
}
