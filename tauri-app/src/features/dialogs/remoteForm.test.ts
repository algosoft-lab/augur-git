import { describe, expect, it } from 'vitest';

import { canSaveRemoteUrl, remoteUrlError } from './remoteForm';

describe('remote URL validation', () => {
  it('accepts an empty field, which the disabled button already reports', () => {
    expect(remoteUrlError('')).toBeNull();
  });

  it('accepts a URL', () => {
    expect(remoteUrlError('https://example.com/owner/repository.git')).toBeNull();
  });

  it('rejects a field that holds nothing but spaces', () => {
    expect(remoteUrlError('   ')).toBe('required');
  });

  it('accepts a URL with surrounding spaces', () => {
    expect(remoteUrlError('  https://example.com/repo.git ')).toBeNull();
  });
});

describe('remote URL saving', () => {
  it('refuses an empty URL', () => {
    expect(canSaveRemoteUrl('', 'https://example.com/repo.git')).toBe(false);
  });

  it('refuses a URL that did not change', () => {
    expect(canSaveRemoteUrl('https://example.com/repo.git', 'https://example.com/repo.git')).toBe(
      false
    );
  });

  it('accepts a URL that changed', () => {
    expect(canSaveRemoteUrl('git@example.com:repo.git', 'https://example.com/repo.git')).toBe(true);
  });

  it('compares the trimmed value, so trailing spaces are not a change', () => {
    expect(
      canSaveRemoteUrl('  https://example.com/repo.git  ', 'https://example.com/repo.git')
    ).toBe(false);
  });
});
