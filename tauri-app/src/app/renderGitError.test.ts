/**
 * Rendering a backend error.
 *
 * A failure arrives as a catalog key and a detail, and the two are not the same
 * shape. Most keys carry the detail inside a sentence, and a few are complete on
 * their own with nothing to add. A renderer that assumes one shape silently
 * drops half the information in the other case, which is why both are asserted
 * here rather than only the convenient one.
 */

import { describe, expect, it } from 'vitest';

import { renderGitError } from '../app/store';

/** A translator over a two-entry catalog, so a test cannot pass by accident. */
const CATALOG: Record<string, string> = {
  'err-git-run': 'Failed to run git: { $detail }',
  'err-repo-closed': 'This repository tab is no longer open.',
  'wsl-path-not-absolute': 'Enter an absolute Linux path starting with /'
};

/** Returns the key itself when the catalog does not define it, as the app does. */
const translate = ((key: string) => CATALOG[key] ?? key) as Parameters<typeof renderGitError>[0];

describe('renderGitError', () => {
  it('substitutes the detail into a key that has a slot for it', () => {
    expect(renderGitError(translate, 'err-git-run', 'fatal: not a git repository')).toBe(
      'Failed to run git: fatal: not a git repository'
    );
  });

  it('leaves a key that is a whole sentence alone', () => {
    // `err-repo-closed` has no placeholder, so appending the detail would read
    // as a second, redundant sentence.
    expect(renderGitError(translate, 'err-repo-closed', 'repository 7 is gone')).toBe(
      'This repository tab is no longer open.'
    );
  });

  it('leaves a path reason alone, which is the same case from the other side', () => {
    // The WSL dialog's own reasons are written to be read on their own, so the
    // renderer has to cope with a key and no detail rather than assume both.
    expect(renderGitError(translate, 'wsl-path-not-absolute', '')).toBe(
      'Enter an absolute Linux path starting with /'
    );
  });

  it('keeps only the first line of a multi-line detail', () => {
    // Git's stderr routinely carries a hint after the error, and a status line
    // has room for one sentence.
    expect(renderGitError(translate, 'err-git-run', 'fatal: bad object\nhint: try --all')).toBe(
      'Failed to run git: fatal: bad object'
    );
  });

  it('degrades to the key rather than showing nothing', () => {
    // A key the catalog does not define is a defect, but the reader still needs
    // to see what happened rather than an empty status line.
    expect(renderGitError(translate, 'err-from-the-future', 'something went wrong')).toBe(
      'err-from-the-future: something went wrong'
    );
    expect(renderGitError(translate, 'err-from-the-future', '')).toBe('err-from-the-future');
  });
});
