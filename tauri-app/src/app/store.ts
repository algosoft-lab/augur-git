/**
 * Application state.
 *
 * One store serves every window. The main window owns the tab list; a compare
 * or About window ignores the tab state and renders its own surface. Keeping a
 * single store means a settings change made in one window is visible in the
 * others without a second source of truth.
 */

import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { create } from 'zustand';

import * as ipc from '../bridge/ipc';
import type {
  AppConfig,
  BuildInfo,
  CheckoutTarget,
  CompareRevisionArg,
  DiffLayoutPreference,
  FileChange,
  FileStatus,
  GitAction,
  LayoutSettings,
  LocationConfig,
  RepoEvent,
  RepoSummary,
  ShortcutState,
  WindowMode,
  WorkspaceState,
  WorkingTreeDiffKind
} from '../bridge/types';
import { createTranslator, resolveLocale, type Translator } from '../i18n';
import {
  applyRepoEvent,
  codeFor,
  emptyRepo,
  firstLine,
  groupFiles,
  hasLocalBranches,
  integrationBlocked,
  isConflicted,
  isStaged,
  isUntracked,
  localBranches,
  stashCount,
  type RepoState
} from './repoState';

export type WindowRole = 'main' | 'compare' | 'about' | 'settings';

export type SidecarPage = 'changes' | 'history' | 'branches' | 'diff';
export type SidecarListPage = Exclude<SidecarPage, 'diff'>;

export interface SidecarRepoUi {
  page: SidecarPage;
  diffReturnPage: SidecarListPage;
  graphQuery: string;
  graphField: 'subject' | 'full';
  graphScrollTop: number;
  changesScrollTop: number;
  branchesScrollTop: number;
  commitDraft: string;
}

export function emptySidecarRepoUi(): SidecarRepoUi {
  return {
    page: 'changes',
    diffReturnPage: 'changes',
    graphQuery: '',
    graphField: 'subject',
    graphScrollTop: 0,
    changesScrollTop: 0,
    branchesScrollTop: 0,
    commitDraft: ''
  };
}

/** One entry in the tab bar. */
export interface TabEntry {
  /**
   * Tab identity.
   *
   * A repository tab's key is its path plus location; a start page's is
   * generated, because it has no repository yet.
   */
  key: string;
  /**
   * The repository behind this tab, or null for a start page.
   *
   * A start page is a real tab in the reference: it takes a slot, shows the
   * recent repositories, and is replaced in place when a repository is opened
   * into it rather than pushing a second tab.
   */
  repoId: number | null;
  path: string;
  location: LocationConfig;
  /** Start pages are not written to the saved workspace. */
  persisted: boolean;
}

/** The key prefix that marks a tab as a start page rather than a repository. */
const START_PAGE_PREFIX = 'start:';

/** A distinct key per start page, so two of them never collide. */
let startPages = 0;
function nextStartPage(): string {
  startPages += 1;
  return `${START_PAGE_PREFIX}${startPages}`;
}

/** Overlay dialogs. Only one is ever open. */
export type Overlay =
  | { kind: 'none' }
  | { kind: 'newBranch' }
  | { kind: 'renameBranch'; old: string }
  | { kind: 'renameRemoteBranch'; remote: string; old: string }
  | { kind: 'stash' }
  | { kind: 'stashDrop'; reference: string }
  | { kind: 'merge'; noFf: boolean }
  | { kind: 'rebase' }
  | { kind: 'deleteRef'; name: string; isTag: boolean }
  | { kind: 'deleteRemoteBranch'; remote: string; branch: string }
  | { kind: 'forcePush' }
  | { kind: 'pushSetUpstream'; branch: string; remote: string }
  | {
      kind: 'discard';
      scope: { kind: 'workingTree'; staged: boolean; all: boolean };
      trackedCount: number;
      untrackedCount: number;
    }
  | { kind: 'mergeConflict'; source: string; detail: string }
  | { kind: 'mergeError'; label: string; detail: string }
  | { kind: 'rebaseConflict'; label: string; detail: string; source?: string }
  | { kind: 'rebaseError'; label: string; detail: string }
  | { kind: 'stashPopConflict'; detail: string }
  | { kind: 'wslOpen'; distros: string[]; loading: boolean };

/** A transient message shown in the status bar of the main window. */
export interface Notice {
  level: 'info' | 'warning' | 'error';
  message: string;
}

/** How a tab that is being opened affects the active tab. */
export interface OpenOptions {
  /**
   * Whether opening this tab should also make it the active one.
   *
   * A person opens a tab to look at it, so this defaults to true. Restoring the
   * saved workspace opens tabs in order to fill them in, and the tab the window
   * is about to show has to stay the active one.
   */
  activate?: boolean;
}

interface AppStore {
  ready: boolean;
  role: WindowRole;
  compareRepoId: number | null;

  build: BuildInfo | null;
  config: AppConfig;
  workspace: WorkspaceState;
  shortcuts: ShortcutState;
  storePaths: string[];
  locale: string;
  t: Translator;

  repos: Record<number, RepoState>;
  sidecarUi: Record<number, SidecarRepoUi>;
  tabs: TabEntry[];
  activeTabKey: string | null;
  /**
   * Events for a repository that is not in `repos` yet.
   *
   * The backend starts a worker thread as part of `open_repository`, and its
   * first snapshot can reach the webview before the command's own reply does.
   * Tauri makes no ordering promise between an event and a reply, so those
   * events are held here and applied the moment the repository is adopted. The
   * cap keeps a repository that is never adopted from growing this without
   * bound; only the newest events matter, since a later snapshot supersedes an
   * earlier one.
   */
  pendingEvents: Record<number, RepoEvent[]>;

  overlay: Overlay;
  notice: Notice | null;
  /** The About window is a single instance, focused instead of duplicated. */
  aboutOpen: boolean;

