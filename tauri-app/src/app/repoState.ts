/**
 * The state one open repository owns.
 *
 * Every field here is derived from a backend event, so the shape of this object
 * mirrors the event protocol rather than the view that renders it. Keeping the
 * derivation in one pure function means a late event can be tested without a
 * window, and a stale one is rejected before it can overwrite newer state.
 */

import type {
  CommitMessage,
  DiffDocument,
  FileChange,
  FileStatus,
  GraphRow,
  LocationConfig,
  LogRow,
  BranchInfo,
  RefLabel,
  RefsInfo,
  RepoEvent,
  WorkingTreeDiffStats
} from '../bridge/types';

export type RepoStatus = 'loading' | 'ready' | 'error';

/**
 * What the bottom panel is currently showing.
 *
 * A commit with no chosen file shows every changed file at once, which is what
 * the reference application does on selection, so `file` is null rather than
 * the panel starting on a single file.
 */
export type DiffPane =
  | { kind: 'none' }
  | { kind: 'commit'; file: FileChange | null }
  | { kind: 'working'; staged: boolean; file: FileStatus };

export interface CommitSelection {
  oid: string;
  short: string;
  subject: string;
}

export interface StatusMessage {
  text: string;
  ok: boolean | null;
}

export interface RepoState {
  id: number;
  path: string;
  location: LocationConfig;
  status: RepoStatus;
  /** Localized error text when the repository could not be opened. */
  errorMessage: string | null;

  branch: string;
  head: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: FileStatus[];
  diffStats: WorkingTreeDiffStats;
  branches: BranchInfo[];
  /** The full refs snapshot: remotes, remote branches, tags, and stashes. */
  refs: RefsInfo;
  hasConflicts: boolean;
  /** Number of files that can be stashed, used to gate the stash action. */
  stashableCount: number;

  logRows: LogRow[];
  hasMore: boolean;
  /** Set once the log scope has been requested for the current status. */
  logScopeSent: boolean;
  graph: GraphRow[];
  refLabels: Record<string, RefLabel[]>;

  selected: CommitSelection | null;
  /** Identity of the newest commit selection, including repeat selections. */
  commitRequestId: number;
  commitFiles: FileChange[];
  commitFilesLoading: boolean;
  commitFilesError: string | null;
  commitMergeParent: string | null;
  commitMessages: Record<string, CommitMessage>;
  /** Which file the bottom panel is focused on, if any. */
  pane: DiffPane;
  /**
   * Parsed diffs for the selected commit, keyed by file path.
   *
   * Every file of the commit is loaded, because the panel shows them all until
   * one is chosen. Keying by path rather than holding a single document is what
   * lets a late answer for one file land without disturbing the others.
   */
  commitDiffs: Record<string, DiffDocument>;
  /** File paths whose diff requests have not completed for this selection. */
  commitDiffPending: Record<string, true>;
  /** Per-file errors for the current commit selection. */
  commitDiffErrors: Record<string, string>;
  workingDocument: DiffDocument | null;
  workingLoading: boolean;
  workingError: string | null;
  /** Monotonic frontend id used to correlate working-tree diff answers. */
  workingRequest: number;
  /** Request currently awaited for the selected working-tree file. */
  workingInFlight: number | null;
  /** A status refresh arrived while the selected file's diff was in flight. */
  workingRefreshPending: boolean;

  busy: boolean;
  busyVerb: string | null;
  message: StatusMessage | null;
}

