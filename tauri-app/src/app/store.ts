/**
 * Application state.
 *
 * One store serves every window. The main window owns the tab list; a compare
 * or About window ignores the tab state and renders its own surface. Keeping a
 * single store means a settings change made in one window is visible in the
 * others without a second source of truth.
 */

import { create } from "zustand";

import * as ipc from "../bridge/ipc";
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
  WorkspaceState,
} from "../bridge/types";
import { createTranslator, resolveLocale, type Translator } from "../i18n";
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
  type RepoState,
} from "./repoState";

export type WindowRole = "main" | "compare" | "about";

/** One entry in the tab bar. */
export interface TabEntry {
  /** Repository identity: the path plus its location. */
  key: string;
  repoId: number;
  path: string;
  location: LocationConfig;
}

/** Overlay dialogs. Only one is ever open. */
export type Overlay =
  | { kind: "none" }
  | { kind: "newBranch" }
  | { kind: "renameBranch"; old: string }
  | { kind: "renameRemoteBranch"; remote: string; old: string }
  | { kind: "stash" }
  | { kind: "stashDrop"; reference: string }
  | { kind: "merge"; noFf: boolean }
  | { kind: "rebase" }
  | { kind: "deleteRef"; name: string; isTag: boolean }
  | { kind: "deleteRemoteBranch"; remote: string; branch: string }
  | { kind: "forcePush" }
  | { kind: "pushSetUpstream"; branch: string; remote: string }
  | {
      kind: "discard";
      scope: { kind: "workingTree"; staged: boolean; all: boolean };
      trackedCount: number;
      untrackedCount: number;
    }
  | { kind: "mergeConflict"; source: string; detail: string }
  | { kind: "mergeError"; label: string; detail: string }
  | { kind: "rebaseConflict"; detail: string }
  | { kind: "rebaseError"; label: string; detail: string }
  | { kind: "cliReport"; operation: "install" | "remove"; report: CliReport }
  | { kind: "wslOpen"; distros: string[]; loading: boolean };

/** The shell-installer report, pre-formatted for display. */
export interface CliReport {
  operation: "install" | "remove";
  entries: { path: string; outcome: string }[];
  fallbackBinary: boolean;
}

/** A transient message shown in the status bar of the main window. */
export interface Notice {
  level: "info" | "warning" | "error";
  message: string;
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

  settingsOpen: boolean;
  overlay: Overlay;
  notice: Notice | null;
  /** The About window is a single instance, focused instead of duplicated. */
  aboutOpen: boolean;

  // ===== actions =====
  initialize: (role: WindowRole, compareRepoId: number | null) => Promise<void>;
  setTranslator: (locale: string, catalog: Record<string, string>) => void;
  openTab: (path: string, location?: LocationConfig) => Promise<void>;
  openPaths: (paths: string[]) => Promise<void>;
  closeTab: (key: string) => Promise<void>;
  selectTab: (key: string) => Promise<void>;
  setActiveRepo: (repoId: number | null) => void;

  applyEvent: (repoId: number, event: RepoEvent) => void;
  refresh: (repoId: number) => Promise<void>;
  setBusy: (repoId: number, busy: boolean) => void;
  setMessage: (repoId: number, text: string, ok: boolean | null) => void;
  selectCommit: (repoId: number, oid: string, short: string, subject: string) => Promise<void>;
  clearCommit: (repoId: number) => void;
  selectWorkingFile: (repoId: number, staged: boolean, file: FileStatus) => Promise<void>;
  selectCommitFile: (repoId: number, file: FileChange) => Promise<void>;
  setLogScope: (repoId: number) => Promise<void>;

  runAction: (repoId: number, action: GitAction) => Promise<void>;
  openOverlay: (overlay: Overlay) => void;
  closeOverlay: () => void;
  setSettingsOpen: (open: boolean) => void;
  notify: (notice: Notice | null) => void;

  updateLayout: (layout: Partial<LayoutSettings>) => Promise<void>;
  setLanguage: (language: AppConfig["language"]) => Promise<void>;
  setTheme: (theme: AppConfig["theme"]) => Promise<void>;
  setDiffLayout: (layout: DiffLayoutPreference) => Promise<void>;
  setView: (view: Partial<AppConfig["view"]>) => Promise<void>;
  setTypography: (
    typography: Partial<AppConfig["typography"]>,
  ) => Promise<void>;
  setShortcut: (command: string, keys: string[] | null) => Promise<void>;
}

