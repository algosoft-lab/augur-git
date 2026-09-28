import { describe, expect, it } from 'vitest';

import { validateBranchName } from './branchName';

const existing = ['main', 'feature/one'];

describe('branch name validation', () => {
  it('accepts the names people actually use', () => {
    for (const name of ['dev', 'feature/two', 'v1.2.3', 'fix_bug-1', 'topic+.patch']) {
      expect(validateBranchName(name, existing)).toBeNull();
    }
  });

  it('rejects an empty name', () => {
    expect(validateBranchName('', existing)).toBe('empty');
  });

  it('rejects invalid ref syntax', () => {
    const invalid = [
      '-dev',
      '.hidden',
      'a..b',
      'a b',
      'a~b',
      'a^b',
      'a:b',
      'a?b',
      'a*b',
      'a[b',
      'a\\b',
      'a@{b',
      'a.lock',
      'a/',
      'a.',
      '/a',
      'a//b',
      'a\tb'
    ];
    for (const name of invalid) {
      expect(validateBranchName(name, existing), name).toBe('invalid');
    }
  });

  it('rejects an existing branch', () => {
    expect(validateBranchName('main', existing)).toBe('exists');
    expect(validateBranchName('feature/one', existing)).toBe('exists');
  });

  it('lets a rename keep the old name but not take another one', () => {
    expect(validateBranchName('main', existing, 'main')).toBeNull();
    expect(validateBranchName('feature/one', existing, 'main')).toBe('exists');
  });
});
