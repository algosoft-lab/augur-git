/**
 * Pure form logic for the credential settings page.
 *
 * The backend validates the same rules again; this module exists so the form
 * can explain a mistake before a round trip and so the rule set is testable
 * without a window.
 */

import type { CredentialEntryView, CredentialInput, CredentialStorage } from '../../bridge/types';

export interface CredentialDraft {
  id: string | null;
  protocol: 'https' | 'http';
  host: string;
  pathPrefix: string;
  username: string;
  password: string;
  storage: CredentialStorage;
  repositoryOnly: boolean;
  /** Path of the repository the draft is scoped to, when it is scoped. */
  repository: string | null;
}

export type DraftIssue = 'host' | 'username' | 'password';

/** A new draft, scoped to the repository the settings window was opened from. */
export function emptyDraft(storage: CredentialStorage, repository: string | null): CredentialDraft {
  return {
    id: null,
    protocol: 'https',
    host: '',
    pathPrefix: '',
    username: '',
    password: '',
    storage,
    repositoryOnly: false,
    repository
  };
}

/** Turn a stored entry into an editable draft. The secret is never loaded. */
export function draftFromEntry(entry: CredentialEntryView): CredentialDraft {
  return {
    id: entry.id,
    protocol: entry.protocol === 'http' ? 'http' : 'https',
    host: entry.host,
    pathPrefix: entry.pathPrefix ?? '',
    username: entry.username,
    password: '',
    storage: entry.storage,
    repositoryOnly: entry.scope.kind === 'repository',
    repository: entry.scope.kind === 'repository' ? entry.scope.path : null
  };
}

export function validateDraft(draft: CredentialDraft): DraftIssue[] {
  const issues: DraftIssue[] = [];
  const host = draft.host
    .trim()
    .replace(/^[a-z]+:\/\//i, '')
    .split('/')[0];
  if (!host || host.includes(' ') || host.includes('@')) {
    issues.push('host');
  }
  if (!draft.username.trim()) {
    issues.push('username');
  }
  const forbidden = (value: string) => value.includes('\n') || value.includes('\r');
  if (forbidden(draft.username) || forbidden(draft.password)) {
    issues.push('password');
  }
  return issues;
}

/** Whether the draft can be submitted as it stands. */
export function isDraftComplete(draft: CredentialDraft): boolean {
  return validateDraft(draft).length === 0;
}

/** The command payload for a draft. */
export function draftToInput(draft: CredentialDraft): CredentialInput {
  const host =
    draft.host
      .trim()
      .replace(/^[a-z]+:\/\//i, '')
      .split('/')[0] ?? '';
  return {
    id: draft.id,
    protocol: draft.protocol,
    host,
    pathPrefix: draft.pathPrefix.trim().replace(/^\/+|\/+$/g, '') || null,
    username: draft.username.trim(),
    password: draft.password ? draft.password : null,
    scope:
      draft.repositoryOnly && draft.repository
        ? { kind: 'repository', location: { kind: 'local' }, path: draft.repository }
        : { kind: 'global' },
    storage: draft.storage
  };
}

/** Storage choices a storage selector may offer. */
export function storageChoices(
  keychainAvailable: boolean,
  plaintextAvailable: boolean
): CredentialStorage[] {
  const choices: CredentialStorage[] = ['session'];
  if (keychainAvailable) choices.unshift('keychain');
  if (plaintextAvailable) choices.push('plaintext');
  return choices;
}
