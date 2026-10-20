import { useEffect, useRef, useState } from 'react';

import { Icon } from '../../components/Icon';
import { DialogCard, Spinner, TextInput } from '../../components/controls';
import { firstLine } from '../../app/repoState';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';
import { validateBranchName } from './branchName';
import { canSaveRemoteUrl, remoteUrlError } from './remoteForm';

/**
 * The label the backend reports for each remote operation.
 *
 * A result is matched against the label of the request that started it, so a
 * background command that finishes in the meantime is not mistaken for the
 * answer to this dialog.
 */
const OPERATION_LABELS = {
  add: 'remote add',
  setUrl: 'remote set-url',
  remove: 'remote remove'
} as const;

type Operation = keyof typeof OPERATION_LABELS;

interface PendingOperation {
  operation: Operation;
  name: string;
  /** The result id that was current when the request was sent. */
  baseline: number;
}

interface Notice {
  tone: 'success' | 'error';
  text: string;
}

export function ManageRemotesDialog({
  focus
}: {
  /** The remote the dialog opens on, when it was reached from one. */
  focus?: { remote: string; panel: 'edit' | 'remove' };
}) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const setMessage = useStore((state) => state.setMessage);

  const [panel, setPanel] = useState<'add' | 'edit'>(focus?.panel === 'edit' ? 'edit' : 'add');
  const [selected, setSelected] = useState<string | null>(focus?.remote ?? null);
  const [name, setName] = useState('');
  const [url, setUrl] = useState(() => {
    if (!focus || !repoId) {
      return '';
    }
    const current = useStore.getState().repos[repoId];
    return current?.refs.remote_urls.find((remote) => remote.name === focus.remote)?.url ?? '';
  });
  const [confirming, setConfirming] = useState(focus?.panel === 'remove');
  const [notice, setNotice] = useState<Notice | null>(null);
  const [pending, setPending] = useState<PendingOperation | null>(null);

  const nameInput = useRef<HTMLInputElement>(null);
  const urlInput = useRef<HTMLInputElement>(null);

  const busy = Boolean(repo?.busy);

  // The list is derived from the refs on every render rather than remembered.
  // There are only a handful of remotes, and a snapshot that arrives with the
  // same array identity would otherwise leave the list showing a remote that
  // is no longer there.
  const remotes = (repo?.refs.remotes ?? []).map((remoteName) => ({
    name: remoteName,
    url: repo?.refs.remote_urls.find((remote) => remote.name === remoteName)?.url ?? ''
  }));
  const remoteNames = remotes.map((remote) => remote.name);
  const selectedRemote = remotes.find((remote) => remote.name === selected) ?? null;

  const trimmedName = name.trim();
  const nameError = panel === 'add' ? validateBranchName(trimmedName, remoteNames) : null;
  const urlError = remoteUrlError(url);
  const canAdd = Boolean(
    repoId && !busy && trimmedName.length > 0 && url.trim().length > 0 && nameError === null
  );
  const canSave = Boolean(
    repoId && !busy && selectedRemote && canSaveRemoteUrl(url, selectedRemote.url)
  );

  // The remote the current branch tracks is where a plain push goes, so the
  // list says which one it is.
  const upstream = repo?.upstream ?? null;
  const upstreamRemote = upstream ? upstream.split('/')[0]! : null;
  const upstreamBranch =
    upstream && upstreamRemote ? upstream.slice(upstreamRemote.length + 1) : '';

  // A result belongs to the request that started it, so the dialog waits for
  // its own outcome instead of reading a message some other command wrote.
  useEffect(() => {
    if (!pending) {
      return;
    }
    const outcome = repo?.commandOutcome;
    if (!outcome || outcome.id <= pending.baseline) {
      return;
    }
    if (outcome.label && outcome.label !== OPERATION_LABELS[pending.operation]) {
      return;
    }
    setPending(null);
    if (outcome.success) {
      completeOperation(pending);
    } else {
      setNotice({
        tone: 'error',
        text: firstLine(outcome.message) || t(translate, 'remotes-failed')
      });
    }
  }, [pending, repo?.commandOutcome, translate]);

  // The panel that is on screen decides where the caret belongs.
  useEffect(() => {
    if (panel === 'add') {
      nameInput.current?.focus();
    } else {
      urlInput.current?.focus();
    }
  }, [panel]);

  function completeOperation(operation: PendingOperation) {
    if (operation.operation === 'remove') {
      setPanel('add');
      setSelected(null);
      setName('');
      setUrl('');
      setConfirming(false);
    }
    if (operation.operation === 'add') {
      // The name goes back to the field, so the next remote can be typed
      // straight away and the name that was just added is not read as a
      // duplicate of itself.
      setName('');
      setUrl('');
    }
    setNotice({
      tone: 'success',
      text:
        operation.operation === 'add'
          ? ta(translate, 'remotes-added', { name: operation.name })
          : operation.operation === 'setUrl'
            ? ta(translate, 'remotes-url-saved', { name: operation.name })
            : ta(translate, 'remotes-removed', { name: operation.name })
    });
    if (operation.operation !== 'setUrl') {
      nameInput.current?.focus();
    }
  }

  const startAdd = () => {
    setPanel('add');
    setSelected(null);
    setName('');
    setUrl('');
    setConfirming(false);
    setNotice(null);
    nameInput.current?.focus();
  };

  const startEdit = (remote: { name: string; url: string }) => {
    setPanel('edit');
    setSelected(remote.name);
    setUrl(remote.url);
    setConfirming(false);
    setNotice(null);
    urlInput.current?.focus();
  };

  const startRemove = (remote: { name: string; url: string }) => {
    setPanel('edit');
    setSelected(remote.name);
    setUrl(remote.url);
    setConfirming(true);
    setNotice(null);
  };

  const send = (operation: PendingOperation, action: Parameters<typeof runAction>[1]) => {
    if (!repoId) {
      return;
    }
    setNotice(null);
    setMessage(repoId, '', null);
    setPending({ ...operation, baseline: repo?.commandOutcome?.id ?? 0 });
    void runAction(repoId, action);
  };

  const submitAdd = () => {
    if (!canAdd || pending) {
      return;
    }
    send(
      { operation: 'add', name: trimmedName, baseline: 0 },
      { action: 'remoteAdd', name: trimmedName, url: url.trim() }
    );
  };

  const submitUrl = () => {
    if (!canSave || !selected || pending) {
      return;
    }
    send(
      { operation: 'setUrl', name: selected, baseline: 0 },
      { action: 'remoteSetUrl', name: selected, url: url.trim() }
    );
  };

  const submitRemove = () => {
    if (!selected || pending) {
      return;
    }
    send(
      { operation: 'remove', name: selected, baseline: 0 },
      { action: 'remoteRemove', name: selected }
    );
  };

  const nameErrorText =
    nameError === 'exists'
      ? ta(translate, 'remotes-name-exists', { name: trimmedName })
      : t(translate, 'remotes-name-invalid');

  return (
    <DialogCard
      testId="manage-remotes-dialog"
      width={720}
      title={t(translate, 'remotes-title')}
      subtitle={repo?.path}
      icon={<Icon name="globe" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <div className="remote-manager">
          <section className="remote-manager__list" aria-label={t(translate, 'remotes-list-label')}>
            <div className="remote-manager__list-head">
              <span>{t(translate, 'remotes-list-label')}</span>
              <span className="remote-manager__count" data-testid="remotes-count">
                {remotes.length}
              </span>
            </div>
            {remotes.length === 0 ? (
              <div className="remote-manager__empty" data-testid="remotes-empty">
                <Icon name="globe" size={20} />
                <span className="remote-manager__empty-title">
                  {t(translate, 'remotes-empty-title')}
                </span>
                <span className="remote-manager__empty-hint">
                  {t(translate, 'remotes-empty-hint')}
                </span>
              </div>
            ) : (
              <ul className="remote-manager__items" data-testid="remotes-list">
                {remotes.map((remote) => (
                  <li
                    className={`remote-manager__row${
                      panel === 'edit' && selected === remote.name ? ' is-selected' : ''
                    }`}
                    key={remote.name}
                  >
                    <button
                      type="button"
                      className="remote-manager__item"
                      aria-current={
                        panel === 'edit' && selected === remote.name ? 'true' : undefined
                      }
                      title={ta(translate, 'remotes-edit-label', { name: remote.name })}
                      data-testid={`remote-edit-${remote.name}`}
                      onClick={() => startEdit(remote)}
                    >
                      <span className="remote-manager__item-name">
                        {remote.name}
                        {upstreamRemote === remote.name ? (
                          <span
                            className="remote-manager__badge"
                            title={ta(translate, 'remotes-tracks', { branch: upstreamBranch })}
                          >
                            {upstreamBranch}
                          </span>
                        ) : null}
                      </span>
                      <span className="mono remote-manager__item-url" title={remote.url}>
                        {remote.url}
                      </span>
                    </button>
                    <button
                      type="button"
                      className="icon-button remote-manager__remove"
                      aria-label={ta(translate, 'remotes-remove-label', { name: remote.name })}
                      title={t(translate, 'remotes-remove')}
                      data-testid={`remote-remove-${remote.name}`}
                      disabled={busy}
                      onClick={() => startRemove(remote)}
                    >
                      <Icon name="trash-2" size={13} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {panel === 'edit' ? (
              <button
                type="button"
                className="tool-button remote-manager__add"
                data-testid="remote-add-another"
                disabled={busy}
                onClick={startAdd}
              >
                <Icon name="plus" size={13} /> {t(translate, 'remotes-add-new')}
              </button>
            ) : null}
          </section>

          <section
            className="remote-manager__panel"
            aria-label={t(translate, 'remotes-form-label')}
          >
            {confirming && selected ? (
              <div className="remote-manager__confirm" data-testid="remote-remove-confirm-panel">
                <span className="remote-manager__confirm-icon">
                  <Icon name="triangle-alert" size={16} />
                </span>
                <span className="remote-manager__confirm-title">
                  {t(translate, 'remotes-remove-title')}
                </span>
                <span className="remote-manager__confirm-text">
                  {ta(translate, 'remotes-remove-warning', { name: selected })}
                </span>
              </div>
            ) : (
              <>
                <div className="remote-manager__panel-head">
                  <span className="remote-manager__panel-title">
                    {panel === 'edit'
                      ? t(translate, 'remotes-edit-remote-title')
                      : t(translate, 'remotes-add-title')}
                  </span>
                </div>
                {notice ? (
                  <div
                    className={`remote-manager__notice remote-manager__notice--${notice.tone}`}
                    role={notice.tone === 'error' ? 'alert' : 'status'}
                    data-testid="remotes-notice"
                  >
                    <Icon name={notice.tone === 'error' ? 'triangle-alert' : 'check'} size={13} />
                    <span>{notice.text}</span>
                  </div>
                ) : null}
                <label className="settings__label" htmlFor="remote-name-input">
                  {t(translate, 'remotes-name-label')}
                </label>
                <TextInput
                  id="remote-name-input"
                  inputRef={nameInput}
                  value={panel === 'edit' ? (selected ?? '') : name}
                  onChange={setName}
                  placeholder={t(translate, 'remotes-name-placeholder')}
                  monospace
                  readOnly={panel === 'edit'}
                  disabled={busy}
                  testId="remote-name-input"
                  ariaDescribedBy={
                    panel === 'add' && nameError && nameError !== 'empty'
                      ? 'remote-name-error'
                      : undefined
                  }
                  onSubmit={panel === 'edit' ? submitUrl : submitAdd}
                  onEscape={closeOverlay}
                />
                {panel === 'add' && nameError && nameError !== 'empty' ? (
                  <div
                    className="remote-manager__field-error"
                    id="remote-name-error"
                    role="alert"
                    data-testid="remote-name-error"
                  >
                    {nameErrorText}
                  </div>
                ) : null}
                <label className="settings__label" htmlFor="remote-url-input">
                  {t(translate, 'remotes-url-label')}
                </label>
                <TextInput
                  id="remote-url-input"
                  inputRef={urlInput}
                  value={url}
                  onChange={setUrl}
                  placeholder={t(translate, 'remotes-url-placeholder')}
                  monospace
                  disabled={busy}
                  testId="remote-url-input"
                  ariaDescribedBy={urlError ? 'remote-url-error' : undefined}
                  onSubmit={panel === 'edit' ? submitUrl : submitAdd}
                  onEscape={closeOverlay}
                />
                {urlError ? (
                  <div
                    className="remote-manager__field-error"
                    id="remote-url-error"
                    role="alert"
                    data-testid="remote-url-error"
                  >
                    {t(translate, 'remotes-url-required')}
                  </div>
                ) : null}
                <div className="settings__hint">{t(translate, 'remotes-url-hint')}</div>
                {panel === 'edit' ? (
                  <button
                    type="button"
                    className="tool-button remote-manager__remove-action"
                    data-testid="remote-remove-action"
                    disabled={busy}
                    onClick={() => setConfirming(true)}
                  >
                    <Icon name="trash-2" size={12} /> {t(translate, 'remotes-remove')}
                  </button>
                ) : null}
              </>
            )}
          </section>
        </div>
      }
      footer={
        confirming ? (
          <>
            <button
              type="button"
              className="tool-button"
              onClick={() => setConfirming(false)}
              data-testid="remote-remove-cancel"
            >
              {t(translate, 'remotes-keep')}
            </button>
            <button
              type="button"
              className="tool-button tool-button--danger"
              onClick={submitRemove}
              disabled={busy}
              data-testid="remote-remove-confirm"
            >
              {pending?.operation === 'remove' ? (
                <Spinner size={11} />
              ) : (
                <Icon name="trash-2" size={12} />
              )}{' '}
              {t(translate, 'remotes-remove')}
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
            {panel === 'edit' ? (
              <button
                type="button"
                className="tool-button tool-button--primary"
                onClick={submitUrl}
                disabled={!canSave}
                data-testid="remote-submit"
              >
                {pending?.operation === 'setUrl' ? <Spinner size={11} /> : null}{' '}
                {t(translate, 'remotes-save')}
              </button>
            ) : (
              <button
                type="button"
                className="tool-button tool-button--primary"
                onClick={submitAdd}
                disabled={!canAdd}
                data-testid="remote-submit"
              >
                {pending?.operation === 'add' ? <Spinner size={11} /> : null}{' '}
                {t(translate, 'remotes-add')}
              </button>
            )}
          </>
        )
      }
    />
  );
}
