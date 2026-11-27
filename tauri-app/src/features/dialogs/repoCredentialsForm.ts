/**
 * Pure form state, validation, and payload conversion for the Repository
 * Credentials modal.
 *
 * Kept pure so unit tests can verify Strategy 1 (`use-http-path`), Strategy 2
 * (`isolated-store` / `isolated-prompt`), remote URL username rewriting, and
 * repository-scoped vault draft defaults without mounting a window.
 */

import type {
  CredentialStorage,
  RemoteUsernameAction,
  RepoCredentialMode,
  RepoLocalCredentialConfig,
  RepoLocalCredentialInput,
  RepoRemoteCredentialInfo
} from '../../bridge/types';
import type { CredentialDraft } from '../settings/credentialsForm';

export interface RepoLocalDraft {
  mode: RepoCredentialMode;
  useHttpPath: boolean;
  username: string;
  storeFile: string;
  storePassword: string;
  remoteUrlUsernameAction: RemoteUsernameAction;
  localUserName: string;
  localUserEmail: string;
}

export type LocalDraftIssue =
  | 'storeFile'
  | 'username'
  | 'storePassword'
  | 'localUserName'
  | 'localUserEmail'
  | 'remoteUrlEmbed';

/** Build a repository-scoped vault draft pre-filled from the selected remote. */
export function vaultDraftFromRemote(
  remote: RepoRemoteCredentialInfo | null,
  defaultStorage: CredentialStorage,
  repoPath: string
): CredentialDraft {
  return {
    id: null,
    protocol: remote?.protocol === 'http' ? 'http' : 'https',
    host: remote?.host ?? '',
    pathPrefix: remote?.path ?? '',
    username: remote?.urlUsername ?? '',
    password: '',
    storage: defaultStorage,
    repositoryOnly: true,
    repository: repoPath
  };
}

/** Build an editable local `.git/config` draft from the backend snapshot. */
export function localDraftFromConfig(
  config: RepoLocalCredentialConfig
): RepoLocalDraft {
  return {
    mode: config.mode,
    useHttpPath: config.mode === 'use-http-path' ? true : (config.useHttpPath ?? false),
    username: config.username ?? '',
    storeFile: config.storeFile ?? config.suggestedStoreFile,
    storePassword: '',
    remoteUrlUsernameAction: 'keep',
    localUserName: config.localUserName ?? '',
    localUserEmail: config.localUserEmail ?? ''
  };
}

function hasControlChar(value: string): boolean {
  return value.includes('\n') || value.includes('\r') || value.includes('\0');
}

export function validateLocalDraft(draft: RepoLocalDraft): LocalDraftIssue[] {
  const issues: LocalDraftIssue[] = [];
  if (
    hasControlChar(draft.username) ||
    hasControlChar(draft.storeFile) ||
    hasControlChar(draft.storePassword) ||
    hasControlChar(draft.localUserName)
  ) {
    issues.push('storePassword');
  }
  if (draft.mode === 'isolated-store' && !draft.storeFile.trim()) {
    issues.push('storeFile');
  }
  if (draft.mode === 'isolated-store' && draft.storePassword && !draft.username.trim()) {
    issues.push('username');
  }
  if (draft.remoteUrlUsernameAction === 'embed' && !draft.username.trim()) {
    issues.push('remoteUrlEmbed');
  }
  const email = draft.localUserEmail.trim();
  if (email && (hasControlChar(email) || !email.includes('@') || /\s/.test(email))) {
    issues.push('localUserEmail');
  }
  return issues;
}

export function isLocalDraftComplete(draft: RepoLocalDraft): boolean {
  return validateLocalDraft(draft).length === 0;
}

export function isLocalDraftDirty(draft: RepoLocalDraft, baseline: RepoLocalDraft): boolean {
  return (
    draft.mode !== baseline.mode ||
    draft.useHttpPath !== baseline.useHttpPath ||
    draft.username.trim() !== baseline.username.trim() ||
    (draft.mode === 'isolated-store' && draft.storeFile.trim() !== baseline.storeFile.trim()) ||
    draft.storePassword.length > 0 ||
    draft.remoteUrlUsernameAction !== baseline.remoteUrlUsernameAction ||
    draft.localUserName.trim() !== baseline.localUserName.trim() ||
    draft.localUserEmail.trim() !== baseline.localUserEmail.trim()
  );
}

export function localDraftToInput(
  draft: RepoLocalDraft,
  selectedRemote: RepoRemoteCredentialInfo | null
): RepoLocalCredentialInput {
  const clean = (value: string) => {
    const trimmed = value.trim();
    return trimmed ? trimmed : null;
  };
  return {
    mode: draft.mode,
    useHttpPath:
      draft.mode === 'use-http-path'
        ? true
        : draft.mode === 'inherit'
          ? false
          : draft.useHttpPath,
    username: draft.mode === 'inherit' ? null : clean(draft.username),
    storeFile: draft.mode === 'isolated-store' ? clean(draft.storeFile) : null,
    storePassword:
      draft.mode === 'isolated-store' && draft.storePassword ? draft.storePassword : null,
    targetRemoteName: selectedRemote?.name ?? null,
    targetRemoteUrl: selectedRemote?.isHttp ? selectedRemote.url : null,
    remoteUrlUsernameAction:
      selectedRemote?.isHttp ? draft.remoteUrlUsernameAction : 'keep',
    localUserName: clean(draft.localUserName),
    localUserEmail: clean(draft.localUserEmail)
  };
}

/** Preview the resulting HTTP(S) remote URL when embedding or stripping a username. */
export function previewRemoteUrlWithUsername(
  url: string,
  action: RemoteUsernameAction,
  username: string
): string | null {
  if (action === 'keep') return null;
  const trimmed = url.trim();
  const schemeSep = trimmed.indexOf('://');
  if (schemeSep <= 0) return null;
  const protocol = trimmed.slice(0, schemeSep);
  if (protocol.toLowerCase() !== 'http' && protocol.toLowerCase() !== 'https') {
    return null;
  }
  const rest = trimmed.slice(schemeSep + 3);
  const pathSep = rest.search(/[/?#]/);
  const authority = pathSep === -1 ? rest : rest.slice(0, pathSep);
  const suffix = pathSep === -1 ? '' : rest.slice(pathSep);
  const atIdx = authority.lastIndexOf('@');
  const host = atIdx === -1 ? authority : authority.slice(atIdx + 1);
  if (!host) return null;

  if (action === 'strip') {
    return `${protocol}://${host}${suffix}`;
  }
  const user = username.trim();
  if (!user) return null;
  const encoded = encodeURIComponent(user);
  return `${protocol}://${encoded}@${host}${suffix}`;
}