  // ===== actions =====
  initialize: (role: WindowRole, compareRepoId: number | null) => Promise<void>;
  setTranslator: (locale: string, catalog: Record<string, string>) => void;
  openTab: (path: string, location?: LocationConfig, options?: OpenOptions) => Promise<void>;
  openPaths: (paths: string[]) => Promise<void>;
  closeTab: (key: string) => Promise<void>;
  selectTab: (key: string) => Promise<void>;
  reorderTab: (sourceKey: string, targetKey: string, placement: 'before' | 'after') => void;
  setActiveRepo: (repoId: number | null) => void;
  patchSidecarUi: (repoId: number, patch: Partial<SidecarRepoUi>) => void;
  setWindowMode: (mode: WindowMode) => Promise<void>;

  applyEvent: (repoId: number, event: RepoEvent) => void;
  refresh: (repoId: number) => Promise<void>;
  refreshWorkingDiff: (repoId: number) => Promise<void>;
  setBusy: (repoId: number, busy: boolean) => void;
  setMessage: (repoId: number, text: string, ok: boolean | null) => void;
  /**
   * Turn a rejected command into the repository's status message.
   *
   * One place, so that every call site localizes the backend's key instead of
   * pasting its raw detail: `err-repo-closed` has a sentence written for it, and
   * showing the detail alone means showing "repository 7 is no longer open".
   */
  reportError: (repoId: number, error: unknown) => string;
  selectCommit: (repoId: number, oid: string, short: string, subject: string) => Promise<void>;
  clearCommit: (repoId: number) => void;
  selectWorkingFile: (repoId: number, staged: boolean, file: FileStatus) => Promise<void>;
  selectCommitFile: (repoId: number, file: FileChange | null) => void;
  setLogScope: (repoId: number) => Promise<void>;

  runAction: (repoId: number, action: GitAction) => Promise<void>;
  openOverlay: (overlay: Overlay) => void;
  closeOverlay: () => void;
  /**
   * Open a start page as a new tab.
   *
   * It shows the recent repositories, and a repository opened into it takes its
   * slot rather than pushing a second tab.
   */
  addStartTab: () => void;
  notify: (notice: Notice | null) => void;

  updateLayout: (layout: Partial<LayoutSettings>) => Promise<void>;
  previewLayout: (layout: Partial<LayoutSettings>) => void;
  persistLayout: () => Promise<void>;
  setLanguage: (language: AppConfig['language']) => Promise<void>;
  setTheme: (theme: AppConfig['theme']) => Promise<void>;
  setDiffLayout: (layout: DiffLayoutPreference) => Promise<void>;
  setView: (view: Partial<AppConfig['view']>) => Promise<void>;
  setTypography: (typography: Partial<AppConfig['typography']>) => Promise<void>;
  setShortcut: (command: string, keys: string[] | null) => Promise<void>;
}

const DEFAULT_LAYOUT: LayoutSettings = {
  sidebar_width: 250,
  right_panel_width: 320,
  diff_height: null,
  file_list_ratio: 0.25
};

/** Ceiling on the buffered events kept for a repository that never appears. */
const PENDING_EVENT_LIMIT = 256;

/** The live store accessors, assigned when the store is created. */
let set: (partial: Partial<AppStore> | ((state: AppStore) => Partial<AppStore>)) => void;
let get: () => AppStore;

/**
 * Opens currently in flight, keyed by tab key.
 *
 * Deliberately outside the reactive state: it is bookkeeping for concurrent
 * callers, not something a render depends on, and putting it in the store would
 * make every observer re-render when a repository finishes opening.
 */
const opening = new Map<string, Promise<void>>();

/**
 * The integration source most recently named per repository, kept for conflict
 * recovery: the dialog that reports a conflicted merge or rebase names the
 * branch that was being integrated. Deliberately outside the reactive state,
 * like the in-flight opens.
 */
const integrationSources = new Map<number, string>();

const WORKING_DIFF_TIMEOUT_MS = 30_000;
const workingDiffTimeouts = new Map<number, ReturnType<typeof setTimeout>>();
let nextWorkingDiffRequestId = Math.floor(Math.random() * 2 ** 32) * 2 ** 21;
const COMMIT_DIFF_TIMEOUT_MS = 30_000;
const commitDiffTimeouts = new Map<
  number,
  { requestId: number; timeout: ReturnType<typeof setTimeout> }
>();
let nextCommitDiffRequestId = Date.now() * 1024;

function allocateWorkingDiffRequestId(previous: number): number {
  nextWorkingDiffRequestId = Math.max(nextWorkingDiffRequestId + 1, previous + 1);
  return nextWorkingDiffRequestId;
}

function allocateCommitDiffRequestId(previous: number): number {
  nextCommitDiffRequestId = Math.max(nextCommitDiffRequestId + 1, previous + 1, Date.now() * 1024);
  return nextCommitDiffRequestId;
}

function clearWorkingDiffTimeout(repoId: number): void {
  const timeout = workingDiffTimeouts.get(repoId);
  if (timeout !== undefined) {
    clearTimeout(timeout);
    workingDiffTimeouts.delete(repoId);
  }
}

function clearCommitDiffTimeout(repoId: number): void {
  const pending = commitDiffTimeouts.get(repoId);
  if (pending !== undefined) {
    clearTimeout(pending.timeout);
    commitDiffTimeouts.delete(repoId);
  }
}

function resetCommitDiffTimeout(repoId: number, requestId: number): void {
  clearCommitDiffTimeout(repoId);
  const timeout = setTimeout(() => {
    const pending = commitDiffTimeouts.get(repoId);
    if (pending?.requestId !== requestId) {
      return;
    }
    commitDiffTimeouts.delete(repoId);

    const repo = get().repos[repoId];
    if (!repo || repo.commitRequestId !== requestId) {
      return;
    }

    const detail = get().t('bottom-commit-diff-timeout');
    if (repo.commitFilesLoading) {
      set({
        repos: {
          ...get().repos,
          [repoId]: { ...repo, commitFilesLoading: false, commitFilesError: detail }
        }
      });
      return;
    }

    const paths = Object.keys(repo.commitDiffPending);
    if (paths.length === 0) {
      return;
    }
    set({
      repos: {
        ...get().repos,
        [repoId]: {
          ...repo,
          commitDiffPending: {},
          commitDiffErrors: {
            ...repo.commitDiffErrors,
            ...Object.fromEntries(paths.map((path) => [path, detail]))
          }
        }
      }
    });
  }, COMMIT_DIFF_TIMEOUT_MS);
  commitDiffTimeouts.set(repoId, { requestId, timeout });
}