export function emptyRepo(id: number, path: string, location: LocationConfig): RepoState {
  return {
    id,
    path,
    location,
    status: 'loading',
    errorMessage: null,

    branch: '',
    head: null,
    upstream: null,
    ahead: 0,
    behind: 0,
    files: [],
    diffStats: { staged: null, unstaged: null, untracked: null },
    branches: [],
    refs: emptyRefs(),
    hasConflicts: false,
    stashableCount: 0,

    logRows: [],
    hasMore: false,
    logScopeSent: false,
    graph: [],
    refLabels: {},

    selected: null,
    commitRequestId: 0,
    commitFiles: [],
    commitFilesLoading: false,
    commitFilesError: null,
    commitMergeParent: null,
    commitMessages: {},
    pane: { kind: 'none' },
    commitDiffs: {},
    commitDiffPending: {},
    commitDiffErrors: {},
    workingDocument: null,
    workingLoading: false,
    workingError: null,
    workingRequest: 0,
    workingInFlight: null,
    workingRefreshPending: false,

    busy: false,
    busyVerb: null,
    message: null
  };
}

export function emptyRefs(): RefsInfo {
  return {
    remotes: [],
    remote_urls: [],
    remote_branches: [],
    tags: [],
    stashes: [],
    comparison_revisions: []
  };
}

/** Files split the way the changes panel groups them. */
export interface FileGroups {
  staged: FileStatus[];
  unstaged: FileStatus[];
  conflicts: FileStatus[];
}

/**
 * Split the working tree into the two groups the panel shows.
 *
 * Git reports an unmerged file on both sides, and a file with a staged change
 * *and* an unstaged one has two separate diffs to view. So the groups are not
 * exclusive: a file can appear in both, which is what makes the unstaged half of
 * a partially staged file reachable. Conflicts live in the changes group, where
 * their status character shows what is wrong with them.
 */
export function groupFiles(files: FileStatus[], showUntracked: boolean): FileGroups {
  const staged: FileStatus[] = [];
  const unstaged: FileStatus[] = [];
  const conflicts: FileStatus[] = [];
  for (const file of files) {
    if (isStaged(file)) {
      staged.push(file);
    }
    if (isConflicted(file)) {
      conflicts.push(file);
    }
    if (isConflicted(file) || (isUntracked(file) ? showUntracked : file.worktree !== ' ')) {
      unstaged.push(file);
    }
  }
  return { staged, unstaged, conflicts };
}

export function isStaged(file: FileStatus): boolean {
  return file.index !== ' ' && file.index !== '?' && !isConflicted(file);
}

export function isUntracked(file: FileStatus): boolean {
  return file.index === '?' && file.worktree === '?';
}

export function isConflicted(file: FileStatus): boolean {
  const pair = `${file.index}${file.worktree}`;
  return file.index === 'U' || file.worktree === 'U' || pair === 'DD' || pair === 'AA';
}

/** The status character that applies to one side of the working tree. */
export function codeFor(file: FileStatus, staged: boolean): string {
  const code = staged
    ? file.index !== ' '
      ? file.index
      : file.worktree
    : file.worktree !== ' '
      ? file.worktree
      : file.index;
  return code === ' ' ? file.index || file.worktree : code;
}

export function hasStagedChanges(file: FileStatus): boolean {
  return isStaged(file);
}

export function hasWorktreeChanges(file: FileStatus): boolean {
  return file.worktree !== ' ';
}

export function canShowUntracked(state: RepoState): boolean {
  return state.files.some((file) => isUntracked(file));
}

/**
 * Whether an integration action must stay blocked.
 *
 * A resolved index can still leave `MERGE_HEAD` behind while Git waits for the
 * merge commit, so a conflict-free status is not on its own enough.
 */
export function integrationBlocked(probe: {
  has_conflicts: boolean;
  merge_head: string | null;
  rebase_in_progress: boolean;
}): boolean {
  return probe.has_conflicts || probe.merge_head !== null || probe.rebase_in_progress;
}

export function localBranches(state: RepoState): string[] {
  return state.branches.filter((branch) => !branch.is_head).map((b) => b.name);
}

export function hasLocalBranches(state: RepoState): boolean {
  return state.branches.some((branch) => !branch.is_head);
}

export function stashCount(state: RepoState): number {
  return state.refs.stashes.length;
}

