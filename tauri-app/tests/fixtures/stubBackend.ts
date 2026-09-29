import { readFileSync } from 'node:fs';

/**
 * A stub Tauri runtime for browser tests.
 *
 * The webview is ordinary web code, so the whole interface can be driven in
 * Chromium as long as `window.__TAURI_INTERNALS__` is present. This file is
 * injected before any application code runs, which means the tests exercise the
 * real components, the real store, and the real event reducers rather than
 * approximations of them.
 *
 * The fixture data is modelled on a real repository: a linear history with a
 * merge, untracked and modified files, several refs, and a diff whose
 * character-level ranges exercise inline highlighting.
 */

import { fixtureRepo, secondFixtureRepo } from './stubData';
import type { StubLogRow, StubRepo } from './stubTypes';

export type { StubFile, StubRefs, StubRepo, StubStatus } from './stubTypes';
export {
  commitFiles,
  diffPayload,
  fixtureRepo,
  longFixtureRepo,
  secondFixtureRepo
} from './stubData';

/** The stub's mutable state, so a test can steer it. */
export interface StubOptions {
  /** Repositories the bootstrap reports as already open. */
  open: StubRepo[];
  /**
   * How long the comparison's per-file diffs take to arrive, in milliseconds.
   *
   * The real worker streams them, and the progress counter only exists while
   * they are in flight, so a test that wants to see it needs them to be slow.
   */
  compareDelay?: number;
  /** Hold command replies until after early comparison events were emitted. */
  compareReplyDelay?: number;

  /**
   * Make every comparison fail as a whole with this detail.
   *
   * The real worker answers a comparison whose endpoints do not resolve with
   * one request-level error and then a finished event, so a test of the
   * failure path needs the same shape.
   */
  failCompare?: string;

  /**
   * Paths the saved workspace lists as open, with no repository behind them.
   *
   * This is the state a window starts in when the backend has no repository for
   * a tab the previous session left behind, so every restored tab has to be
   * opened by this window.
   */
  savedTabs?: string[];
  /**
   * The key the saved workspace records as its active tab.
   *
   * A launch that saved several tabs comes back on the one it was left on, which
   * is not necessarily the first one in the list.
   */
  savedActiveTab?: string;
  /** Repositories handed out by `open_repository`, in order. */
  available: StubRepo[];
  /** Fail `open_repository` with this key instead of succeeding. */
  openFailure?: { key: string; detail: string };
  /** Reject `run_action` for these action names. */
  failingActions?: string[];
  /**
   * Commands the backend refuses, with the key and detail it refuses them with.
   *
   * Separate from `failingActions`, which makes a command *run* and report a
   * failure; this one makes it refuse, which is the path that has to localize
   * the key rather than paste the detail.
   */
  refusals?: Record<string, { key: string; detail: string }>;
  /** Overrides for the merge preflight probe. */
  probeMerge?: Record<string, unknown>;
  /** Overrides for the rebase preflight probe. */
  probeRebase?: Record<string, unknown>;
  /** How long an action stays in progress, so the progress state is observable. */
  actionDelay?: number;
  /** How long `open_repository` takes, and its snapshot 1.5s after it. */
  openDelay?: number;
  /** How long the WSL distribution list takes to arrive. */
  wslDelay?: number;
  /** Initial pane geometry returned by bootstrap. */
  layout?: Partial<{
    sidebar_width: number;
    right_panel_width: number;
    diff_height: number | null;
    file_list_ratio: number;
  }>;
  /** System font families returned by the appearance settings. */
  fontFamilies?: string[];
  /** Initial persisted font preferences. */
  typography?: Partial<{
    ui_font_family: string | null;
    mono_font_family: string | null;
    ui_font_size: number;
    diff_font_size: number;
  }>;
  /** Strategy the toolbar Pull button uses. */
  pullAction?: 'merge' | 'rebase';
  diffLayout?: 'inline' | 'side-by-side';
}

export const DEFAULT_OPTIONS: StubOptions = {
  open: [],
  available: [fixtureRepo(), secondFixtureRepo()],
  failingActions: []
};

/**
 * The translation catalog the stub serves.
 *
 * Read from the real English catalog rather than restated here, so a string
 * cannot drift between the application and the tests that assert on it. A test
 * that needs a value the catalog does not define adds it through
 * {@link catalogOverrides}.
 */