function updateCommitDiffTimeout(repoId: number, repo: RepoState): void {
  if (repo.commitFilesLoading || Object.keys(repo.commitDiffPending).length > 0) {
    resetCommitDiffTimeout(repoId, repo.commitRequestId);
  } else {
    clearCommitDiffTimeout(repoId);
  }
}

function requestCommitFileDiff(
  repoId: number,
  requestId: number,
  oid: string,
  mergeParent: string | null,
  file: FileChange
): void {
  void ipc.loadCommitFileDiff(repoId, requestId, oid, mergeParent, file).catch((error) => {
    const repo = get().repos[repoId];
    if (
      !repo ||
      repo.commitRequestId !== requestId ||
      repo.selected?.oid !== oid ||
      repo.pane.kind !== 'commit'
    ) {
      return;
    }

    const failure = ipc.describeError(error);
    const path = file.new_path;
    const commitDiffPending = { ...repo.commitDiffPending };
    delete commitDiffPending[path];
    const next = {
      ...repo,
      commitDiffPending,
      commitDiffErrors: {
        ...repo.commitDiffErrors,
        [path]: renderGitError(get().t, failure.key, failure.detail)
      }
    };
    set({ repos: { ...get().repos, [repoId]: next } });
    updateCommitDiffTimeout(repoId, next);
  });
}

function requestWorkingDiff(
  repoId: number,
  requestId: number,
  kind: WorkingTreeDiffKind,
  file: FileStatus
): void {
  clearWorkingDiffTimeout(repoId);
  const timeout = setTimeout(() => {
    workingDiffTimeouts.delete(repoId);
    const repo = get().repos[repoId];
    if (!repo || repo.pane.kind !== 'working' || repo.workingInFlight !== requestId) {
      return;
    }
    set({
      repos: {
        ...get().repos,
        [repoId]: {
          ...repo,
          workingInFlight: null,
          workingRefreshPending: false,
          workingLoading: false,
          workingError: get().t('diff-working-tree-timeout')
        }
      }
    });
  }, WORKING_DIFF_TIMEOUT_MS);
  workingDiffTimeouts.set(repoId, timeout);

  void ipc.loadWorkingTreeDiff(repoId, requestId, kind, file).catch((error) => {
    const repo = get().repos[repoId];
    if (!repo || repo.workingInFlight !== requestId) {
      return;
    }
    clearWorkingDiffTimeout(repoId);
    const failure = ipc.describeError(error);
    set({
      repos: {
        ...get().repos,
        [repoId]: {
          ...repo,
          workingInFlight: null,
          workingRefreshPending: false,
          workingLoading: false,
          workingError: renderGitError(get().t, failure.key, failure.detail)
        }
      }
    });
  });
}

/** Merge-shaped commands whose failure can stop on conflicts. */
const MERGE_CONFLICT_LABELS = new Set(['merge', 'merge --no-ff', 'pull']);
/** Rebase-shaped commands whose failure can stop on conflicts. */
const REBASE_CONFLICT_LABELS = new Set(['rebase', 'pull --rebase']);

/**
 * Probe a repository after one of these commands failed, and turn a real
 * conflict into the matching recovery dialog.
 *
 * The command's own failure has already reached the status bar, so a probe
 * that finds no conflict, fails itself, or races a closed repository stays
 * silent: the status text is then all there is to say.
 */
function watchForConflicts(repoId: number, label: string, detail: string): void {
  const merge = MERGE_CONFLICT_LABELS.has(label);
  const rebase = REBASE_CONFLICT_LABELS.has(label);
  const stash = label === 'stash pop';
  if (!merge && !rebase && !stash) {
    return;
  }
  // Git leaves the unmerged index behind on a failed command, but the worker
  // only refreshes the snapshot after success, so one is requested here; the
  // disabled integration actions and the conflicted rows follow from it.
  void useStore.getState().refresh(repoId);
  if (merge) {
    void openMergeConflict(repoId, label, detail);
  } else if (rebase) {
    void openRebaseConflict(repoId, label, detail);
  } else {
    void openStashPopConflict(repoId, detail);
  }
}

async function openMergeConflict(repoId: number, label: string, detail: string): Promise<void> {
  // A pull merges its upstream, which the status snapshot already carries; a
  // plain merge names the branch that was picked when it started. The probe's
  // source only feeds target resolution, so a stale or unknown name still
  // yields the repository state the decision needs.
  const repo = useStore.getState().repos[repoId];
  const source =
    label === 'pull' ? (repo?.upstream ?? 'pull') : (integrationSources.get(repoId) ?? label);
  try {
    const probe = await ipc.probeMerge(repoId, source);
    const store = useStore.getState();
    if (!store.repos[repoId] || !(probe.has_conflicts || probe.merge_head !== null)) {
      return;
    }
    store.openOverlay({
      kind: 'mergeConflict',
      source,
      // Git's own output names the conflicted files; the muted line above it
      // already carries the warning text.
      detail
    });
  } catch {
    // The repository closed or the probe failed; the status-bar failure text
    // is already showing what went wrong.
  }
}

