/**
 * How a failed command is read.
 *
 * The backend reports the label of the operation it ran, and the label names
 * the target of an operation when it has one. A remote-scoped pull is
 * therefore `pull origin main` rather than `pull`, so the kind of conflict a
 * failure can leave behind is read from the operation instead of matched in
 * full.
 */

/** The kind of conflict a failed command can leave behind. */
export type ConflictKind = 'merge' | 'rebase' | 'stash';

export function conflictKind(label: string): ConflictKind | null {
  if (label === 'stash pop') {
    return 'stash';
  }
  if (label === 'rebase' || label === 'pull --rebase') {
    return 'rebase';
  }
  if (label === 'merge' || label === 'merge --no-ff' || label === 'pull') {
    return 'merge';
  }
  // A pull that names a remote and a branch is one of the remote-scoped
  // variants, and the rebase one is checked first because it starts with the
  // same word.
  if (label.startsWith('pull --rebase ')) {
    return 'rebase';
  }
  if (label.startsWith('pull ')) {
    return 'merge';
  }
  return null;
}
