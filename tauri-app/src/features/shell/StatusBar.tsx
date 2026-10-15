/**
 * The bottom status bar.
 *
 * Three things share one 24px row: the repository path, the result of the last
 * operation, and a transient state such as "scanning". While a Git command runs
 * its animated verb replaces the result, because the worker executes commands
 * serially and an older message would be stale by the time it was shown.
 */

import { Spinner } from '../../components/controls';
import { useStore } from '../../app/store';
import type { RepoState } from '../../app/repoState';
import { t, ta } from '../../i18n/strings';

/** Last path segment, with a fallback for a filesystem root. */
function basename(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function StatusBar({ repo }: { repo: RepoState | null }) {
  const translate = useStore((state) => state.t);
  const refresh = useStore((store) => store.refresh);
  const showRetry = repo?.status === 'error' || repo?.statusStale === true;

  let state: { text: string; className: string } | null = null;
  if (repo?.status === 'loading') {
    // The reference names the repository it is scanning, not just the fact.
    state = {
      text: ta(translate, 'status-scanning-at', { repo: basename(repo.path) }),
      className: 'status-bar__state status-bar__state--scanning'
    };
  } else if (repo?.status === 'error') {
    state = {
      text: repo.errorMessage ?? t(translate, 'err-unknown'),
      className: 'status-bar__state status-bar__state--error'
    };
  } else if (repo?.statusStale) {
    state = {
      text:
        repo.snapshotId === null
          ? (repo.errorMessage ?? t(translate, 'err-unknown'))
          : t(translate, 'status-stale'),
      className: 'status-bar__state status-bar__state--error'
    };
  }

  const busy = repo?.busyVerb ?? null;
  const message = !busy && repo?.message ? repo.message : null;

  return (
    <div className="status-bar" data-testid="status-bar">
      <div className="status-bar__path" title={repo?.path ?? ''}>
        {repo?.path ?? t(translate, 'status-no-repo-selected')}
      </div>
      <div className="status-bar__right">
        {message ? (
          <div
            className={`status-bar__message--${message.ok === false ? 'fail' : 'ok'}`}
            data-testid="status-message"
          >
            {message.text}
          </div>
        ) : null}
        {busy ? (
          <>
            <Spinner color="var(--warning-background)" rhythm="two-turn-pause" />
            <span data-testid="status-busy">{busy}</span>
          </>
        ) : null}
        {showRetry && repo ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid="status-retry"
            disabled={repo.busy}
            onClick={() => void refresh(repo.id)}
          >
            {t(translate, 'status-retry')}
          </button>
        ) : null}
        {state ? (
          <div
            className={state.className}
            title={repo?.statusStale ? (repo.errorMessage ?? '') : undefined}
          >
            {state.text}
          </div>
        ) : null}
      </div>
    </div>
  );
}
