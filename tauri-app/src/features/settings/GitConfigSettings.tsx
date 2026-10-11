/**
 * The Git section of the settings window: the global Git configuration.
 *
 * Every field maps to one allow-listed key the backend knows. Empty fields
 * mean "unset", which returns the key to Git's built-in behavior. Writes are
 * explicit — a Save button — because each one shells out to `git config
 * --global`, unlike the application preferences which persist transparently.
 */

import { useEffect, useState } from 'react';

import * as ipc from '../../bridge/ipc';
import type { GitConfigEntry, GitConfigKey, GitConfigSnapshot } from '../../bridge/types';
import { Select, TextInput } from '../../components/controls';
import { renderGitError, useStore } from '../../app/store';
import { t } from '../../i18n/strings';

/** Editable fields in display order, keyed by the Git key each one writes. */
const GIT_KEYS: GitConfigKey[] = [
  'user.name',
  'user.email',
  'init.defaultBranch',
  'core.autocrlf',
  'core.editor'
];

type GitFields = Record<GitConfigKey, string>;

function fieldsFromSnapshot(snapshot: GitConfigSnapshot): GitFields {
  return {
    'user.name': snapshot.userName ?? '',
    'user.email': snapshot.userEmail ?? '',
    'init.defaultBranch': snapshot.initDefaultBranch ?? '',
    'core.autocrlf': snapshot.coreAutocrlf ?? '',
    'core.editor': snapshot.coreEditor ?? ''
  };
}

/**
 * Client-side validation, one message key per failing field.
 *
 * The backend re-validates; this only catches what the interface can see
 * before a round trip, so a broken value disables Save instead of failing
 * after it.
 */
export function validateGitFields(fields: GitFields): Partial<Record<GitConfigKey, string>> {
  const errors: Partial<Record<GitConfigKey, string>> = {};
  const email = fields['user.email'].trim();
  if (email && (!email.includes('@') || /\s/.test(email))) {
    errors['user.email'] = 'git-identity-email-required';
  }
  const branch = fields['init.defaultBranch'].trim();
  if (
    branch &&
    (/\s/.test(branch) ||
      branch.startsWith('-') ||
      branch.includes('..') ||
      branch.endsWith('.lock') ||
      branch.startsWith('/') ||
      branch.endsWith('/'))
  ) {
    errors['init.defaultBranch'] = 'git-default-branch-invalid';
  }
  return errors;
}

const AUTOCRLF_OPTIONS: { value: string; key: string }[] = [
  { value: '', key: 'git-autocrlf-unset' },
  { value: 'true', key: 'git-autocrlf-true' },
  { value: 'input', key: 'git-autocrlf-input' },
  { value: 'false', key: 'git-autocrlf-false' }
];