const DEFAULT_LAYOUT: LayoutSettings = {
  sidebar_width: 250,
  right_panel_width: 320,
  diff_height: null,
  file_list_ratio: 0.25,
};

/** Ceiling on the buffered events kept for a repository that never appears. */
const PENDING_EVENT_LIMIT = 256;

/**
 * Create a repository's state and fold in any events that arrived first.
 *
 * The buffered events are replayed in order, so a repository that is adopted
 * late still ends up with the same state as one adopted immediately.
 */
function adopt(
  state: AppStore,
  id: number,
  path: string,
  location: LocationConfig,
): RepoState {
  let repo = emptyRepo(id, path, location);
  for (const event of state.pendingEvents[id] ?? []) {
    repo = applyRepoEvent(repo, event, state.t, (key, detail) =>
      renderGitError(state.t, key, detail),
    );
  }
  return repo;
}

function systemLanguage(): string {
  return typeof navigator === "undefined" ? "en-US" : navigator.language;
}

function tabKey(path: string, location: LocationConfig): string {
  return location.kind === "wsl" ? `wsl:${location.distro ?? ""}:${path}` : path;
}

/**
 * Render a backend error.
 *
 * The error keys are catalog entries that already carry a `{ $detail }`
 * placeholder, so the raw text is substituted in rather than appended. A key the
 * catalog does not know degrades to the key plus the detail instead of showing
 * nothing at all.
 */
export function renderGitError(
  t: Translator,
  key: string,
  detail: string,
): string {
  const template = t(key);
  const trimmed = firstLine(detail);
  if (template === key) {
    return trimmed ? `${key}: ${trimmed}` : key;
  }
  return template.replace(/\{\s*\$detail\s*\}/g, trimmed);
}