async function openRebaseConflict(repoId: number, label: string, detail: string): Promise<void> {
  try {
    const probe = await ipc.probeRebase(repoId, null);
    const store = useStore.getState();
    const conflicted = probe.has_conflicts || probe.rebase_in_progress || probe.rebase_head;
    if (!store.repos[repoId] || !conflicted) {
      return;
    }
    // A conflict raised by a pull rebase has no other source to name.
    const source =
      label === 'pull --rebase' ? undefined : (integrationSources.get(repoId) ?? undefined);
    store.openOverlay({ kind: 'rebaseConflict', label, source, detail });
  } catch {
    // The repository closed or the probe failed; the status-bar failure text
    // is already showing what went wrong.
  }
}

async function openStashPopConflict(repoId: number, detail: string): Promise<void> {
  try {
    // The probe's source is unused here: only the repository state decides,
    // because a conflicted stash pop leaves an unmerged index but no merge.
    const probe = await ipc.probeMerge(repoId, 'stash pop');
    const store = useStore.getState();
    if (!store.repos[repoId] || !(probe.has_conflicts || probe.merge_head !== null)) {
      return;
    }
    store.openOverlay({ kind: 'stashPopConflict', detail });
  } catch {
    // The repository closed or the probe failed; the status-bar failure text
    // is already showing what went wrong.
  }
}

/**
 * Write the repository tabs to the saved workspace.
 *
 * Start pages are left out: they are a way into a repository, not a repository,
 * and restoring one on the next launch would show a page the person did not ask
 * to see.
 */
function persistTabs(activeKey: string | null): void {
  const state = useStore.getState();
  const saved = state.tabs.filter((tab) => tab.persisted);
  void ipc.setWorkspaceTabs(
    saved.map((tab) => ({ path: tab.path, location: tab.location })),
    activeKey
  );
}

/**
 * Ask the backend for a repository and adopt whatever it sends back.
 *
 * Split out of `openTab` so the caller can register the promise before the first
 * await. A failure is reported as a notice and releases the tab's claim, because
 * there is no repository to show.
 */
async function startOpen(key: string, path: string, location: LocationConfig): Promise<void> {
  let summary: RepoSummary;
  try {
    summary = await ipc.openRepository(path, location);
  } catch (error) {
    const failure = ipc.describeError(error);
    set((state) => ({
      // The claim is released, because there is no repository to show.
      tabs: state.tabs.filter((tab) => tab.key !== key),
      activeTabKey:
        state.activeTabKey === key
          ? (state.tabs.find((tab) => tab.key !== key)?.key ?? null)
          : state.activeTabKey,
      notice: {
        level: 'error',
        message: renderGitError(state.t, failure.key, failure.detail)
      }
    }));
    return;
  }
  set((state) => ({
    repos: {
      ...state.repos,
      [summary.id]: adopt(state, summary.id, summary.path, summary.location)
    },
    tabs: state.tabs.map((tab) => (tab.key === key ? { ...tab, repoId: summary.id } : tab))
  }));
  // Events that arrived before the repository was known are applied now, so a
  // late adoption is indistinguishable from an early one.
  const buffered = get().pendingEvents[summary.id];
  if (buffered) {
    const { [summary.id]: _drained, ...rest } = get().pendingEvents;
    set({ pendingEvents: rest });
    for (const event of buffered) {
      get().applyEvent(summary.id, event);
    }
  }
  // The active tab is recorded rather than assumed to be this one: a tab opened
  // in the background while the workspace is being restored must not overwrite
  // the tab the window is showing.
  persistTabs(get().activeTabKey);
}

/**
 * The in-flight first boot.
 *
 * React's strict mode runs the start-up effect twice, and a second
 * `initialize` would rebuild the tab list out of the bootstrap response and
 * discard the repositories the first one is still opening. A window's role and
 * compare target are read once and never change, so the second caller waits for
 * the first instead of starting the whole restore again.
 */
let booting: Promise<void> | null = null;

/**
 * Load the saved state and give every tab a repository.
 *
 * The backend keeps no repository open across launches, so the saved tab list
 * arrives as claims: paths with nothing behind them. The tab the workspace was
 * saved with is opened first, so the window the person is about to look at fills
 * in before the tabs behind it, and no tab is activated on the way, because the
 * saved selection is what the window should show.
 */
