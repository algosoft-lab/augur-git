import { useEffect, useRef, useState } from 'react';

import { Icon } from '../../components/Icon';
import { DialogCard, Spinner, TextInput } from '../../components/controls';
import { firstLine } from '../../app/repoState';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';
import { validateBranchName } from './branchName';
import { canSaveRemoteUrl, remoteUrlError } from './remoteForm';
import { remoteConversion } from './remoteProtocol';

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
  rename: 'remote rename',
  remove: 'remote remove'
} as const;

type Operation = keyof typeof OPERATION_LABELS;

interface PendingOperation {
  operation: Operation;
  name: string;
  url?: string;
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
  const activeRepoId = useActiveRepoId();
  const [repoId] = useState(activeRepoId);
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const setMessage = useStore((state) => state.setMessage);

  const [panel, setPanel] = useState<'add' | 'edit'>(focus ? 'edit' : 'add');
  const [selected, setSelected] = useState<string | null>(focus?.remote ?? null);
  const [name, setName] = useState(focus?.remote ?? '');
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
  const [baseline, setBaseline] = useState({ name: focus?.remote ?? '', url });
  const [leave, setLeave] = useState<{ proceed: () => void } | null>(null);

  const nameInput = useRef<HTMLInputElement>(null);
  const urlInput = useRef<HTMLInputElement>(null);

  const busy = Boolean(repo?.busy || pending);

  // The list is derived from the refs on every render rather than remembered.
  // There are only a handful of remotes, and a snapshot that arrives with the
  // same array identity would otherwise leave the list showing a remote that
  // is no longer there.
  const remotes = (repo?.refs.remotes ?? []).map((remoteName) => ({
    name: remoteName,
    url: repo?.refs.remote_urls.find((remote) => remote.name === remoteName)?.url ?? '',
    hasPushUrl:
      repo?.refs.remote_urls.find((remote) => remote.name === remoteName)?.has_push_url ?? false
  }));
  const remoteNames = remotes.map((remote) => remote.name);
  const selectedRemote = remotes.find((remote) => remote.name === selected) ?? null;

  const trimmedName = name.trim();
  const nameError = validateBranchName(trimmedName, remoteNames, selected ?? undefined);
  const urlError = remoteUrlError(url);
  const canAdd = Boolean(
    repoId && !busy && trimmedName.length > 0 && url.trim().length > 0 && nameError === null
  );
  const canSave = Boolean(repoId && !busy && selectedRemote && canSaveRemoteUrl(url, baseline.url));

  const canRename = Boolean(
    repoId && !busy && selectedRemote && !nameError && trimmedName !== baseline.name
  );
  const dirty = name.trim() !== baseline.name || url.trim() !== baseline.url.trim();
  const conversion = remoteConversion(url);

  const navigate = (proceed: () => void) => {
    if (busy) return;
    if (dirty) setLeave({ proceed });
    else proceed();
  };
  const requestClose = () => navigate(closeOverlay);