export const useStore = create<AppStore>((set, get) => ({
  ready: false,
  role: "main",
  compareRepoId: null,

  build: null,
  config: {
    schema_version: 1,
    theme: "catppuccin-mocha",
    language: "system",
    view: {
      show_untracked: true,
      auto_follow: true,
      diff_layout: "side-by-side",
      graph_history: "all-branches",
      auto_refresh_on_focus: true,
      commit_action: "commit",
    },
    typography: {
      ui_font_family: null,
      mono_font_family: null,
      ui_font_size: 16,
      diff_font_size: 16,
    },
    recent_repos: [],
  },
  workspace: {
    schema_version: 1,
    open_tabs: [],
    active_tab: null,
    layout: DEFAULT_LAYOUT,
  },
  shortcuts: { resolved: [], overrides: {} },
  storePaths: [],
  locale: "en-US",
  t: (key: string) => key,

  repos: {},
  tabs: [],
  activeTabKey: null,
  pendingEvents: {},

  settingsOpen: false,
  overlay: { kind: "none" },
  notice: null,
  aboutOpen: false,

  async initialize(role, compareRepoId) {
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
      t: createTranslator(boot.catalogs),
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
    const tabs: TabEntry[] = boot.workspace.open_tabs.map((tab) => {
      const key = tabKey(tab.path, tab.location);
      const existing = boot.repositories.find(
        (summary) => tabKey(summary.path, summary.location) === key,
      );
      return {
        key,
        repoId: existing?.id ?? -1,
        path: tab.path,
        location: tab.location,
      };
    });

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
    for (const tab of tabs) {
      if (tab.repoId < 0) {
        await get().openTab(tab.path, tab.location);
      }
    }

    // An adopted repository has a worker this window did not start, so nothing
    // will be pushed until it asks. Without this a window that boots while a
    // repository is already open shows it blank forever. Only the repository
    // this window is about to display is requested; a background tab fills in
    // when it is selected.
    if (role === "compare" && compareRepoId !== null && repos[compareRepoId]) {
      await get().refresh(compareRepoId);
    } else if (boot.workspace.active_tab) {
      const active = tabs.find((entry) => entry.key === boot.workspace.active_tab);
      if (active && active.repoId >= 0) {
        await get().refresh(active.repoId);
      }
    }
  },

  setTranslator(locale, catalog) {
    set({ locale, t: createTranslator(catalog) });
  },

  async openTab(path, location) {
    const target: LocationConfig = location ?? { kind: "local" };
    const key = tabKey(path, target);
    const existing = get().tabs.find((tab) => tab.key === key);
    if (existing) {
      await get().selectTab(key);
      return;
    }

    // The slot is claimed before the request, not after it. Two drops of the
    // same folder, or a drop racing a menu item, would otherwise both pass the
    // check above and open the repository twice. The tab appears immediately in
    // its loading state, which is also what the user expects to see.
    set((state) => ({
      tabs: [...state.tabs, { key, repoId: -1, path, location: target }],
      activeTabKey: key,
    }));

    let summary: RepoSummary;
    try {
      summary = await ipc.openRepository(path, target);
    } catch (error) {
      const failure = ipc.describeError(error);
      set((state) => ({
        // The claim is released, because there is no repository to show.
        tabs: state.tabs.filter((tab) => tab.key !== key),
        activeTabKey:
          state.activeTabKey === key
            ? (state.tabs.find((tab) => tab.key !== key)?.key ?? null)
            : state.activeTabKey,
        notice: { level: "error", message: renderGitError(state.t, failure.key, failure.detail) },
      }));
      return;
    }
    set((state) => ({
      repos: {
        ...state.repos,
        [summary.id]: adopt(state, summary.id, summary.path, summary.location),
      },
      tabs: state.tabs.map((tab) =>
        tab.key === key ? { ...tab, repoId: summary.id } : tab,
      ),
    }));
    const pending = get().pendingEvents[summary.id];
    if (pending) {
      const { [summary.id]: _drained, ...rest } = get().pendingEvents;
      set({ pendingEvents: rest });
      for (const event of pending) {
        get().applyEvent(summary.id, event);
      }
    }
    void ipc.setWorkspaceTabs(
      get().tabs.map((tab) => ({ path: tab.path, location: tab.location })),
      key,
    );
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
    const repos = { ...state.repos };
    delete repos[tab.repoId];
    const pendingEvents = { ...state.pendingEvents };
    delete pendingEvents[tab.repoId];
    set({ tabs, activeTabKey, repos, pendingEvents });
    if (tab.repoId >= 0) {
      await ipc.closeRepository(tab.repoId);
      await ipc.closeCompareWindow(tab.repoId);
    }
    await ipc.setWorkspaceTabs(
      tabs.map((entry) => ({ path: entry.path, location: entry.location })),
      activeTabKey,
    );
    if (activeTabKey) {
      const next = get().tabs.find((entry) => entry.key === activeTabKey);
      if (next) {
        void get().refresh(next.repoId);
      }
    }
  },

  async selectTab(key) {
    set({ activeTabKey: key });
    await ipc.setWorkspaceTabs(
      get().tabs.map((tab) => ({ path: tab.path, location: tab.location })),
      key,
    );
    const tab = get().tabs.find((entry) => entry.key === key);
    if (tab) {
      void get().refresh(tab.repoId);
    }
  },

  setActiveRepo(repoId) {
    set({ compareRepoId: repoId });
  },

  applyEvent(repoId, event) {
    const state = get();
    const repo = state.repos[repoId];
    if (!repo) {
      // The repository is still being opened. Holding the event is what keeps
      // the first status from being lost to the race between the command reply
      // and the worker's first snapshot.
      const queue = state.pendingEvents[repoId] ?? [];
      set({
        pendingEvents: {
          ...state.pendingEvents,
          [repoId]: [...queue, event].slice(-PENDING_EVENT_LIMIT),
        },
      });
      return;
    }
    const t = state.t;
    const next = applyRepoEvent(repo, event, t, (key, detail) =>
      renderGitError(t, key, detail),
    );
    if (next !== repo) {
      set({ repos: { ...state.repos, [repoId]: next } });
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

  setBusy(repoId, busy) {
    const repo = get().repos[repoId];
    if (!repo) {
      return;
    }
    set({
      repos: { ...get().repos, [repoId]: { ...repo, busy, busyVerb: null } },
    });
  },

  setMessage(repoId, text, ok) {
    const repo = get().repos[repoId];
    if (!repo) {
      return;
    }
    set({ repos: { ...get().repos, [repoId]: { ...repo, message: { text, ok } } } });
  },

  async selectCommit(repoId, oid, short, subject) {
    const repo = get().repos[repoId];
    if (!repo) {
      return;
    }
    set({
      repos: {
        ...get().repos,
        [repoId]: {
          ...repo,
          selected: { oid, short, subject },
          commitFiles: [],
          commitMergeParent: null,
          // Every changed file is shown until one is chosen, matching the
          // reference application.
          pane: { kind: "commit", file: null },
          commitDiffs: {},
        },
      },
    });
    await ipc.selectCommit(repoId, oid);
  },

  clearCommit(repoId) {
    const repo = get().repos[repoId];
    if (!repo) {
      return;
    }
    set({
      repos: {
        ...get().repos,
        [repoId]: {
          ...repo,
          selected: null,
          commitFiles: [],
          commitMergeParent: null,
          pane: { kind: "none" },
          commitDiffs: {},
        },
      },
    });
  },

  async selectWorkingFile(repoId, staged, file) {
    const repo = get().repos[repoId];
    if (!repo) {
      return;
    }
    const requestId = repo.workingRequest + 1;
    set({
      repos: {
        ...get().repos,
        [repoId]: {
          ...repo,
          workingRequest: requestId,
          pane: { kind: "working", staged, file },
          workingDocument: null,
          workingLoading: true,
          workingError: null,
        },
      },
    });
    try {
      const assigned = await ipc.loadWorkingTreeDiff(repoId, staged ? "staged" : "unstaged", file);
      // The backend allocates the id, so adopt it: a later request makes any
      // answer still in flight stale.
      const current = get().repos[repoId];
      if (current) {
        set({
          repos: {
            ...get().repos,
            [repoId]: { ...current, workingRequest: Math.max(current.workingRequest, assigned) },
          },
        });
      }
    } catch (error) {
      const failure = ipc.describeError(error);
      const current = get().repos[repoId];
      if (current) {
        set({
          repos: {
            ...get().repos,
            [repoId]: {
              ...current,
              workingLoading: false,
              workingError: failure.detail,
            },
          },
        });
      }
    }
  },

  async selectCommitFile(repoId, file) {
    const repo = get().repos[repoId];
    if (!repo?.selected) {
      return;
    }
    set({
      repos: { ...get().repos, [repoId]: { ...repo, pane: { kind: "commit", file } } },
    });
    await ipc.loadCommitFileDiff(repoId, repo.selected.oid, repo.commitMergeParent, file);
  },

  async setLogScope(repoId) {
    const repo = get().repos[repoId];
    if (!repo) {
      return;
    }
    const scope = get().config.view.graph_history;
    const upstream = scope === "current-branch" ? repo.upstream : null;
    try {
      await ipc.setLogScope(repoId, upstream);
      set({
        repos: { ...get().repos, [repoId]: { ...repo, logScopeSent: true } },
      });
    } catch {
      // The repository closed; the tab is already gone.
    }
  },

  async runAction(repoId, action) {
    get().setBusy(repoId, true);
    try {
      await ipc.runAction(repoId, action);
    } catch (error) {
      const failure = ipc.describeError(error);
      get().setBusy(repoId, false);
      get().setMessage(repoId, renderGitError(get().t, failure.key, failure.detail), false);
    }
  },

  openOverlay(overlay) {
    set({ overlay });
  },

  closeOverlay() {
    set({ overlay: { kind: "none" } });
  },

  setSettingsOpen(open) {
    set({ settingsOpen: open });
  },

  notify(notice) {
    set({ notice });
  },

  async updateLayout(patch) {
    const layout: LayoutSettings = { ...get().workspace.layout, ...patch };
    set({ workspace: { ...get().workspace, layout } });
    await ipc.setLayout(layout);
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
  },
}));

/** The repository backing the active tab, if any. */
export function activeRepo(state: AppStore): RepoState | null {
  if (!state.activeTabKey) {
    return null;
  }
  const tab = state.tabs.find((entry) => entry.key === state.activeTabKey);
  if (!tab) {
    return null;
  }
  return state.repos[tab.repoId] ?? null;
}

export function tabRepo(state: AppStore, key: string): RepoState | null {
  const tab = state.tabs.find((entry) => entry.key === key);
  return tab ? (state.repos[tab.repoId] ?? null) : null;
}

/** Compare revision argument for a checkout-style ref. */
export function revisionFromTarget(target: CheckoutTarget): CompareRevisionArg {
  switch (target.kind) {
    case "localBranch":
      return { kind: "local", name: target.localBranch };
    case "remoteBranch":
      return { kind: "remote", name: target.remoteBranch };
    case "tag":
      return { kind: "tag", name: target.tag };
    case "commit":
      return { kind: "commit", name: target.commit };
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
  tabKey,
};
export type { RepoState };
