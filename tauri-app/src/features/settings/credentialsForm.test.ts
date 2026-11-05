import { describe, expect, it } from 'vitest';

import type { CredentialEntryView } from '../../bridge/types';
import {
  draftFromEntry,
  draftToInput,
  emptyDraft,
  isDraftComplete,
  storageChoices,
  validateDraft
} from './credentialsForm';

function entry(overrides: Partial<CredentialEntryView> = {}): CredentialEntryView {
  return {
    id: 'abc',
    protocol: 'https',
    host: 'github.com',
    pathPrefix: null,
    username: 'dev',
    scope: { kind: 'global' },
    storage: 'keychain',
    createdAt: 1,
    updatedAt: 2,
    lastUsedAt: null,
    lastFailureAt: null,
    scopeLabel: null,
    secretPresent: true,
    failing: false,
    ...overrides
  };
}

describe('credential draft', () => {
  it('starts empty with the preferred storage', () => {
    const draft = emptyDraft('session', '/home/dev/repo');
    expect(draft.protocol).toBe('https');
    expect(draft.storage).toBe('session');
    expect(draft.repository).toBe('/home/dev/repo');
    expect(isDraftComplete(draft)).toBe(false);
  });

  it('converts a stored entry without loading the secret', () => {
    const draft = draftFromEntry(entry({ pathPrefix: 'team/repo' }));
    expect(draft.password).toBe('');
    expect(draft.pathPrefix).toBe('team/repo');
    expect(draft.repositoryOnly).toBe(false);
  });

  it('keeps a repository scope from the entry', () => {
    const draft = draftFromEntry(
      entry({ scope: { kind: 'repository', location: { kind: 'local' }, path: '/home/dev/work' } })
    );
    expect(draft.repositoryOnly).toBe(true);
    expect(draft.repository).toBe('/home/dev/work');
  });

  it('accepts a pasted remote address as the host', () => {
    const draft = {
      ...emptyDraft('keychain', null),
      host: ' https://GitHub.com/team/repo.git ',
      username: 'dev'
    };
    expect(validateDraft(draft)).toEqual([]);
    expect(draftToInput(draft).host).toBe('GitHub.com');
  });

  it('refuses an empty or space-containing host', () => {
    for (const host of ['', '   ', 'bad host']) {
      const draft = { ...emptyDraft('keychain', null), host, username: 'dev' };
      expect(validateDraft(draft)).toContain('host');
    }
  });

  it('refuses a missing username and an embedded newline', () => {
    const missing = { ...emptyDraft('keychain', null), host: 'github.com' };
    expect(validateDraft(missing)).toContain('username');
    const newline = {
      ...emptyDraft('keychain', null),
      host: 'github.com',
      username: 'dev',
      password: 'one\ntwo'
    };
    expect(validateDraft(newline)).toContain('password');
  });

  it('sends null for a password that was left empty so the stored one is kept', () => {
    const draft = {
      ...emptyDraft('keychain', null),
      host: 'github.com',
      username: 'dev',
      id: 'abc'
    };
    expect(draftToInput(draft).password).toBeNull();
  });

  it('trims the path prefix and drops empty ones', () => {
    const withPath = {
      ...emptyDraft('keychain', null),
      host: 'github.com',
      username: 'dev',
      pathPrefix: ' /team/repo/ '
    };
    expect(draftToInput(withPath).pathPrefix).toBe('team/repo');
    const withoutPath = { ...withPath, pathPrefix: '   ' };
    expect(draftToInput(withoutPath).pathPrefix).toBeNull();
  });

  it('scopes an entry to the repository only when asked', () => {
    const draft = {
      ...emptyDraft('keychain', '/home/dev/work'),
      host: 'github.com',
      username: 'dev',
      repositoryOnly: true
    };
    expect(draftToInput(draft).scope).toEqual({
      kind: 'repository',
      location: { kind: 'local' },
      path: '/home/dev/work'
    });
    expect(draftToInput({ ...draft, repositoryOnly: false }).scope).toEqual({ kind: 'global' });
  });

  it('offers a backend only when it exists', () => {
    expect(storageChoices(false, false)).toEqual(['session']);
    expect(storageChoices(true, false)).toEqual(['keychain', 'session']);
    expect(storageChoices(true, true)).toEqual(['keychain', 'session', 'plaintext']);
  });
});