export function catalog(overrides: Record<string, string> = {}): Record<string, string> {
  const file = readFileSync(
    new URL('../../src-tauri/crates/augur-core/i18n/en-US.ftl', import.meta.url),
    'utf8'
  );
  const entries: Record<string, string> = {};
  for (const line of file.split('\n')) {
    const match = /^([a-z0-9-]+) = (.*)$/.exec(line.trim());
    if (match) {
      entries[match[1]!] = match[2]!;
    }
  }
  return { ...entries, ...overrides };
}

/** The script injected into the page before the application loads. */
export function stubSource(options: StubOptions, overrides: Record<string, string> = {}): string {
  return `(${install.toString()})(${JSON.stringify(options)}, ${JSON.stringify(
    catalog(overrides)
  )});`;
}

function install(
  options: {
    open: StubRepo[];
    available: StubRepo[];
    savedTabs?: string[];
    savedActiveTab?: string;
    openFailure?: { key: string; detail: string };
    failingActions?: string[];
    probeMerge?: Record<string, unknown>;
    probeRebase?: Record<string, unknown>;
    compareDelay?: number;
    compareReplyDelay?: number;
    workingDiffFailure?: string;
    workingDiffDelay?: number;
    refusals?: Record<string, { key: string; detail: string }>;
    layout?: Partial<{
      sidebar_width: number;
      right_panel_width: number;
      diff_height: number | null;
      file_list_ratio: number;
    }>;
    fontFamilies?: string[];
    typography?: Partial<{
      ui_font_family: string | null;
      mono_font_family: string | null;
      ui_font_size: number;
      diff_font_size: number;
    }>;
    diffLayout?: 'inline' | 'side-by-side';
  },
  catalog: Record<string, string>
): void {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const subscriptions = new Map<number, { event: string; listener: (payload: unknown) => void }>();
  const unregisterListener = (event: string, id: number) => {
    const subscription = subscriptions.get(id);
    if (subscription?.event === event) {
      listeners.get(event)?.delete(subscription.listener);
      subscriptions.delete(id);
    }
  };
  Object.defineProperty(globalThis, '__TAURI_EVENT_PLUGIN_INTERNALS__', {
    configurable: true,
    value: { unregisterListener }
  });
  const log: { cmd: string; args: unknown }[] = [];
  const failure = options.openFailure ?? null;
  const failing = new Set(options.failingActions ?? []);
  const refusals = options.refusals ?? {};
  let opened = 0;
  let requestCounter = 0;
  let compareRequest = 0;

  const config = {
    schema_version: 1,
    theme: 'catppuccin-mocha',
    language: 'system',
    view: {
      show_untracked: true,
      auto_follow: true,
      diff_layout: options.diffLayout ?? 'side-by-side',
      graph_history: 'all-branches',
      auto_refresh: true,
      commit_action: 'commit',
      pull_action: options.pullAction ?? 'merge'
    },
    typography: {
      ui_font_family: null,
      mono_font_family: null,
      ui_font_size: 16,
      diff_font_size: 16,
      ...(options.typography ?? {})
    },
    recent_repos: [
      { path: '/Users/dev/projects/augur-git', location: { kind: 'local' } },
      { path: '/Users/dev/projects/other-app', location: { kind: 'local' } }
    ]
  };
  try {
    const persisted = JSON.parse(localStorage.getItem('augur-test-settings') ?? '{}');
    if (persisted.typography) Object.assign(config.typography, persisted.typography);
  } catch {
    localStorage.removeItem('augur-test-settings');
  }

  const typographyModifier = /Mac|iPhone|iPad|iPod/.test(navigator.platform) ? 'cmd' : 'ctrl';
  const defaultShortcuts = [
    { command: 'app.quit', keys: ['CmdOrCtrl+Q'] },
    { command: 'repo.pull', keys: ['p'] },
    { command: 'repo.push', keys: ['shift-p'] },
    { command: 'repo.fetch', keys: ['f'] },
    { command: 'repo.refresh', keys: ['shift-r'] },
    { command: 'commit.focus', keys: ['c'] },
    { command: 'refs.checkout', keys: ['space'] },
    { command: 'commits.checkout', keys: ['space'] },
    { command: 'changes.toggle-stage', keys: ['space'] },
    { command: 'list.next', keys: ['j', 'down'] },
    { command: 'list.previous', keys: ['k', 'up'] },
    { command: 'graph.search', keys: ['slash'] },
    { command: 'diff.font-increase', keys: [`${typographyModifier}-plus`] },
    { command: 'diff.font-decrease', keys: [`${typographyModifier}-minus`] },
    { command: 'diff.font-reset', keys: [`${typographyModifier}-0`] }
  ];
  const shortcutOverrides: Record<string, string[]> = {};
  const shortcutState = () => ({
    resolved: defaultShortcuts.map((entry) => ({
      ...entry,
      keys: shortcutOverrides[entry.command] ?? entry.keys
    })),
    defaults: defaultShortcuts,
    overrides: { ...shortcutOverrides }
  });

  const savedTabs = options.savedTabs ?? [];
  const workspace = {
    schema_version: 1,
    open_tabs: [
      ...options.open.map((repo) => ({ path: repo.path, location: repo.location })),
      ...savedTabs.map((path) => ({ path, location: { kind: 'local' } }))
    ],
    active_tab: options.savedActiveTab ?? options.open[0]?.path ?? savedTabs[0] ?? null,
    layout: {
      sidebar_width: 250,
      right_panel_width: 320,
      diff_height: null,
      file_list_ratio: 0.25,
      ...(options.layout ?? {})
    }
  };
  try {
    Object.assign(
      workspace.layout,
      JSON.parse(sessionStorage.getItem('augur-test-layout') ?? '{}')
    );
  } catch {
    sessionStorage.removeItem('augur-test-layout');
  }

  const build = {
    name: 'Augur Git Tauri',
    binary: 'augur-git-tauri',
    identifier: 'com.augur.git.tauri',
    version: '0.1.0',
    authors: 'Augur',
    commit: 'abc1234',
    version_line: '0.1.0 (abc1234)',
    platform: 'macOS aarch64'
  };

  let eventId = 0;

  /**
   * Deliver an event the way Tauri does: wrapped in an envelope with the event
   * name and a delivery id, because the JavaScript `listen` helper reads
   * `event.payload` and would otherwise see `undefined`.
   */
  function emit(name: string, payload: unknown): void {
    eventId += 1;
    const envelope = { event: name, id: eventId, payload };
    for (const handler of listeners.get(name) ?? []) {
      handler(envelope);
    }
  }

  window.addEventListener('storage', (event) => {
    if (event.key !== 'augur-test-settings' || !event.newValue) return;
    try {
      const persisted = JSON.parse(event.newValue);
      if (persisted.typography) config.typography = persisted.typography;
      emit('augur://app-event', { type: 'settingsChanged' });
    } catch {
      // The next backend read will report defaults for malformed test storage.
    }
  });

  /** Push the full status and refs snapshot a freshly opened repository has. */
  function announce(repo: StubRepo): void {
    emit('augur://repo-event', { repoId: repo.id, type: 'status', ...repo.status });
    emit('augur://repo-event', { repoId: repo.id, type: 'refs', refs: repo.refs });
    emit('augur://repo-event', {
      repoId: repo.id,
      type: 'logPage',
      rows: repo.rows,
      replace: true,
      hasMore: false
    });
  }

  /**
   * A linear lane layout, which is enough for a browser test: the frontend only
   * draws what the backend sends.
   */
  function graphLayout(rows: StubLogRow[]): {
    graph: unknown[];
    labels: Record<string, unknown[]>;
  } {
    const graph = rows.map((row, index) => {
      const lane = row.parents.length > 1 ? 1 : 0;
      return {
        input_lanes: [{ oid: row.oid, color_index: lane }],
        output_lanes: row.parents.map((parent, order) => ({
          oid: parent,
          color_index: order
        })),
        parent_lanes: row.parents.map((_, order) => order),
        node_lane: 0,
        lane_count: Math.max(1, row.parents.length),
        is_head: index === 0,
        is_merge: row.parents.length > 1,
        has_incoming: index > 0,
        node_color: lane,
        node_input_lanes: index > 0 ? [0] : []
      };
    });
    const labels: Record<string, unknown[]> = {};
    for (const row of rows) {
      const parsed: { name: string; kind: string }[] = [];
      for (const piece of row.decorations.split(',').map((part) => part.trim())) {
        if (piece.length === 0) {
          continue;
        }
        if (piece.startsWith('HEAD')) {
          parsed.push({ name: 'HEAD', kind: 'head' });
        } else if (piece.startsWith('tag:')) {
          parsed.push({ name: piece.slice(4), kind: 'tag' });
        } else if (piece.includes('/')) {
          parsed.push({ name: piece, kind: 'remoteBranch' });
        } else {
          parsed.push({ name: piece, kind: 'localBranch' });
        }
      }
      labels[row.oid] = parsed;
    }
    return { graph, labels };
  }

  /**
   * The two files a comparison reports.
   *
   * Defined here rather than shared with the fixture module because this
   * function is serialised into the page, where module scope does not exist.
   */
  function compareFiles(): Record<string, unknown>[] {
    return [
      {
        path: 'src/lib.rs',
        old_path: null,
        new_path: 'src/lib.rs',
        status: 'modified',
        old_blob: null,
        new_blob: null,
        added: 3,
        deleted: 1
      },
      {
        path: 'src/new.rs',
        old_path: null,
        new_path: 'src/new.rs',
        status: 'added',
        old_blob: null,
        new_blob: null,
        added: 40,
        deleted: 0
      }
    ];
  }

  function diffFor(path: string, language: string | null): Record<string, unknown> {
    return {
      path,
      language,
      rows: [
        {
          kind: 'hunk',
          old_no: null,
          new_no: null,
          old_text: null,
          new_text: null,
          old_line_index: null,
          new_line_index: null,
          hunk_header: '@@ -10,6 +10,7 @@ fn run()'
        },
        {
          kind: 'context',
          old_no: 10,
          new_no: 10,
          old_text: '    let mut count = 0;',
          new_text: '    let mut count = 0;',
          old_line_index: 0,
          new_line_index: 0,
          hunk_header: null
        },
        {
          kind: 'del',
          old_no: 11,
          new_no: null,
          old_text: '    count += 1;',
          new_text: null,
          old_line_index: 1,
          new_line_index: null,
          hunk_header: null
        },
        {
          kind: 'add',
          old_no: null,
          new_no: 11,
          old_text: null,
          new_text: '    count += 2;',
          old_line_index: null,
          new_line_index: 1,
          hunk_header: null
        }
      ],
      aligned_rows: [
        {
          kind: 'hunk',
          old_no: null,
          new_no: null,
          old_text: null,
          new_text: null,
          old_line_index: null,
          new_line_index: null,
          hunk_header: '@@ -10,6 +10,7 @@ fn run()'
        },
        {
          kind: 'context',
          old_no: 10,
          new_no: 10,
          old_text: '    let mut count = 0;',
          new_text: '    let mut count = 0;',
          old_line_index: 0,
          new_line_index: 0,
          hunk_header: null
        },
        {
          kind: 'del',
          old_no: 11,
          new_no: null,
          old_text: '    count += 1;',
          new_text: null,
          old_line_index: 1,
          new_line_index: null,
          hunk_header: null
        },
        {
          kind: 'add',
          old_no: null,
          new_no: 11,
          old_text: null,
          new_text: '    count += 2;',
          old_line_index: null,
          new_line_index: 1,
          hunk_header: null
        }
      ],
      old_source: null,
      new_source: null,
      inline_old: [[], [{ start: 13, end: 14 }], [], []],
      inline_new: [[], [{ start: 13, end: 14 }]],
      binary: false,
      copy_text: 'diff --git a/x b/x\\n'
    };
  }

  const handlers: Record<string, (args: any) => unknown> = {
    // Drained, not read: a second collection while one is in flight must not
    // hand the same path out twice.
    take_pending_paths: () => (options.pendingPaths ?? []).splice(0),

    bootstrap: () => ({
      window: 'main',
      config,
      workspace,
      locale: 'en-US',
      catalogs: catalog,
      shortcuts: shortcutState(),
      build,
      store_paths: [
        '~/Library/Application Support/com.augur.git.tauri/settings.json',
        '~/Library/Application Support/com.augur.git.tauri/workspace.json'
      ],
      repositories: options.open.map((repo) => ({
        id: repo.id,
        path: repo.path,
        location: repo.location
      })),
      has_pending_paths: (options.pendingPaths ?? []).length > 0
    }),

    current_config: () => config,

    open_repository: (args: any) => {
      if (failure) {
        return Promise.reject(failure);
      }
      const known = options.available.find((repo) => repo.path === args.path);
      // A slow open separates the command's reply from the first snapshot, the
      // way the real backend can, which is the window the scanning state lives
      // in.
      const adopt = (repo: StubRepo) => {
        options.open.push(repo);
        if (options.openDelay) {
          // Long enough that a test can observe the interface's scanning
          // state before the snapshot lands.
          setTimeout(() => announce(repo), 1500);
        } else {
          announce(repo);
        }
        return { id: repo.id, path: repo.path, location: repo.location };
      };
      const settle = <T>(value: T): T | Promise<T> =>
        options.openDelay
          ? new Promise<T>((resolve) => setTimeout(() => resolve(value), options.openDelay))
          : value;
      if (known) {
        return settle(adopt(known));
      }
      // An unknown path still opens, because the real backend only fails when
      // the path is not a repository.
      const repo = {
        ...(options.available[opened] ?? options.available[0]),
        path: args.path,
        location: args.location ?? { kind: 'local' }
      } as StubRepo;
      opened += 1;
      return settle(adopt(repo));
    },

    close_repository: (args: any) => {
      options.open = options.open.filter((repo) => repo.id !== args.repoId);
      return null;
    },

    repository_summary: (args: any) => options.open.find((repo) => repo.id === args.repoId) ?? null,

    refresh_repository: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      if (repo) {
        // A slow open delays every snapshot, not just the first, so a refresh
        // that races the opening cannot end the scanning state early.
        if (options.openDelay) {
          setTimeout(() => announce(repo), 1500);
        } else {
          announce(repo);
        }
      }
      return null;
    },

    set_auto_refresh_target: () => null,

    set_log_scope: () => null,
    load_more_log_page: () => null,

    select_commit: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const row = repo?.rows.find((item) => item.oid === args.oid);
      if (!repo || !row) {
        return null;
      }
      emit('augur://repo-event', {
        repoId: repo.id,
        type: 'commitFiles',
        oid: row.oid,
        files: [
          {
            path: 'src/lib.rs',
            old_path: null,
            new_path: 'src/lib.rs',
            status: 'modified',
            old_blob: null,
            new_blob: null,
            added: 4,
            deleted: 1
          },
          {
            path: 'src/commands/repo.rs',
            old_path: null,
            new_path: 'src/commands/repo.rs',
            status: 'added',
            old_blob: null,
            new_blob: null,
            added: 120,
            deleted: 0
          }
        ],
        merge_parent: row.parents[1] ?? null
      });
      return null;
    },

    request_commit_message: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const row = repo?.rows.find((item) => item.oid === args.oid);
      if (!row) {
        return null;
      }
      emit('augur://repo-event', {
        repoId: repo.id,
        type: 'commitMessage',
        oid: row.oid,
        message: {
          subject: row.subject,
          body: row.message.split('\\n\\n').slice(1).join('\\n\\n'),
          co_authors: [{ name: 'Ada', email: 'ada@example.com' }]
        }
      });
      return null;
    },

    load_commit_file_diff: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      if (!repo) {
        return null;
      }
      emit('augur://repo-event', {
        repoId: repo.id,
        type: 'fileDiff',
        oid: args.oid,
        file: args.file,
        document: diffFor(args.file.new_path, 'rust')
      });
      return null;
    },

    load_working_tree_diff: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const requestId = ++requestCounter;
      if (!repo) {
        return requestId;
      }
      const failDetail = options.workingDiffFailure ?? null;
      const delay = options.workingDiffDelay ?? 30;
      const document = diffFor(args.file.path, 'rust');
      // A short delay makes the loading state observable, which is the point of
      // testing it in a browser.
      setTimeout(
        () => {
          emit('augur://repo-event', {
            repoId: repo.id,
            type: failDetail ? 'workingTreeFileDiffError' : 'workingTreeFileDiff',
            requestId,
            kind: args.kind,
            file: args.file,
            detail: failDetail ?? '',
            document: failDetail ? undefined : document
          });
        },
        failDetail ? Math.max(delay, 400) : delay
      );
      return requestId;
    },

    working_tree_operation: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const requestId = ++requestCounter;
      if (repo) {
        setTimeout(() => {
          emit('augur://repo-event', {
            repoId: repo.id,
            type: 'workingTreeOperationFinished',
            requestId,
            action: args.action,
            scope: { kind: 'workingTree', staged: false, all: args.all },
            success: true,
            detail: ''
          });
        }, 20);
      }
      return requestId;
    },

    graph_layout: (args: any) => graphLayout(args.rows),

    // The same constants as the Rust layout, so the columns appear at the same
    // widths the real backend uses.
    column_visibility: (args: any) => {
      const hash = 60;
      const author = 140;
      const date = 120;
      const messageMin = 120;
      const gap = 8;
      const padRight = 8;
      return {
        author:
          args.totalWidth >=
          args.treeWidth + hash + author + date + 4 * gap + padRight + messageMin,
        message: args.totalWidth >= args.treeWidth + hash + date + 3 * gap + padRight + messageMin
      };
    },

    run_action: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      if (!repo) {
        return null;
      }
      const name = args.action.action;
      const label =
        {
          fetch: 'fetch --all --prune',
          pullMerge: 'pull',
          pullRebase: 'pull --rebase',
          push: 'push',
          pushForce: 'push --force',
          pushSetUpstream: 'push --set-upstream',
          pushRenameRemote: 'push --rename',
          pushDeleteRemote: 'push --delete',
          merge: 'merge',
          abortMerge: 'merge --abort',
          rebase: 'rebase',
          abortRebase: 'rebase --abort',
          checkout: 'checkout',
          createBranch: 'branch',
          copyCommitMessage: 'copy-commit-message',
          renameBranch: 'branch -m',
          deleteBranch: 'branch -d',
          deleteTag: 'tag -d',
          stash: 'stash push',
          stashPop: 'stash pop',
          stashDrop: 'stash drop',
          commit: 'commit',
          applyPatch: 'apply'
        }[name] ?? name;
      const bad = failing.has(name);
      // The clipboard copy is exercised through the clipboard plugin, which the
      // stub records; the message body is what a success would copy.
      const succeeded = name === 'copyCommitMessage' ? false : !bad;
      const duration = options.actionDelay ?? 10;
      setTimeout(() => {
        emit('augur://repo-event', {
          repoId: repo.id,
          type: 'commandStarted',
          label,
          verb: 'Working'
        });
        setTimeout(() => {
          emit('augur://repo-event', {
            repoId: repo.id,
            type: 'commandDone',
            label,
            success: succeeded,
            message: succeeded
              ? `Add the Tauri command surface\n\nWith a body.\n`
              : bad
                ? 'fatal: could not read from remote'
                : 'fatal: clipboard unavailable'
          });
        }, duration);
      }, 10);
      return null;
    },

    probe_merge: () => ({
      head: 'abc1234',
      merge_head: null,
      rebase_in_progress: false,
      has_changes: false,
      has_conflicts: false,
      already_merged: false,
      ...(options.probeMerge ?? {})
    }),

    probe_rebase: () => ({
      other_operation_in_progress: false,
      rebase_in_progress: false,
      has_changes: false,
      ...(options.probeRebase ?? {})
    }),

    start_compare: () => {
      compareRequest += 1;
      const requestId = compareRequest;
      setTimeout(() => {
        if (options.failCompare !== undefined) {
          emit('augur://repo-event', {
            repoId: 7,
            type: 'branchCompareError',
            requestId,
            detail: options.failCompare
          });
          emit('augur://repo-event', {
            repoId: 7,
            type: 'branchCompareFinished',
            requestId
          });
          return;
        }
        emit('augur://repo-event', {
          repoId: 7,
          type: 'branchCompareFiles',
          requestId,
          files: compareFiles()
        });
        // The worker asks for each file's diff and answers one event per file,
        // which is what the aggregate view collects.
        const delay = options.compareDelay ?? 0;
        compareFiles().forEach((file, index) => {
          setTimeout(
            () => {
              emit('augur://repo-event', {
                repoId: 7,
                type: 'branchCompareFileDiff',
                requestId,
                file,
                document: diffFor(file.new_path, 'rust')
              });
            },
            12 + delay + index * (delay > 0 ? delay : 5)
          );
        });
        setTimeout(
          () => {
            emit('augur://repo-event', {
              repoId: 7,
              type: 'branchCompareFinished',
              requestId
            });
          },
          22 + delay * 2
        );
      }, 10);
      return options.compareReplyDelay
        ? new Promise((resolve) => setTimeout(() => resolve(requestId), options.compareReplyDelay))
        : requestId;
    },

    cancel_compare: () => null,
    export_patch: () => ++compareRequest,

    list_font_families: () =>
      options.fontFamilies ?? ['Inter', 'Menlo', 'Fira Code', 'Source Sans 3'],
    theme_options: () => [
      'github-dark',
      'github-dark-default',
      'github-light-default',
      'catppuccin-latte',
      'catppuccin-frappe',
      'catppuccin-macchiato',
      'catppuccin-mocha',
      'dracula',
      'tokyo-night',
      'tokyo-night-storm',
      'tokyo-night-light',
      'gruvbox-dark',
      'gruvbox-light',
      'nord',
      'solarized-dark',
      'solarized-light',
      'rose-pine',
      'rose-pine-moon',
      'rose-pine-dawn',
      'ayu-dark',
      'ayu-mirage',
      'ayu-light',
      'kanagawa-wave',
      'kanagawa-lotus',
      'atom-one-dark',
      'atom-one-light',
      'everforest-dark',
      'everforest-light',
      'night-owl',
      'light-owl',
      'claude-dark',
      'claude-light'
    ],
    list_wsl_distros: () =>
      options.wslDelay
        ? new Promise((resolve) =>
            setTimeout(() => resolve(['Ubuntu', 'Debian']), options.wslDelay)
          )
        : ['Ubuntu', 'Debian'],
    probe_wsl_repository: () => null,

    set_language: (args: any) => {
      config.language = args.language;
      return null;
    },
    set_theme: (args: any) => {
      config.theme = args.theme;
      return null;
    },
    set_view: (args: any) => {
      Object.assign(config.view, args.view);
      return null;
    },
    set_typography: (args: any) => {
      Object.assign(config.typography, args.typography);
      localStorage.setItem(
        'augur-test-settings',
        JSON.stringify({ typography: config.typography })
      );
      return null;
    },
    set_commit_action: (args: any) => {
      config.view.commit_action = args.action;
      return null;
    },
    set_diff_layout: (args: any) => {
      config.view.diff_layout = args.layout;
      return null;
    },
    set_layout: (args: any) => {
      Object.assign(workspace.layout, args.layout);
      sessionStorage.setItem('augur-test-layout', JSON.stringify(workspace.layout));
      return null;
    },
    set_workspace_tabs: (args: any) => {
      workspace.open_tabs = args.tabs;
      workspace.active_tab = args.active;
      return null;
    },
    set_shortcut: (args: any) => {
      if (args.keys === null) delete shortcutOverrides[args.command];
      else shortcutOverrides[args.command] = args.keys;
      return shortcutState();
    },
    validate_shortcut: (args: any) => {
      if (typeof args.value !== 'string') {
        return Promise.reject({ key: 'err-invalid-shortcut', detail: '' });
      }
      if (!args.value.trim()) return [];
      return args.value.split(',').map((combo: string) => {
        const legacyPlusForm = combo.trim().includes('+');
        const parts = legacyPlusForm ? combo.trim().split('+') : combo.trim().split('-');
        const rawKey = parts.pop() ?? '';
        const aliases: Record<string, string> = {
          control: 'ctrl',
          command: 'cmd',
          super: 'cmd',
          cmdorctrl: 'cmdorctrl',
          option: 'alt',
          arrowup: 'up',
          arrowdown: 'down',
          arrowleft: 'left',
          arrowright: 'right',
          ' ': 'space',
          '+': 'plus',
          '=': 'plus',
          '-': 'minus',
          '/': 'slash'
        };
        const key = aliases[rawKey.toLowerCase()] ?? rawKey.toLowerCase();
        const modifiers = parts.map(
          (part: string) => aliases[part.toLowerCase()] ?? part.toLowerCase()
        );
        if (
          !legacyPlusForm &&
          rawKey.length === 1 &&
          rawKey !== rawKey.toLowerCase() &&
          !modifiers.includes('shift')
        ) {
          modifiers.push('shift');
        }
        const order: Record<string, number> = {
          ctrl: 0,
          cmdorctrl: 0,
          alt: 1,
          shift: 2,
          cmd: 3,
          meta: 4
        };
        modifiers.sort((left: string, right: string) => (order[left] ?? 5) - (order[right] ?? 5));
        return [...modifiers, key].join('-');
      });
    },
    flush_state: () => null,
    open_about_window: () => null,
    open_settings_window: () => null,
    open_compare_window: () => 'compare-1',
    close_compare_window: () => null,
    focus_main_window: () => null,
    request_open_paths: (args: any) => {
      emit('augur://open-paths', { paths: args.paths });
      return null;
    }
  };

  // The plugin commands the interface reaches through the official JavaScript
  // packages. They are recorded rather than implemented, except for the dialog
  // picker, which returns the first available fixture.
  const pluginHandlers: Record<string, (args: any) => unknown> = {
    'plugin:event|listen': (args: any) => {
      const set = listeners.get(args.event) ?? new Set();
      listeners.set(args.event, set);
      // `handler` is the identifier `transformCallback` allocated, which is the
      // name the property is defined under on the global object.
      const id = args.handler;
      const listener = (payload: unknown) => {
        (globalThis as any)[`_${id}`]?.(payload);
      };
      subscriptions.set(id, { event: args.event, listener });
      set.add(listener);
      return Promise.resolve(id);
    },
    'plugin:event|unlisten': (args: any) => {
      unregisterListener(args.event, args.eventId);
      return null;
    },
    'plugin:event|emit': (args: any) => {
      emit(args.event, args.payload);
      return null;
    },
    'plugin:dialog|open': () => {
      const next = options.available.find(
        (repo) => !options.open.some((open) => open.path === repo.path)
      );
      return next?.path ?? options.available[0]?.path ?? null;
    },
    'plugin:dialog|save': () => '/tmp/compare.patch',
    'plugin:clipboard-manager|write_text': () => null,
    'plugin:clipboard-manager|read_text': () => '',
    'plugin:window|show': () => null,
    'plugin:window|destroy': () => null,
    'plugin:window|start_dragging': () => null,
    'plugin:window|toggle_maximize': () => null,
    'plugin:window|is_maximized': () => false,
    'plugin:opener|open_path': () => null
  };

  let callbackId = 1;
  const callbacks = new Map<number, (payload: unknown) => void>();

  const internals = {
    metadata: { currentWindow: { label: options.window ?? 'main' } },
    transformCallback(callback: (payload: unknown) => void, once = false): number {
      const id = callbackId;
      callbackId += 1;
      if (once) {
        callbacks.set(id, (payload) => {
          callbacks.delete(id);
          callback(payload);
        });
      } else {
        callbacks.set(id, callback);
      }
      Object.defineProperty(globalThis, `_${id}`, {
        configurable: true,
        value: (payload: unknown) => callbacks.get(id)?.(payload)
      });
      return id;
    },
    async invoke(cmd: string, args: any = {}): Promise<unknown> {
      log.push({ cmd, args });
      // A refusal configured for this command, so a test can assert on how the
      // interface renders a keyed failure rather than only that a bare one
      // arrives. Resolved before the handlers, because the point of it is that
      // the command never runs.
      const refused = refusals[cmd];
      if (refused) {
        return Promise.reject(refused);
      }
      if (cmd in handlers) {
        return handlers[cmd]!(args);
      }
      if (cmd in pluginHandlers) {
        return pluginHandlers[cmd]!(args);
      }
      // An unimplemented command is a real defect in a test: surface it rather
      // than resolving to undefined and failing somewhere else.
      return Promise.reject({ key: 'err-unknown', detail: cmd });
    }
  };

  Object.defineProperty(globalThis, '__TAURI_INTERNALS__', {
    configurable: true,
    value: internals
  });

  // Exposed so a test can steer the stub and assert what the interface asked
  // for, which is how the command-label protocol and the open failure path are
  // covered.
  Object.defineProperty(globalThis, '__STUB__', {
    configurable: true,
    value: {
      log,
      emit,
      options,
      config,
      workspace,
      announce: (repoId: number) => {
        const repo = options.open.find((item) => item.id === repoId);
        if (repo) {
          announce(repo);
        }
      }
    }
  });
}
