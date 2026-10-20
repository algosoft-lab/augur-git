/**
 * Validation for the remote form.
 *
 * The name rules live in `branchName.ts`, because Git checks a remote name the
 * way it checks a branch name. The URL rules are here, because a remote URL is
 * free-form: Git accepts a path, an SSH target, or an HTTP address, so the only
 * thing the form can rule out is an empty one.
 */

export type RemoteUrlError = 'required';

/** The error shown under the URL field, or null when the URL is usable. */
export function remoteUrlError(url: string): RemoteUrlError | null {
  return url.length > 0 && !url.trim() ? 'required' : null;
}

/**
 * Whether a changed URL is worth saving.
 *
 * The save button stays disabled until the URL differs from the one the
 * remote already has, so an accidental Enter does not rewrite the same
 * address.
 */
export function canSaveRemoteUrl(url: string, original: string): boolean {
  const value = url.trim();
  return value.length > 0 && value !== original.trim();
}
