import { describe, expect, it } from 'vitest';

import { validateGitFields } from './GitConfigSettings';

const emptyFields = () => ({
  'user.name': '',
  'user.email': '',
  'init.defaultBranch': '',
  'core.autocrlf': '',
  'core.editor': ''
});

describe('validateGitFields', () => {
  it('accepts unset fields', () => {
    expect(validateGitFields(emptyFields())).toEqual({});
  });

  it('requires an email to look like one', () => {
    expect(validateGitFields({ ...emptyFields(), 'user.email': 'ada@example.com' })).toEqual({});
    expect(validateGitFields({ ...emptyFields(), 'user.email': 'not-an-email' })).toEqual({
      'user.email': 'git-identity-email-required'
    });
    expect(validateGitFields({ ...emptyFields(), 'user.email': 'a b@c' })).toEqual({
      'user.email': 'git-identity-email-required'
    });
  });

  it('rejects branch names Git would refuse', () => {
    expect(validateGitFields({ ...emptyFields(), 'init.defaultBranch': 'feature/one' })).toEqual(
      {}
    );
    for (const rejected of ['-branch', 'a b', 'a..b', 'head.lock', '/root', 'branch/']) {
      expect(validateGitFields({ ...emptyFields(), 'init.defaultBranch': rejected })).toEqual({
        'init.defaultBranch': 'git-default-branch-invalid'
      });
    }
  });
});