  // Capture Escape before application shortcuts can close an unsaved form.
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (leave) setLeave(null);
      else requestClose();
    };
    window.addEventListener('keydown', escape, true);
    return () => window.removeEventListener('keydown', escape, true);
  }, [busy, dirty, leave]);

  // The remote the current branch tracks is where a plain push goes, so the
  // list says which one it is.
  const upstream = repo?.upstream ?? null;
  const upstreamRemote = upstream
    ? (remoteNames
        .filter((remote) => upstream.startsWith(`${remote}/`))
        .sort((a, b) => b.length - a.length)[0] ?? null)
    : null;
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
    if (operation.operation === 'rename') {
      setSelected(operation.name);
      setName(operation.name);
      setBaseline((current) => ({ ...current, name: operation.name }));
    }
    if (operation.operation === 'setUrl') {
      setUrl(operation.url!);
      setBaseline((current) => ({ ...current, url: operation.url! }));
    }
    if (operation.operation === 'remove' || operation.operation === 'add') {
      setBaseline({ name: '', url: '' });
    }
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
          : operation.operation === 'rename'
            ? ta(translate, 'remotes-renamed', { name: operation.name })
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
    setBaseline({ name: '', url: '' });
    setConfirming(false);
    setNotice(null);
    nameInput.current?.focus();
  };

  const startEdit = (remote: { name: string; url: string }) => {
    setPanel('edit');
    setSelected(remote.name);
    setName(remote.name);
    setBaseline({ name: remote.name, url: remote.url });
    setUrl(remote.url);
    setConfirming(false);
    setNotice(null);
    urlInput.current?.focus();
  };

  const startRemove = (remote: { name: string; url: string }) => {
    setPanel('edit');
    setSelected(remote.name);
    setName(remote.name);
    setBaseline({ name: remote.name, url: remote.url });
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
      { operation: 'setUrl', name: selected, url: url.trim(), baseline: 0 },
      { action: 'remoteSetUrl', name: selected, url: url.trim(), oldUrl: baseline.url }
    );
  };

  const submitRename = () => {
    if (!canRename || !selected) return;
    send(
      { operation: 'rename', name: trimmedName, baseline: 0 },
      { action: 'remoteRename', oldName: selected, newName: trimmedName }
    );
  };

  const submitRemove = () => {
    if (!selected || busy) {
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
      onBackdrop={requestClose}
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
                      disabled={busy || Boolean(leave)}
                      onClick={() => {
                        if (selected !== remote.name || confirming)
                          navigate(() => startEdit(remote));
                      }}
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
                      disabled={busy || Boolean(leave)}
                      onClick={() => navigate(() => startRemove(remote))}
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
                disabled={busy || Boolean(leave)}
                onClick={() => navigate(startAdd)}
              >
                <Icon name="plus" size={13} /> {t(translate, 'remotes-add-new')}
              </button>
            ) : null}
          </section>

          <section
            className="remote-manager__panel"
            aria-label={t(translate, 'remotes-form-label')}
          >
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
            {leave ? (
              <div
                className="remote-manager__confirm"
                role="alert"
                data-testid="remote-discard-panel"
              >
                <span className="remote-manager__confirm-title">
                  {t(translate, 'remotes-unsaved-title')}
                </span>
                <span className="remote-manager__confirm-text">
                  {t(translate, 'remotes-unsaved-detail')}
                </span>
              </div>
            ) : confirming && selected ? (
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
                <label className="settings__label" htmlFor="remote-name-input">
                  {t(translate, 'remotes-name-label')}
                </label>
                <TextInput
                  id="remote-name-input"
                  inputRef={nameInput}
                  value={name}
                  onChange={setName}
                  placeholder={t(translate, 'remotes-name-placeholder')}
                  monospace
                  disabled={busy}
                  testId="remote-name-input"
                  ariaDescribedBy={
                    nameError && nameError !== 'empty' ? 'remote-name-error' : undefined
                  }
                  onSubmit={panel === 'edit' ? submitRename : submitAdd}
                  onEscape={requestClose}
                />
                {nameError && nameError !== 'empty' ? (
                  <div
                    className="remote-manager__field-error"
                    id="remote-name-error"
                    role="alert"
                    data-testid="remote-name-error"
                  >
                    {nameErrorText}
                  </div>
                ) : null}
                {panel === 'edit' ? (
                  <button
                    type="button"
                    className="tool-button"
                    data-testid="remote-rename"
                    disabled={!canRename}
                    onClick={submitRename}
                  >
                    {pending?.operation === 'rename' ? <Spinner size={11} /> : null}
                    {t(translate, 'remotes-rename')}
                  </button>
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
                  onEscape={requestClose}
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
                {selectedRemote?.hasPushUrl ? (
                  <div className="settings__hint" data-testid="remote-push-url-hint">
                    {t(translate, 'remotes-push-url-hint')}
                  </div>
                ) : null}
                {url.trim() ? (
                  <>
                    {conversion.target ? (
                      <button
                        type="button"
                        className="tool-button"
                        data-testid="remote-convert"
                        disabled={busy || Boolean(conversion.reason)}
                        onClick={() => {
                          if (conversion.url) {
                            setUrl(conversion.url);
                            setNotice(null);
                          }
                        }}
                      >
                        {t(
                          translate,
                          conversion.target === 'ssh'
                            ? 'remotes-convert-ssh'
                            : 'remotes-convert-https'
                        )}
                      </button>
                    ) : null}
                    <div className="settings__hint" data-testid="remote-conversion-hint">
                      {t(
                        translate,
                        conversion.reason
                          ? `remotes-conversion-${conversion.reason}`
                          : 'remotes-conversion-preview'
                      )}
                    </div>
                  </>
                ) : null}
                {panel === 'edit' ? (
                  <button
                    type="button"
                    className="tool-button remote-manager__remove-action"
                    data-testid="remote-remove-action"
                    disabled={busy}
                    onClick={() => navigate(() => setConfirming(true))}
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
        leave ? (
          <>
            <button
              type="button"
              className="tool-button"
              data-testid="remote-keep-editing"
              onClick={() => setLeave(null)}
              autoFocus
            >
              {t(translate, 'remotes-keep-editing')}
            </button>
            <button
              type="button"
              className="tool-button tool-button--danger"
              data-testid="remote-discard"
              onClick={() => {
                const proceed = leave.proceed;
                setLeave(null);
                setName(baseline.name);
                setUrl(baseline.url);
                proceed();
              }}
            >
              {t(translate, 'remotes-discard')}
            </button>
          </>
        ) : confirming ? (
          <>
            <button
              type="button"
              className="tool-button"
              onClick={() => setConfirming(false)}
              disabled={busy}
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
              onClick={requestClose}
              disabled={busy}
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