async function restoreSession(role: WindowRole, compareRepoId: number | null): Promise<void> {
  const boot = await ipc.bootstrap();
  const locale = resolveLocale(boot.config.language, systemLanguage());
  set({
    role,
    compareRepoId,
    build: boot.build,
    config: boot.config,
    workspace: boot.workspace,
    shortcuts: boot.shortcuts,
    storePaths: boot.store_paths,
    locale,
    t: createTranslator(boot.catalogs)
  });

  // Every already-open repository is re-adopted so a reloaded window
  // resubscribes to a worker it does not own.
  const staged = useStore.getState();
  const repos: Record<number, RepoState> = {};
  for (const summary of boot.repositories) {
    repos[summary.id] = adopt(staged, summary.id, summary.path, summary.location);
  }
  const { pendingEvents } = staged;
  for (const summary of boot.repositories) {
    delete pendingEvents[summary.id];
  }
  const restoredTabs = new Map<string, TabEntry>();
  for (const tab of boot.workspace.open_tabs) {
    const key = tabKey(tab.path, tab.location);
    if (restoredTabs.has(key)) {
      continue;
    }
    const existing = boot.repositories.find(
      (summary) => tabKey(summary.path, summary.location) === key
    );
    restoredTabs.set(key, {
      key,
      repoId: existing?.id ?? null,
      path: tab.path,
      location: tab.location,
      // A restored tab is saved, unlike one that is still being opened.
      persisted: true
    });
  }
  const tabs = [...restoredTabs.values()];

  // A path handed over by the command line can arrive before the bootstrap
  // response, and that tab must survive: replacing the list with the saved one
  // would drop it and then reopen the same repository as a second tab.
  for (const open of staged.tabs) {
    if (!tabs.some((tab) => tab.key === open.key)) {
      tabs.push(open);
    }
  }
  const activeTabKey =
    boot.workspace.active_tab && tabs.some((tab) => tab.key === boot.workspace.active_tab)
      ? boot.workspace.active_tab
      : (staged.activeTabKey ?? tabs[0]?.key ?? null);
  set({ repos, tabs, activeTabKey, ready: true });

  // Windows opened without a repository argument still need the tab list, so
  // missing sessions are started here.
  const pending = tabs.filter((tab) => tab.repoId === null);
  const shown = pending.findIndex((tab) => tab.key === activeTabKey);
  if (shown > 0) {
    pending.unshift(...pending.splice(shown, 1));
  }
  for (const tab of pending) {
    await get().openTab(tab.path, tab.location, { activate: false });
  }

  // Paths handed over before this window was listening, which the bootstrap
  // said are waiting. Collected here rather than delivered as an event,
  // because an event emitted before the subscription exists is lost. One of
  // them is the reason the window was opened, so it is the tab it shows.
  let handed: string | null = null;
  if (boot.has_pending_paths) {
    for (const path of await ipc.takePendingPaths()) {
      const key = tabKey(path, { kind: 'local' });
      await get().openTab(path, { kind: 'local' });
      // A path the backend refuses to open releases its tab again, and then the
      // tab the workspace was saved with is the one left to show.
      if (get().tabs.some((entry) => entry.key === key && entry.repoId !== null)) {
        handed = key;
      }
    }
  }

  // An adopted repository has a worker this window did not start, so nothing
  // will be pushed until it asks, and a repository this window did start can
  // have had its first snapshot lost to a subscription that was not yet there.
  // Without this a restored tab can sit empty until it is clicked.
  if (role === 'compare' && compareRepoId !== null) {
    if (get().repos[compareRepoId]) {
      await get().refresh(compareRepoId);
    }
    return;
  }
  const wanted = handed ?? activeTabKey;
  if (!wanted) {
    return;
  }
  const tab = get().tabs.find((entry) => entry.key === wanted);
  if (!tab || tab.repoId === null) {
    return;
  }
  if (get().activeTabKey === wanted) {
    await get().refresh(tab.repoId);
    return;
  }
  // Selecting also records the choice, so the window does not come back on a
  // different tab next time.
  await get().selectTab(wanted);
}

/**
 * Create a repository's state and fold in any events that arrived first.
 *
 * The buffered events are replayed in order, so a repository that is adopted
 * late still ends up with the same state as one adopted immediately.
 */
function adopt(state: AppStore, id: number, path: string, location: LocationConfig): RepoState {
  let repo = emptyRepo(id, path, location);
  for (const event of state.pendingEvents[id] ?? []) {
    repo = applyRepoEvent(repo, event, state.t, (key, detail) =>
      renderGitError(state.t, key, detail)
    );
  }
  return repo;
}

function systemLanguage(): string {
  return typeof navigator === 'undefined' ? 'en-US' : navigator.language;
}

function tabKey(path: string, location: LocationConfig): string {
  return location.kind === 'wsl' ? `wsl:${location.distro ?? ''}:${path}` : path;
}

/**
 * Render a backend error.
 *
 * The error keys are catalog entries that already carry a `{ $detail }`
 * placeholder, so the raw text is substituted in rather than appended. A key the
 * catalog does not know degrades to the key plus the detail instead of showing
 * nothing at all.
 */
export function renderGitError(t: Translator, key: string, detail: string): string {
  const template = t(key);
  const trimmed = firstLine(detail);
  if (template === key) {
    return trimmed ? `${key}: ${trimmed}` : key;
  }
  return template.replace(/\{\s*\$detail\s*\}/g, trimmed);
}