export function GitConfigSettings() {
  const translate = useStore((state) => state.t);
  const [snapshot, setSnapshot] = useState<GitFields | null>(null);
  const [draft, setDraft] = useState<GitFields | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    let cancelled = false;
    void ipc
      .readGitConfig()
      .then((loaded) => {
        if (cancelled) {
          return;
        }
        const fields = fieldsFromSnapshot(loaded);
        setSnapshot(fields);
        setDraft(fields);
      })
      .catch((failure) => {
        if (!cancelled) {
          setError(ipc.describeError(failure).detail);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!snapshot || !draft) {
    return null;
  }

  const dirty = GIT_KEYS.some((key) => draft[key] !== snapshot[key]);
  const errors = validateGitFields(draft);
  const canSave = dirty && !saving && Object.keys(errors).length === 0;

  const setField = (key: GitConfigKey, value: string) => {
    setSaved(false);
    setDraft((current) => (current ? { ...current, [key]: value } : current));
  };

  const save = async () => {
    if (!canSave) {
      return;
    }
    setSaving(true);
    setError('');
    try {
      const entries: GitConfigEntry[] = GIT_KEYS.filter((key) => draft[key] !== snapshot[key]).map(
        (key) => ({
          key,
          value: draft[key].trim() ? draft[key].trim() : null
        })
      );
      const next = await ipc.writeGitConfig(entries);
      const fields = fieldsFromSnapshot(next);
      setSnapshot(fields);
      setDraft(fields);
      setSaved(true);
    } catch (failure) {
      const described = ipc.describeError(failure);
      setError(renderGitError(translate, described.key, described.detail));
      setSaved(false);
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <p className="settings__description" data-testid="settings-git-description">
        {t(translate, 'git-config-description')}
      </p>
      <div
        className="settings__field settings__search-target"
        data-testid="settings-field-git-user-name"
      >
        <span className="settings__label">{t(translate, 'git-user-name-title')}</span>
        <TextInput
          value={draft['user.name']}
          placeholder={t(translate, 'git-config-not-set')}
          testId="git-user-name"
          onChange={(value) => setField('user.name', value)}
        />
        <div className="settings__hint">{t(translate, 'git-user-name-hint')}</div>
      </div>
      <div
        className="settings__field settings__search-target"
        data-testid="settings-field-git-user-email"
      >
        <span className="settings__label">{t(translate, 'git-user-email-title')}</span>
        <TextInput
          value={draft['user.email']}
          placeholder={t(translate, 'git-config-not-set')}
          testId="git-user-email"
          onChange={(value) => setField('user.email', value)}
        />
        <div className="settings__hint">{t(translate, 'git-user-email-hint')}</div>
        {errors['user.email'] ? (
          <div className="status-conflict" data-testid="git-user-email-error">
            {t(translate, errors['user.email'])}
          </div>
        ) : null}
      </div>
      <div
        className="settings__field settings__search-target"
        data-testid="settings-field-git-default-branch"
      >
        <span className="settings__label">{t(translate, 'git-default-branch-title')}</span>
        <TextInput
          value={draft['init.defaultBranch']}
          placeholder={t(translate, 'git-config-not-set')}
          testId="git-default-branch"
          onChange={(value) => setField('init.defaultBranch', value)}
        />
      </div>
      <div
        className="settings__field settings__search-target"
        data-testid="settings-field-git-autocrlf"
      >
        <span className="settings__label">{t(translate, 'git-autocrlf-title')}</span>
        <Select
          value={draft['core.autocrlf']}
          testId="git-autocrlf"
          options={AUTOCRLF_OPTIONS.map((entry) => ({
            value: entry.value,
            label: t(translate, entry.key)
          }))}
          onChange={(value) => setField('core.autocrlf', value)}
        />
      </div>
      <div
        className="settings__field settings__search-target"
        data-testid="settings-field-git-editor"
      >
        <span className="settings__label">{t(translate, 'git-editor-title')}</span>
        <TextInput
          value={draft['core.editor']}
          placeholder={t(translate, 'git-config-not-set')}
          testId="git-editor"
          onChange={(value) => setField('core.editor', value)}
        />
        <div className="settings__hint">{t(translate, 'git-editor-hint')}</div>
      </div>
      <div className="settings__field" data-testid="settings-field-git-save">
        <div className="settings__row">
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!canSave}
            data-testid="git-config-save"
            onClick={() => void save()}
          >
            {t(translate, 'git-config-save')}
          </button>
          {saved ? (
            <span className="settings__hint" role="status" data-testid="git-config-saved">
              {t(translate, 'git-config-saved')}
            </span>
          ) : null}
          {dirty && !saved && Object.keys(errors).length === 0 ? (
            <span className="settings__hint" data-testid="git-config-unsaved">
              {t(translate, 'git-config-unsaved')}
            </span>
          ) : null}
        </div>
        {Object.entries(errors)
          .filter(([key]) => key !== 'user.email')
          .map(([key, messageKey]) =>
            messageKey ? (
              <div key={key} className="status-conflict" role="alert">
                {t(translate, messageKey)}
              </div>
            ) : null
          )}
        {error ? (
          <div className="status-conflict" role="alert" data-testid="git-config-error">
            {error}
          </div>
        ) : null}
      </div>
    </>
  );
}
