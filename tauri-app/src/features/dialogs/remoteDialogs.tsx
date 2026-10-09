import { useMemo, useState } from 'react';

import { Icon } from '../../components/Icon';
import { DialogCard, TextInput } from '../../components/controls';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';
import { validateBranchName } from './branchName';

export function ManageRemotesDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const setMessage = useStore((state) => state.setMessage);
  const [editing, setEditing] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [removing, setRemoving] = useState<string | null>(null);
  const [submitted, setSubmitted] = useState(false);

  const remoteUrls = useMemo(
    () =>
      (repo?.refs.remotes ?? []).map((remoteName) => ({
        name: remoteName,
        url: repo?.refs.remote_urls.find((remote) => remote.name === remoteName)?.url ?? ''
      })),
    [repo?.refs.remotes, repo?.refs.remote_urls]
  );
  const nameError = validateBranchName(name, repo?.refs.remotes ?? [], editing ?? undefined);
  const canSave = Boolean(repoId && !repo?.busy && url.trim() && nameError === null);
  const errorMessage = submitted && repo?.message?.ok === false ? repo.message.text : '';

  const startAdd = () => {
    setEditing(null);
    setName('');
    setUrl('');
    setRemoving(null);
    setSubmitted(false);
  };

  const startEdit = (remoteName: string, remoteUrl: string) => {
    setEditing(remoteName);
    setName(remoteName);
    setUrl(remoteUrl);
    setRemoving(null);
    setSubmitted(false);
  };

  const submit = () => {
    if (!repoId || !canSave) return;
    setSubmitted(true);
    setMessage(repoId, '', null);
    void runAction(
      repoId,
      editing === null
        ? { action: 'remoteAdd', name, url: url.trim() }
        : { action: 'remoteSetUrl', name: editing, url: url.trim() }
    );
  };

  const removeRemote = () => {
    if (!repoId || !removing || repo?.busy) return;
    setSubmitted(true);
    setMessage(repoId, '', null);
    void runAction(repoId, { action: 'remoteRemove', name: removing });
    setRemoving(null);
    if (editing === removing) {
      setEditing(null);
      setName('');
      setUrl('');
    }
  };

  const remoteToRemove = remoteUrls.find((remote) => remote.name === removing);

  return (
    <DialogCard
      testId="manage-remotes-dialog"
      width={620}
      title={t(translate, removing ? 'remotes-remove-title' : 'remotes-title')}
      icon={<Icon name="globe" size={16} />}
      onBackdrop={closeOverlay}
      body={
        removing ? (
          <div className="stack">
            <div className="muted">
              {ta(translate, 'remotes-remove-warning', { name: removing })}
            </div>
            {errorMessage ? (
              <div className="status-conflict" data-testid="remotes-error">
                {errorMessage}
              </div>
            ) : null}
          </div>
        ) : (
          <div className="remote-manager stack">
            <section className="stack stack--tight" aria-label={t(translate, 'remotes-list-label')}>
              <div className="settings__label">{t(translate, 'remotes-list-label')}</div>
              {remoteUrls.length === 0 ? (
                <div className="muted remote-manager__empty" data-testid="remotes-empty">
                  {t(translate, 'remotes-empty')}
                </div>
              ) : (
                <div className="remote-manager__list" data-testid="remotes-list">
                  {remoteUrls.map((remote) => (
                    <div className="remote-manager__item" key={remote.name}>
                      <div className="remote-manager__details">
                        <strong>{remote.name}</strong>
                        <span className="mono muted remote-manager__url" title={remote.url}>
                          {remote.url}
                        </span>
                      </div>
                      <button
                        type="button"
                        className="tool-button"
                        aria-label={ta(translate, 'remotes-edit-label', { name: remote.name })}
                        title={t(translate, 'remotes-edit')}
                        data-testid={`remote-edit-${remote.name}`}
                        disabled={repo?.busy}
                        onClick={() => startEdit(remote.name, remote.url)}
                      >
                        <Icon name="pencil" size={13} />
                      </button>
                      <button
                        type="button"
                        className="tool-button tool-button--danger"
                        aria-label={ta(translate, 'remotes-remove-label', { name: remote.name })}
                        title={t(translate, 'remotes-remove')}
                        data-testid={`remote-remove-${remote.name}`}
                        disabled={repo?.busy}
                        onClick={() => {
                          setRemoving(remote.name);
                          setSubmitted(false);
                        }}
                      >
                        <Icon name="trash-2" size={13} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </section>

            <section className="stack" aria-label={t(translate, 'remotes-form-label')}>
              <div className="settings__label">
                {t(translate, editing === null ? 'remotes-add-title' : 'remotes-edit-title')}
              </div>
              <label className="settings__label" htmlFor="remote-name-input">
                {t(translate, 'remotes-name-label')}
              </label>
              <TextInput
                id="remote-name-input"
                value={name}
                onChange={setName}
                autoFocus
                monospace
                disabled={editing !== null || repo?.busy}
                testId="remote-name-input"
                onSubmit={submit}
                onEscape={closeOverlay}
              />
              {nameError && nameError !== 'empty' ? (
                <div className="status-conflict" data-testid="remote-name-error">
                  {nameError === 'exists'
                    ? ta(translate, 'remotes-name-exists', { name })
                    : t(translate, 'remotes-name-invalid')}
                </div>
              ) : null}
              <label className="settings__label" htmlFor="remote-url-input">
                {t(translate, 'remotes-url-label')}
              </label>
              <TextInput
                id="remote-url-input"
                value={url}
                onChange={setUrl}
                placeholder={t(translate, 'remotes-url-placeholder')}
                monospace
                testId="remote-url-input"
                disabled={repo?.busy}
                onSubmit={submit}
                onEscape={closeOverlay}
              />
              {url.length > 0 && !url.trim() ? (
                <div className="status-conflict" data-testid="remote-url-error">
                  {t(translate, 'remotes-url-required')}
                </div>
              ) : null}
              <div className="settings__hint">{t(translate, 'remotes-url-hint')}</div>
              {submitted && repo?.message?.ok === false && !removing ? (
                <div className="status-conflict" data-testid="remotes-error">
                  {repo.message.text}
                </div>
              ) : null}
            </section>
          </div>
        )
      }
      footer={
        removing ? (
          <>
            <button
              type="button"
              className="tool-button"
              onClick={() => {
                setRemoving(null);
                setSubmitted(false);
              }}
              data-testid="remote-remove-cancel"
            >
              {t(translate, 'dialog-cancel')}
            </button>
            <button
              type="button"
              className="tool-button tool-button--danger"
              onClick={removeRemote}
              disabled={!remoteToRemove || repo?.busy}
              data-testid="remote-remove-confirm"
            >
              <Icon name="trash-2" size={12} /> {t(translate, 'remotes-remove')}
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="tool-button"
              onClick={closeOverlay}
              data-testid="manage-remotes-close"
            >
              {t(translate, 'dialog-cancel')}
            </button>
            {editing !== null ? (
              <button
                type="button"
                className="tool-button"
                onClick={startAdd}
                data-testid="remote-edit-cancel"
              >
                {t(translate, 'remotes-add-another')}
              </button>
            ) : null}
            <button
              type="button"
              className="tool-button tool-button--primary"
              onClick={submit}
              disabled={!canSave}
              data-testid="remote-submit"
            >
              {t(translate, editing === null ? 'remotes-add' : 'remotes-save')}
            </button>
          </>
        )
      }
    />
  );
}
