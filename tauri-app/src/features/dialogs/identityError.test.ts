import { describe, expect, it } from 'vitest';

import { isGitIdentityError } from './identityError';

describe('isGitIdentityError', () => {
  it('recognizes the phrasings modern and older Git releases print', () => {
    expect(
      isGitIdentityError(
        'Author identity unknown\n\n*** Please tell me who you are.\n\nRun\n\n  git config --global user.email "you@example.com"\n  git config --global user.name "Your Name"'
      )
    ).toBe(true);
    expect(isGitIdentityError("fatal: unable to auto-detect email address (got 'x@y(none)')")).toBe(
      true
    );
    expect(isGitIdentityError("fatal: unable to auto-detect name (got 'dev')")).toBe(true);
    expect(
      isGitIdentityError('Committer identity unknown\n\n*** Please tell me who you are.')
    ).toBe(true);
  });

  it('does not claim unrelated failures', () => {
    expect(
      isGitIdentityError('fatal: not a git repository (or any of the parent directories)')
    ).toBe(false);
    expect(isGitIdentityError('error: failed to push some refs')).toBe(false);
    expect(isGitIdentityError('')).toBe(false);
  });

  it('ignores case', () => {
    expect(isGitIdentityError('AUTHOR IDENTITY UNKNOWN')).toBe(true);
  });
});
