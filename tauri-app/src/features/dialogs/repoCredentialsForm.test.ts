import { describe, expect, it } from 'vitest';

import type { RepoLocalCredentialConfig, RepoRemoteCredentialInfo } from '../../bridge/types';
import {
  isLocalDraftComplete,
  isLocalDraftDirty,
  localDraftFromConfig,
  localDraftToInput,
  previewRemoteUrlWithUsername,
  validateLocalDraft,
  vaultDraftFromRemote
} from './repoCredentialsForm';

function remote(overrides: Partial<RepoRemoteCredentialInfo> = {}): RepoRemoteCredentialInfo {
  return {
    name: 'origin',
    url: 'https://github.com/org/repo.git',
    isHttp: true,
    protocol: 'https',
    host: 'github.com',
    path: 'org/repo.git',
    urlUsername: null,
    matchedVaultEntryId: null,
    matchedVaultScope: null,
    matchedVaultUsername: null,
    hasUrlHelperConflict: false,
    ...overrides
  };
}

function localConfig(overrides: Partial<RepoLocalCredentialConfig> = {}): RepoLocalCredentialConfig {
  return {
    mode: 'inherit',
    useHttpPath: null,
    username: null,
    localHelpers: [],
    storeFile: null,
    suggestedStoreFile: '/home/dev/.git-credentials-repo',
    effectiveHelpers: [],
    localUserName: null,
    localUserEmail: null,
    effectiveUserName: 'Dev',
    effectiveUserEmail: 'dev@example.com',
    ...overrides
  };
}

describe('repoCredentialsForm', () => {
  it('pre-fills repository-scoped vault draft from the selected HTTP remote', () => {
    const draft = vaultDraftFromRemote(
      remote({ urlUsername: 'work_user' }),
      'keychain',
      '/home/dev/work'
    );
    expect(draft.protocol).toBe('https');
    expect(draft.host).toBe('github.com');
    expect(draft.pathPrefix).toBe('org/repo.git');
    expect(draft.username).toBe('work_user');
    expect(draft.repositoryOnly).toBe(true);
    expect(draft.repository).toBe('/home/dev/work');
  });

  it('requires a store file in isolated-store mode and username when writing a password', () => {
    const draft = {
      ...localDraftFromConfig(localConfig()),
      mode: 'isolated-store' as const,
      storeFile: '   ',
      storePassword: 'token'
    };
    const issues = validateLocalDraft(draft);
    expect(issues).toContain('storeFile');
    expect(issues).toContain('username');
    expect(isLocalDraftComplete(draft)).toBe(false);
  });

  it('validates email format only when a local email is provided', () => {
    const validEmpty = localDraftFromConfig(localConfig());
    expect(validateLocalDraft(validEmpty)).toEqual([]);

    const badEmail = { ...validEmpty, localUserEmail: 'not-an-email' };
    expect(validateLocalDraft(badEmail)).toContain('localUserEmail');
  });

  it('tracks dirty state accurately across fields', () => {
    const baseline = localDraftFromConfig(localConfig());
    expect(isLocalDraftDirty(baseline, baseline)).toBe(false);
    expect(isLocalDraftDirty({ ...baseline, mode: 'use-http-path' }, baseline)).toBe(true);
    expect(isLocalDraftDirty({ ...baseline, localUserName: 'Alice' }, baseline)).toBe(true);
  });

  it('serializes local draft into RepoLocalCredentialInput', () => {
    const draft = {
      ...localDraftFromConfig(localConfig()),
      mode: 'isolated-store' as const,
      useHttpPath: true,
      username: ' special_user ',
      storeFile: ' /home/dev/.git-credentials-work ',
      storePassword: 'secret',
      remoteUrlUsernameAction: 'embed' as const,
      localUserName: ' Special ',
      localUserEmail: ' special@example.com '
    };
    const payload = localDraftToInput(draft, remote());
    expect(payload).toEqual({
      mode: 'isolated-store',
      useHttpPath: true,
      username: 'special_user',
      storeFile: '/home/dev/.git-credentials-work',
      storePassword: 'secret',
      targetRemoteName: 'origin',
      targetRemoteUrl: 'https://github.com/org/repo.git',
      remoteUrlUsernameAction: 'embed',
      localUserName: 'Special',
      localUserEmail: 'special@example.com'
    });
  });

  it('previews embedding and stripping a username on HTTP(S) URLs', () => {
    expect(
      previewRemoteUrlWithUsername('https://github.com/org/repo.git', 'embed', 'alice@corp')
    ).toBe('https://alice%40corp@github.com/org/repo.git');
    expect(
      previewRemoteUrlWithUsername('https://alice@github.com/org/repo.git', 'strip', '')
    ).toBe('https://github.com/org/repo.git');
    expect(
      previewRemoteUrlWithUsername('git@github.com:org/repo.git', 'embed', 'alice')
    ).toBeNull();
  });
});
