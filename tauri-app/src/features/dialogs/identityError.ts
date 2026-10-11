/**
 * Recognize Git's "who are you" failures.
 *
 * A commit without a configured identity fails after Git has done all the
 * work of building the commit, and its message says so in one of a few
 * phrasings that changed across Git versions. Matching them turns a dead end
 * into a dialog that fixes the cause. Kept as a pure function so the phrasing
 * list is testable without a repository.
 */

const IDENTITY_ERROR_PATTERN =
  /author identity unknown|committer identity unknown|please tell me who you are|unable to auto-detect (?:e-?mail|name)/i;

/** Whether a failed command's message is Git refusing for lack of identity. */
export function isGitIdentityError(message: string): boolean {
  return IDENTITY_ERROR_PATTERN.test(message);
}