/**
 * Apply one backend event.
 *
 * The function is pure and returns a new object only when something changed, so
 * React can rely on identity to decide what to re-render. Requests that are no
 * longer the newest are dropped here rather than in the components.
 */
export function applyRepoEvent(
  state: RepoState,
  event: RepoEvent,
  label: (key: string) => string,
  renderError: (key: string, detail: string) => string
): RepoState {
  switch (event.type) {
    case 'status': {
      const hasConflicts = event.files.some((file) => isConflicted(file));
      const stashable = event.files.filter(
        (file) => isStaged(file) || hasWorktreeChanges(file) || isConflicted(file)
      ).length;
      return {
        ...state,
        status: 'ready',
        errorMessage: null,
        branch: event.branch,
        head: event.head,
        upstream: event.upstream,
        ahead: event.ahead,
        behind: event.behind,
        files: event.files,
        diffStats: event.diff_stats ?? { staged: null, unstaged: null, untracked: null },
        branches: event.branches,
        hasConflicts,
        stashableCount: stashable
      };
    }
    case 'logPage': {
      const rows = event.replace ? event.rows : [...state.logRows, ...event.rows];
      return { ...state, logRows: rows, hasMore: event.hasMore };
    }
    case 'refs':
      return { ...state, refs: event.refs };
    case 'commitFiles':
      if (
        state.selected?.oid !== event.oid ||
        state.commitRequestId !== event.requestId ||
        state.pane.kind !== 'commit'
      ) {
        return state;
      }
      return {
        ...state,
        commitFiles: event.files,
        commitFilesLoading: false,
        commitFilesError: null,
        commitMergeParent: event.merge_parent,
        commitDiffPending: Object.fromEntries(
          event.files.map((file) => [file.new_path, true] as const)
        ),
        commitDiffErrors: {}
      };
    case 'commitFilesError':
      if (
        state.selected?.oid !== event.oid ||
        state.commitRequestId !== event.requestId ||
        state.pane.kind !== 'commit'
      ) {
        return state;
      }
      return {
        ...state,
        commitFilesLoading: false,
        commitFilesError: renderError(event.error.key, event.error.detail),
        commitDiffPending: {}
      };
    case 'commitMessage':
      return {
        ...state,
        commitMessages: { ...state.commitMessages, [event.oid]: event.message }
      };
    case 'fileDiff': {
      // An answer for a commit that is no longer selected is stale; an answer
      // for any file of the selected commit is kept, because the panel shows
      // them all until one is focused.
      if (
        state.pane.kind !== 'commit' ||
        state.selected?.oid !== event.oid ||
        state.commitRequestId !== event.requestId
      ) {
        return state;
      }
      const path = event.file.new_path;
      const commitDiffPending = { ...state.commitDiffPending };
      delete commitDiffPending[path];
      const commitDiffErrors = { ...state.commitDiffErrors };
      delete commitDiffErrors[path];
      if (
        state.commitDiffs[path] === event.document &&
        state.commitDiffPending[path] === undefined &&
        state.commitDiffErrors[path] === undefined
      ) {
        return state;
      }
      return {
        ...state,
        commitDiffs: { ...state.commitDiffs, [path]: event.document },
        commitDiffPending,
        commitDiffErrors
      };
    }
    case 'fileDiffError': {
      if (
        state.pane.kind !== 'commit' ||
        state.selected?.oid !== event.oid ||
        state.commitRequestId !== event.requestId
      ) {
        return state;
      }
      const path = event.file.new_path;
      const commitDiffPending = { ...state.commitDiffPending };
      delete commitDiffPending[path];
      return {
        ...state,
        commitDiffPending,
        commitDiffErrors: {
          ...state.commitDiffErrors,
          [path]: renderError(event.error.key, event.error.detail)
        }
      };
    }
    case 'workingTreeFileDiff': {
      if (event.requestId !== state.workingInFlight) {
        return state;
      }
      if (
        state.pane.kind !== 'working' ||
        state.pane.staged !== (event.kind === 'staged') ||
        (state.pane.file.path !== event.file.path &&
          state.pane.file.path !== event.file.old_path &&
          state.pane.file.old_path !== event.file.path)
      ) {
        return state;
      }
      return {
        ...state,
        workingDocument: event.document,
        workingLoading: false,
        workingError: null,
        workingInFlight: null
      };
    }
    case 'workingTreeFileDiffError': {
      if (event.requestId !== state.workingInFlight) {
        return state;
      }
      if (
        state.pane.kind !== 'working' ||
        state.pane.staged !== (event.kind === 'staged') ||
        (state.pane.file.path !== event.file.path &&
          state.pane.file.path !== event.file.old_path &&
          state.pane.file.old_path !== event.file.path)
      ) {
        return state;
      }
      return {
        ...state,
        workingLoading: false,
        workingError: event.detail,
        workingInFlight: null,
        workingRefreshPending: false
      };
    }
    case 'workingTreeOperationFinished': {
      // Staging, unstaging, and discarding each say so, because the change is
      // otherwise silent: the file moves between groups and nothing on screen
      // says why. A failure names Git's own explanation.
      const scope = event.scope;
      const key = workingTreeResultKey(event.action, scope, event.success);
      return {
        ...state,
        busy: false,
        message: {
          text: event.success
            ? label(key)
            : label(key).replace('{ $error }', firstLine(event.detail)),
          ok: event.success
        }
      };
    }
    case 'commandStarted':
      return { ...state, busy: true, busyVerb: event.verb, message: null };
    case 'commandDone': {
      // Applying a patch says what it did to the tree, which "apply finished"
      // does not: the result is an unstaged change either way, and the reader
      // needs to know which files to look at.
      if (event.label === 'apply') {
        return {
          ...state,
          busy: false,
          busyVerb: null,
          message: {
            text: event.success
              ? label('patch-apply-success')
              : label('patch-apply-failed').replace('{ $error }', firstLine(event.message)),
            ok: event.success
          }
        };
      }
      // Aborting a conflicted stash pop says what it did, because "reset
      // --hard succeeded" would not say whether the stash survived.
      if (event.label === 'stash pop abort') {
        return {
          ...state,
          busy: false,
          busyVerb: null,
          message: {
            text: event.success
              ? label('stash-pop-abort-success')
              : `${label('command-failed').replace('{ $label }', event.label).replace('{ $error }', firstLine(event.message))}`,
            ok: event.success
          }
        };
      }
      return {
        ...state,
        busy: false,
        busyVerb: null,
        message: {
          text: event.success
            ? label('command-success').replace('{ $label }', event.label)
            : `${label('command-failed').replace('{ $label }', event.label).replace('{ $error }', firstLine(event.message))}`,
          ok: event.success
        }
      };
    }
    case 'openFailed':
      return {
        ...state,
        status: 'error',
        errorMessage: renderError(event.error.key, event.error.detail),
        busy: false,
        busyVerb: null
      };
    case 'statusError':
      return {
        ...state,
        status: 'error',
        errorMessage: renderError(event.error.key, event.error.detail)
      };
    case 'error':
      return {
        ...state,
        busy: false,
        busyVerb: null,
        message: {
          text: renderError(event.error.key, event.error.detail),
          ok: false
        }
      };
    default:
      return state;
  }
}

export function firstLine(text: string): string {
  return text.split('\n')[0] ?? '';
}

/** Localized i18n key describing a completed working-tree operation. */
/**
 * The catalog key reporting a working-tree operation's outcome.
 *
 * A failure has one shared key carrying Git's explanation, because the useful
 * information is the reason rather than which of the six operations failed.
 */
export function workingTreeResultKey(
  action: 'stage' | 'unstage' | 'discard',
  scope: 'file' | 'all',
  success: boolean
): string {
  if (!success) {
    return 'changes-operation-failed';
  }
  const group = scope === 'all' ? '-all' : '';
  return `changes-${action}${group}-success`;
}