export const useStore = create<AppStore>((storeSet, storeGet) => {
  // Kept at module scope so a helper outside the store object can read and
  // write the same state, rather than reaching back through the hook.
  set = storeSet;
  get = storeGet;
  return {
    ready: false,
    role: 'main',
    compareRepoId: null,

    build: null,
    config: {
      schema_version: 2,
      theme: 'claude-dark',
      language: 'system',
      view: {
        show_untracked: true,
        auto_follow: true,
        diff_layout: 'inline',
        graph_history: 'all-branches',
        auto_refresh: true,
        commit_action: 'commit',
        pull_action: 'rebase'
      },
      typography: {
        ui_font_family: null,
        mono_font_family: null,
        ui_font_size: 16,
        diff_font_size: 16
      },
      recent_repos: []
    },
    workspace: {
      schema_version: 2,
      open_tabs: [],
      active_tab: null,
      layout: DEFAULT_LAYOUT,
      window_mode: 'desktop',
      desktop_window: null,
      sidecar_window: null
    },
    shortcuts: { resolved: [], defaults: [], overrides: {} },
    storePaths: [],
    locale: 'en-US',
    t: (key: string) => key,

    repos: {},
    sidecarUi: {},
    tabs: [],
    activeTabKey: null,
    pendingEvents: {},

    overlay: { kind: 'none' },
    notice: null,
    aboutOpen: false,

    async initialize(role, compareRepoId) {
      if (get().ready) {
        return;
      }
      if (!booting) {
        booting = restoreSession(role, compareRepoId).finally(() => {
          booting = null;
        });
      }
      await booting;
    },

    setTranslator(locale, catalog) {
      set({ locale, t: createTranslator(catalog) });
    },

    async openTab(path, location, options) {
      const activate = options?.activate ?? true;
      const target: LocationConfig = location ?? { kind: 'local' };
      const key = tabKey(path, target);
      const existing = get().tabs.find((tab) => tab.key === key);
      if (existing && existing.repoId !== null) {
        if (activate) {
          await get().selectTab(key);
        }
        return;
      }

      // A tab that is present but has no repository yet was adopted from the saved
      // list, which happens before the window knows which repositories the backend
      // has open. The claim is completed here rather than short-circuited, or the
      // tab would sit empty forever.
      const inFlight = opening.get(key);
      if (inFlight) {
        // Something else is opening this very repository, so wait for it rather
        // than starting a second worker for the same path.
        await inFlight;
        if (activate) {
          await get().selectTab(key);
        }
        return;
      }

      // The slot is claimed before the request, not after it, so a second drop of
      // the same folder finds the tab already there. The claim is registered
      // synchronously too, so two callers cannot both decide to start.
      set((state) => {
        if (state.tabs.some((tab) => tab.key === key)) {
          return activate ? { activeTabKey: key } : {};
        }
        // A start page is a slot waiting for a repository, so it is filled rather
        // than left behind with a second tab beside it.
        const slot = state.tabs.findIndex((tab) => tab.repoId === null);
        const claim: TabEntry = { key, repoId: null, path, location: target, persisted: true };
        const tabs = [...state.tabs];
        if (slot >= 0) {
          tabs[slot] = claim;
        } else {
          tabs.push(claim);
        }
        return activate ? { tabs, activeTabKey: key } : { tabs };
      });
      const pending = startOpen(key, path, target);
      opening.set(key, pending);
      try {
        await pending;
      } finally {
        opening.delete(key);
      }
      if (activate) {
        await get().selectTab(key);
      }
    },

    async openPaths(paths) {
      for (const path of paths) {
        await get().openTab(path);
      }
    },

    async closeTab(key) {
      const state = get();
      const index = state.tabs.findIndex((tab) => tab.key === key);
      if (index < 0) {
        return;
      }
      const tab = state.tabs[index]!;
      const tabs = state.tabs.filter((entry) => entry.key !== key);
      let activeTabKey = state.activeTabKey;
      if (activeTabKey === key) {
        const fallback = tabs[index] ?? tabs[index - 1];
        activeTabKey = fallback?.key ?? null;
      }
      // A start page has no repository behind it, so there is nothing to release.
      if (tab.repoId !== null) {
        clearWorkingDiffTimeout(tab.repoId);
        clearCommitDiffTimeout(tab.repoId);
        const repos = { ...state.repos };
        delete repos[tab.repoId];
        const sidecarUi = { ...state.sidecarUi };
        delete sidecarUi[tab.repoId];
        const pendingEvents = { ...state.pendingEvents };
        delete pendingEvents[tab.repoId];
        integrationSources.delete(tab.repoId);
        set({ tabs, activeTabKey, repos, sidecarUi, pendingEvents });
        await ipc.closeRepository(tab.repoId);
        await ipc.closeCompareWindow(tab.repoId);
      } else {
        set({ tabs, activeTabKey });
      }
      persistTabs(activeTabKey);
      if (activeTabKey) {
        const next = get().tabs.find((entry) => entry.key === activeTabKey);
        if (next?.repoId !== null && next?.repoId !== undefined) {
          void get().refresh(next.repoId);
        }
      }
    },

    async selectTab(key) {
      set({ activeTabKey: key });
      persistTabs(key);
      const tab = get().tabs.find((entry) => entry.key === key);
      if (tab && tab.repoId !== null) {
        void get().refresh(tab.repoId);
      }
    },

    reorderTab(sourceKey, targetKey, placement) {
      const state = get();
      if (sourceKey === targetKey) {
        return;
      }

      const sourceIndex = state.tabs.findIndex((tab) => tab.key === sourceKey);
      const targetIndex = state.tabs.findIndex((tab) => tab.key === targetKey);
      if (sourceIndex < 0 || targetIndex < 0) {
        return;
      }

      const remaining = state.tabs.filter((tab) => tab.key !== sourceKey);
      const remainingTargetIndex = remaining.findIndex((tab) => tab.key === targetKey);
      const insertionIndex = remainingTargetIndex + (placement === 'after' ? 1 : 0);
      const tabs = [
        ...remaining.slice(0, insertionIndex),
        state.tabs[sourceIndex]!,
        ...remaining.slice(insertionIndex)
      ];
      if (tabs.every((tab, index) => tab.key === state.tabs[index]?.key)) {
        return;
      }

      set({ tabs });
      persistTabs(state.activeTabKey);
    },

    /**
     * Open a start page as a new tab.
     *
     * The only tab kind that is not a repository.
     *
     * It shows the recent repositories, and a repository opened into it takes its
     * slot rather than pushing a second tab, which is what the reference does.
     */
    addStartTab() {
      const key = `${START_PAGE_PREFIX}${nextStartPage()}`;
      set((state) => ({
        tabs: [
          ...state.tabs,
          { key, repoId: null, path: '', location: { kind: 'local' }, persisted: false }
        ],
        activeTabKey: key
      }));
    },

    setActiveRepo(repoId) {
      set({ compareRepoId: repoId });
    },

    patchSidecarUi(repoId, patch) {
      const current = get().sidecarUi[repoId] ?? emptySidecarRepoUi();
      set({ sidecarUi: { ...get().sidecarUi, [repoId]: { ...current, ...patch } } });
    },

    async setWindowMode(mode) {
      const workspace = await ipc.setWindowMode(mode);
      set({ workspace });
    },

    applyEvent(repoId, event) {
      const state = get();
      if (event.type === 'commandDone' && event.label === 'copy-commit-message') {
        // The operation exists only to produce the message, so it reports
        // success or failure in its own words rather than "copy-commit-message
        // finished", and a success goes to the clipboard. The repository is
        // released here because this branch returns before the reducer runs.
        const repo = state.repos[repoId];
        if (repo) {
          set({
            repos: {
              ...state.repos,
              [repoId]: {
                ...repo,
                busy: false,
                busyVerb: null,
                message: event.success
                  ? { text: state.t('context-copied-commit-message'), ok: true }
                  : {
                      text: state
                        .t('context-copy-commit-message-failed')
                        .replace('{ $error }', firstLine(event.message)),
                      ok: false
                    }
              }
            }
          });
        }
        if (event.success) {
          void writeText(event.message);
        }
        return;
      }
      const repo = state.repos[repoId];
      if (!repo) {
        // The repository is still being opened. Holding the event is what keeps
        // the first status from being lost to the race between the command reply
        // and the worker's first snapshot.
        const queue = state.pendingEvents[repoId] ?? [];
        set({
          pendingEvents: {
            ...state.pendingEvents,
            [repoId]: [...queue, event].slice(-PENDING_EVENT_LIMIT)
          }
        });
        return;
      }
      const t = state.t;
      const next = applyRepoEvent(repo, event, t, (key, detail) => renderGitError(t, key, detail));
      if (next !== repo) {
        set({ repos: { ...state.repos, [repoId]: next } });
      }
      if (
        event.type === 'commandDone' &&
        event.success &&
        (event.label === 'commit' || event.label === 'commit --amend')
      ) {
        get().patchSidecarUi(repoId, { commitDraft: '' });
      }
      if (next !== repo && event.type === 'commitFiles') {
        if (next.commitDiffPending && Object.keys(next.commitDiffPending).length > 0) {
          updateCommitDiffTimeout(repoId, next);
          for (const file of event.files) {
            requestCommitFileDiff(repoId, event.requestId, event.oid, event.merge_parent, file);
          }
        } else {
          clearCommitDiffTimeout(repoId);
        }
      } else if (
        next !== repo &&
        (event.type === 'commitFilesError' ||
          event.type === 'fileDiff' ||
          event.type === 'fileDiffError')
      ) {
        updateCommitDiffTimeout(repoId, next);
      }
      if (
        next !== repo &&
        (event.type === 'workingTreeFileDiff' || event.type === 'workingTreeFileDiffError')
      ) {
        const refreshAgain = repo.workingRefreshPending;
        clearWorkingDiffTimeout(repoId);
        if (refreshAgain) {
          void get().refreshWorkingDiff(repoId);
        }
      }
      if (event.type === 'status') {
        void get().refreshWorkingDiff(repoId);
      }
      // A failed integration command may have stopped on conflicts; the probe
      // runs after the reducer so the status text is never delayed by it.
      if (event.type === 'commandDone' && !event.success) {
        watchForConflicts(repoId, event.label, event.message);
      }
    },

    async refresh(repoId) {
      const repo = get().repos[repoId];
      if (!repo || repo.busy) {
        return;
      }
      try {
        await ipc.refreshRepository(repoId);
      } catch {
        // A repository that closed while the request was in flight is not an error
        // worth surfacing; the tab is already gone.
      }
    },

    async refreshWorkingDiff(repoId) {
      const state = get();
      const activeTab = state.tabs.find((tab) => tab.key === state.activeTabKey);
      const repo = state.repos[repoId];
      if (!repo || activeTab?.repoId !== repoId || repo.pane.kind !== 'working') {
        return;
      }

      const pane = repo.pane;
      const file = repo.files.find(
        (entry) =>
          (entry.path === pane.file.path || entry.old_path === pane.file.path) &&
          (pane.staged ? isStaged(entry) : entry.worktree !== ' ' || isConflicted(entry))
      );
      if (!file) {
        clearWorkingDiffTimeout(repoId);
        set({
          repos: {
            ...get().repos,
            [repoId]: {
              ...repo,
              workingInFlight: null,
              workingRefreshPending: false,
              pane: { kind: 'none' },
              workingDocument: null,
              workingLoading: false,
              workingError: null
            }
          }
        });
        return;
      }

      const sameFileIdentity = pane.file.path === file.path && pane.file.old_path === file.old_path;
      if (repo.workingInFlight !== null && sameFileIdentity) {
        set({
          repos: {
            ...get().repos,
            [repoId]: {
              ...repo,
              pane: { ...pane, file },
              workingRefreshPending: true
            }
          }
        });
        return;
      }

      clearWorkingDiffTimeout(repoId);
      const requestId = allocateWorkingDiffRequestId(repo.workingRequest);
      const kind: WorkingTreeDiffKind = pane.staged ? 'staged' : 'unstaged';
      set({
        repos: {
          ...get().repos,
          [repoId]: {
            ...repo,
            workingRequest: requestId,
            workingInFlight: requestId,
            workingRefreshPending: false,
            pane: { ...pane, file },
            workingLoading: repo.workingDocument === null && repo.workingError === null
          }
        }
      });
      requestWorkingDiff(repoId, requestId, kind, file);
    },

    setBusy(repoId, busy) {
      const repo = get().repos[repoId];
      if (!repo) {
        return;
      }
      set({
        repos: { ...get().repos, [repoId]: { ...repo, busy, busyVerb: null } }
      });
    },

    setMessage(repoId, text, ok) {
      const repo = get().repos[repoId];
      if (!repo) {
        return;
      }
      set({ repos: { ...get().repos, [repoId]: { ...repo, message: { text, ok } } } });
    },

    reportError(repoId, error) {
      const failure = ipc.describeError(error);
      const text = renderGitError(get().t, failure.key, failure.detail);
      get().setMessage(repoId, text, false);
      return text;
    },

    async selectCommit(repoId, oid, short, subject) {
      const repo = get().repos[repoId];
      if (!repo) {
        return;
      }
      clearWorkingDiffTimeout(repoId);
      const requestId = allocateCommitDiffRequestId(repo.commitRequestId);
      clearCommitDiffTimeout(repoId);
      set({
        repos: {
          ...get().repos,
          [repoId]: {
            ...repo,
            selected: { oid, short, subject },
            commitRequestId: requestId,
            commitFiles: [],
            commitFilesLoading: true,
            commitFilesError: null,
            commitMergeParent: null,
            workingInFlight: null,
            workingRefreshPending: false,
            workingLoading: false,
            workingError: null,
            workingDocument: null,
            // Every changed file is shown until one is chosen, matching the
            // reference application.
            pane: { kind: 'commit', file: null },
            commitDiffs: {},
            commitDiffPending: {},
            commitDiffErrors: {}
          }
        }
      });
      resetCommitDiffTimeout(repoId, requestId);
      await ipc.selectCommit(repoId, requestId, oid).catch((error) => {
        const current = get().repos[repoId];
        if (!current || current.commitRequestId !== requestId) {
          return;
        }
        clearCommitDiffTimeout(repoId);
        const failure = ipc.describeError(error);
        set({
          repos: {
            ...get().repos,
            [repoId]: {
              ...current,
              commitFilesLoading: false,
              commitFilesError: renderGitError(get().t, failure.key, failure.detail)
            }
          }
        });
      });
    },

    clearCommit(repoId) {
      const repo = get().repos[repoId];
      if (!repo) {
        return;
      }
      clearCommitDiffTimeout(repoId);
      set({
        repos: {
          ...get().repos,
          [repoId]: {
            ...repo,
            selected: null,
            commitFiles: [],
            commitFilesLoading: false,
            commitFilesError: null,
            commitMergeParent: null,
            pane: { kind: 'none' },
            commitDiffs: {},
            commitDiffPending: {},
            commitDiffErrors: {}
          }
        }
      });
    },

    async selectWorkingFile(repoId, staged, file) {
      const repo = get().repos[repoId];
      if (!repo) {
        return;
      }
      clearWorkingDiffTimeout(repoId);
      clearCommitDiffTimeout(repoId);
      const requestId = allocateWorkingDiffRequestId(repo.workingRequest);
      const kind: WorkingTreeDiffKind = staged ? 'staged' : 'unstaged';
      set({
        repos: {
          ...get().repos,
          [repoId]: {
            ...repo,
            workingRequest: requestId,
            workingInFlight: requestId,
            workingRefreshPending: false,
            pane: { kind: 'working', staged, file },
            workingDocument: null,
            workingLoading: true,
            workingError: null
          }
        }
      });
      requestWorkingDiff(repoId, requestId, kind, file);
    },

    selectCommitFile(repoId, file) {
      const repo = get().repos[repoId];
      if (!repo?.selected) {
        return;
      }
      set({
        repos: { ...get().repos, [repoId]: { ...repo, pane: { kind: 'commit', file } } }
      });
    },

    async setLogScope(repoId) {
      const repo = get().repos[repoId];
      if (!repo) {
        return;
      }
      const scope = get().config.view.graph_history;
      const upstream = scope === 'current-branch' ? repo.upstream : null;
      try {
        await ipc.setLogScope(repoId, upstream);
        set({
          repos: { ...get().repos, [repoId]: { ...repo, logScopeSent: true } }
        });
      } catch {
        // The repository closed; the tab is already gone.
      }
    },

    async runAction(repoId, action) {
      if (action.action === 'merge' || action.action === 'rebase') {
        // Remembered only so a later conflict dialog can name the source.
        integrationSources.set(repoId, action.source);
      }
      get().setBusy(repoId, true);
      try {
        await ipc.runAction(repoId, action);
      } catch (error) {
        get().setBusy(repoId, false);
        get().reportError(repoId, error);
      }
    },

    openOverlay(overlay) {
      set({ overlay });
    },

    closeOverlay() {
      set({ overlay: { kind: 'none' } });
    },

    notify(notice) {
      set({ notice });
    },

    async updateLayout(patch) {
      const layout: LayoutSettings = { ...get().workspace.layout, ...patch };
      set({ workspace: { ...get().workspace, layout } });
      await ipc.setLayout(layout);
    },

    previewLayout(patch) {
      const layout: LayoutSettings = { ...get().workspace.layout, ...patch };
      set({ workspace: { ...get().workspace, layout } });
    },

    async persistLayout() {
      await ipc.setLayout(get().workspace.layout);
    },

    async setLanguage(language) {
      const config = { ...get().config, language };
      set({ config });
      await ipc.setLanguage(language);
      // The backend rebuilds the native menu from the same preference, so the
      // catalogs are refetched rather than merged here.
      const boot = await ipc.bootstrap();
      const locale = boot.locale;
      set({ locale, t: createTranslator(boot.catalogs) });
    },

    async setTheme(theme) {
      set({ config: { ...get().config, theme } });
      await ipc.setTheme(theme);
    },

    async setDiffLayout(layout) {
      const view = { ...get().config.view, diff_layout: layout };
      set({ config: { ...get().config, view } });
      await ipc.setDiffLayout(layout);
    },

    async setView(patch) {
      const view = { ...get().config.view, ...patch };
      set({ config: { ...get().config, view } });
      await ipc.setView(view);
    },

    async setTypography(patch) {
      const typography = { ...get().config.typography, ...patch };
      set({ config: { ...get().config, typography } });
      await ipc.setTypography(typography);
    },

    async setShortcut(command, keys) {
      const shortcuts = await ipc.setShortcut(command, keys);
      set({ shortcuts });
    }
  };
});

/** The repository backing the active tab, if any. */
export function activeRepo(state: AppStore): RepoState | null {
  return state.activeTabKey ? tabRepo(state, state.activeTabKey) : null;
}

/** The repository behind a tab, or null for a start page. */
export function tabRepo(state: AppStore, key: string): RepoState | null {
  const tab = state.tabs.find((entry) => entry.key === key);
  if (!tab || tab.repoId === null) {
    return null;
  }
  return state.repos[tab.repoId] ?? null;
}

/** Compare revision argument for a checkout-style ref. */
export function revisionFromTarget(target: CheckoutTarget): CompareRevisionArg {
  switch (target.kind) {
    case 'localBranch':
      return { kind: 'local', name: target.localBranch };
    case 'remoteBranch':
      return { kind: 'remote', name: target.remoteBranch };
    case 'tag':
      return { kind: 'tag', name: target.tag };
    case 'commit':
      return { kind: 'commit', name: target.commit };
  }
}

export {
  codeFor,
  groupFiles,
  hasLocalBranches,
  integrationBlocked,
  isConflicted,
  isStaged,
  isUntracked,
  localBranches,
  stashCount,
  tabKey
};
export type { RepoState };
