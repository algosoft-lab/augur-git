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

/** One repository the stub backend pretends to have open. */
export interface StubRepo {
  id: number;
  path: string;
  location: { kind: "local" } | { kind: "wsl"; distro: string };
  status: {
    branch: string;
    head: string | null;
    upstream: string | null;
    ahead: number;
    behind: number;
    files: StubFile[];
    branches: { name: string; is_head: boolean }[];
  };
  refs: StubRefs;
  rows: StubLogRow[];
}

export interface StubFile {
  index: string;
  worktree: string;
  path: string;
  old_path: string | null;
}

export interface StubRefs {
  remotes: string[];
  remote_branches: string[];
  tags: string[];
  stashes: { reference: string; description: string }[];
  comparison_revisions: {
    name: string;
    full_name: string;
    kind: "local" | "remote" | "tag" | "commit";
  }[];
}

export interface StubLogRow {
  oid: string;
  short: string;
  author: string;
  date: string;
  timestamp: number;
  subject: string;
  message: string;
  decorations: string;
  parents: string[];
}

/** A distinct, deterministic object id for a fixture commit. */
const oid = (seed: number) => {
  const head = (0x9e3779b1 * (seed + 1)).toString(16).padStart(8, "0");
  return (head + seed.toString(16).padStart(4, "0")).padEnd(40, "0").slice(0, 40);
};

function logRow(
  seed: number,
  subject: string,
  author: string,
  minutesAgo: number,
  parents: number[],
  decorations: string,
  body = "",
): StubLogRow {
  return {
    oid: oid(seed),
    short: oid(seed).slice(0, 7),
    author,
    date: "2026-09-27 10:00",
    timestamp: Math.floor(Date.now() / 1000) - minutesAgo * 60,
    subject,
    message: body ? `${subject}\n\n${body}` : subject,
    decorations,
    parents: parents.map(oid),
  };
}

/**
 * The fixture repository.
 *
 * The topology is a trunk with one side branch merged back in, so the graph
 * layout has a lane change, a merge node, and three parents to lay out.
 */
export function fixtureRepo(): StubRepo {
  return {
    id: 7,
    path: "/Users/dev/projects/augur-git",
    location: { kind: "local" },
    status: {
      branch: "master",
      head: oid(1),
      upstream: "origin/master",
      ahead: 2,
      behind: 1,
      files: [
        { index: "M", worktree: " ", path: "src/main.rs", old_path: null },
        { index: "A", worktree: " ", path: "src/git/worker.rs", old_path: null },
        { index: " ", worktree: "M", path: "src/git/graph.rs", old_path: null },
        { index: " ", worktree: "?", path: "notes.md", old_path: null },
        { index: "R", worktree: "R", path: "src/old.rs", old_path: "src/new.rs" },
        {
          index: "U",
          worktree: "U",
          path: "src/conflict.rs",
          old_path: null,
        },
      ],
      branches: [
        { name: "master", is_head: true },
        { name: "feature/tauri", is_head: false },
        { name: "release", is_head: false },
      ],
    },
    refs: {
      remotes: ["origin"],
      remote_branches: ["origin/master", "origin/feature/tauri"],
      tags: ["v1.0.0", "v1.1.0"],
      stashes: [{ reference: "stash@{0}", description: "WIP on master: 9ab1c2d" }],
      comparison_revisions: [
        { name: "master", full_name: "refs/heads/master", kind: "local" },
        {
          name: "feature/tauri",
          full_name: "refs/heads/feature/tauri",
          kind: "local",
        },
        { name: "origin/master", full_name: "refs/remotes/origin/master", kind: "remote" },
        { name: "v1.1.0", full_name: "refs/tags/v1.1.0", kind: "tag" },
      ],
    },
    rows: [
      logRow(1, "Add the Tauri command surface", "Lihao", 12, [2, 3], "HEAD -> master, origin/master, tag: v1.1.0"),
      logRow(4, "Extract the domain crate", "Lihao", 90, [5], "feature/tauri"),
      logRow(2, "Add the event protocol", "Ada", 240, [6], ""),
      logRow(3, "Widen the log page size", "Ada", 300, [6], ""),
      logRow(5, "Start the core extraction", "Lihao", 1500, [7], ""),
      logRow(6, "Introduce the snapshot store", "Grace", 2600, [8], ""),
      logRow(7, "Bootstrap the project", "Grace", 5000, [8], ""),
      logRow(8, "First commit", "Grace", 9000, [], ""),
    ],
  };
}

/** A second repository, used to prove the tab list keeps state per tab. */
export function secondFixtureRepo(): StubRepo {
  const repo = fixtureRepo();
  return {
    ...repo,
    id: 9,
    path: "/Users/dev/projects/other-app",
    status: {
      ...repo.status,
      branch: "trunk",
      head: oid(11),
      upstream: null,
      ahead: 0,
      behind: 0,
      files: [{ index: " ", worktree: "M", path: "README.md", old_path: null }],
      branches: [{ name: "trunk", is_head: true }],
    },
    rows: [logRow(11, "Update the readme", "Ada", 30, [12], "HEAD -> trunk")],
  };
}

/** The file list of the newest commit, so the commit panel has content. */
export function commitFiles(): Record<string, unknown>[] {
  return [
    {
      path: "src/lib.rs",
      old_path: null,
      new_path: "src/lib.rs",
      status: "modified",
      old_blob: null,
      new_blob: null,
      added: 4,
      deleted: 1,
    },
    {
      path: "src/commands/repo.rs",
      old_path: null,
      new_path: "src/commands/repo.rs",
      status: "added",
      old_blob: null,
      new_blob: null,
      added: 120,
      deleted: 0,
    },
  ];
}

/** A diff payload with a hunk, an addition, a deletion, and an inline range. */
export function diffPayload(path: string, language: string | null): Record<string, unknown> {
  const rows = [
    {
      kind: "hunk",
      old_no: null,
      new_no: null,
      old_text: null,
      new_text: null,
      hunk_header: "@@ -10,6 +10,7 @@ fn run()",
    },
    {
      kind: "context",
      old_no: 10,
      new_no: 10,
      old_text: "    let mut count = 0;",
      new_text: "    let mut count = 0;",
    },
    {
      kind: "del",
      old_no: 11,
      new_no: null,
      old_text: "    count += 1;",
      new_text: null,
    },
    {
      kind: "add",
      old_no: null,
      new_no: 11,
      old_text: null,
      new_text: "    count += 2;",
    },
  ];
  return {
    path,
    language,
    rows,
    aligned_rows: rows,
    old_source: null,
    new_source: null,
    // The changed character is the `1` becoming `2`, which is the point of the
    // inline layout.
    inline_old: [[], [], [], []],
    inline_new: [[], [{ start: 13, end: 14 }]],
    binary: false,
    copy_text: `diff --git a/${path} b/${path}\n@@ -10,6 +10,7 @@\n-    count += 1;\n+    count += 2;\n`,
  };
}

/** The translation catalog the stub serves, covering every key the tests read. */
export function catalog(): Record<string, string> {
  return {
    "app-name": "Augur Git Tauri",
    "app-tagline": "Desktop Git client",
    "welcome-open": "Open",
    "welcome-open-wsl": "Open from WSL",
    "welcome-drop-hint": "or drop a repository folder here",
    "recent-repos": "Recent repositories",
    "no-repo-open": "No repository open",
    "status-scanning": "Scanning…",
    "command-success": "{ $label } finished",
    "command-failed": "{ $label } failed: { $error }",
    "tab-new": "New tab",
    "tab-close": "Close tab",
    "notice-dismiss": "Dismiss",
    "menu-open": "Main menu",
    "menu-edit": "Edit",
    "menu-help": "Help",
    "menu-open-repository": "Open repository…",
    "menu-open-wsl-repository": "Open WSL repository…",
    "menu-new-tab": "New tab",
    "menu-settings": "Settings",
    "menu-about": "About",
    "menu-quit": "Quit",
    "menu-install-cli": "Install shell command",
    "menu-remove-cli": "Remove shell command",
    "menu-recent-repositories": "Recent repositories",
    "menu-no-recent-repositories": "No recent repositories",
    "toolbar-branch": "Branch",
    "toolbar-fetch": "Fetch",
    "toolbar-pull-merge": "Pull",
    "toolbar-pull-rebase": "Rebase pull",
    "toolbar-push": "Push",
    "toolbar-push-force": "Force push",
    "toolbar-compare": "Compare",
    "toolbar-refresh": "Refresh",
    "toolbar-settings": "Settings",
    "menu-branch-new": "New branch…",
    "menu-branch-rename": "Rename branch…",
    "menu-stash": "Stash changes…",
    "menu-stash-pop": "Pop stash",
    "menu-stash-drop": "Drop stash",
    "menu-merge": "Merge into current…",
    "menu-merge-no-ff": "Merge (no fast forward)…",
    "menu-rebase": "Rebase onto…",
    "menu-apply-patch": "Apply patch…",
    "sidebar-repo": "Repository sidebar",
    "section-branches": "Branches",
    "section-remote-branches": "Remote branches",
    "section-tags": "Tags",
    "section-stashes": "Stashes",
    "section-staged": "Staged",
    "section-changes": "Changes",
    "context-checkout": "Checkout",
    "context-copy-branch": "Copy branch name",
    "context-copy-tag": "Copy tag name",
    "context-copy-commit": "Copy commit id",
    "context-show-commit-message": "Show full message",
    "context-rename": "Rename…",
    "context-delete": "Delete…",
    "context-merge-into-current": "Merge into current…",
    "context-copied": "Copied",
    "commit-search-placeholder": "Search commits",
    "commit-search-subject": "Subject",
    "commit-search-full-message": "Full message",
    "commit-search-results": "{ $matches } of { $total }",
    "commit-search-no-results": "No matching commit",
    "graph-empty": "No commits yet",
    "commit-message-dialog-title": "Commit message",
    "commit-message-loading": "Loading…",
    "commit-message-preview": "Commit message preview",
    "commit-coauthors": "Co-authors",
    "commit-title": "Commit",
    "commit-placeholder": "Commit message",
    "commit-btn": "Commit",
    "commit-amend-btn": "Amend",
    "commit-action-commit": "Commit",
    "commit-action-amend": "Amend last",
    "changes-title": "Changes",
    "changes-empty": "No local changes",
    "changes-stage": "Stage",
    "changes-unstage": "Unstage",
    "changes-discard": "Discard",
    "changes-stage-all": "Stage all",
    "changes-unstage-all": "Unstage all",
    "changes-discard-all": "Discard all",
    "changes-action-conflict": "Resolve the conflicts first",
    "status-add": "A",
    "status-del": "D",
    "status-mod": "M",
    "status-ren": "R",
    "status-cpy": "C",
    "status-conflict": "U",
    "status-unknown": "?",
    "bottom-no-commit": "Select a commit",
    "bottom-no-changes": "This commit changed no files",
    "bottom-no-file": "Select a file",
    "bottom-bin": "Binary file",
    "diff-working-tree-staged": "Staged changes",
    "diff-working-tree-changes": "Working tree changes",
    "diff-no-output": "(no output)",
    "diff-copy-tooltip": "Copy the diff",
    "diff-all-files": "All changed files",
    "diff-merge-first-parent": "vs first parent",
    "bottom-merge-empty": "Merge commit has no changes relative to its first parent",
    "dialog-cancel": "Cancel",
    "dialog-confirm": "Confirm",
    "branch-new-title": "New branch",
    "branch-rename-title": "Rename branch",
    "branch-name-label": "Branch name",
    "branch-name-invalid": "That is not a valid branch name",
    "branch-name-exists": "{ $name } already exists",
    "branch-new-hint": "Created from { $branch }",
    "branch-rename-hint": "Renaming { $branch }",
    "rename-remote-branch-title": "Rename remote branch",
    "rename-remote-branch-hint": "{ $remote } / { $branch }",
    "merge-title": "Merge into { $branch }",
    "merge-source-label": "Source",
    "merge-no-ff-label": "Create a merge commit even when fast-forward is possible",
    "merge-already-up-to-date": "Already up to date",
    "merge-conflict-title": "Merge conflicts",
    "merge-conflict-warning": "{ $source } did not merge cleanly.",
    "merge-abort": "Abort the merge",
    "rebase-title": "Rebase { $branch }",
    "rebase-warning": "Rebasing { $branch } rewrites its history.",
    "rebase-conflict-title": "Rebase stopped on conflicts",
    "rebase-conflict-warning": "The rebase paused. Resolve the conflicts, then continue.",
    "rebase-abort": "Abort the rebase",
    "rebase-error-close": "Close",
    "rebase-preflight-operation-in-progress": "Another Git operation is already in progress",
    "rebase-preflight-dirty": "Commit or stash the working tree changes first",
    "stash-title": "Stash changes",
    "stash-message-label": "Message",
    "stash-hint": "{ $count } files will be stashed",
    "stash-drop-title": "Drop stash",
    "stash-drop-warning": "{ $reference } will be removed. This cannot be undone.",
    "delete-branch-title": "Delete branch",
    "delete-tag-title": "Delete tag",
    "delete-branch-warning": "{ $name } will be deleted from this repository.",
    "delete-tag-warning": "{ $name } will be deleted from this repository.",
    "delete-force-label": "Force delete even if not fully merged",
    "delete-remote-branch-title": "Delete remote branch",
    "delete-remote-branch-warning": "{ $remote }/{ $branch } will be deleted on the remote.",
    "push-force-title": "Force push",
    "push-force-warning": "Force pushing replaces the remote history. Commits on the remote that are not on this branch are lost.",
    "push-force-cancel": "Cancel",
    "push-force-confirm": "Force push",
    "push-upstream-title": "Publish branch",
    "push-upstream-warning": "{ $branch } has no upstream. Publishing sets it to { $remote }.",
    "push-upstream-cancel": "Cancel",
    "push-upstream-confirm": "Publish",
    "discard-title": "Discard changes",
    "discard-file-warning": "This file will be restored from HEAD.",
    "discard-all-warning": "These files will be restored from HEAD. Uncommitted work is lost.",
    "discard-untracked-file-warning": "This untracked file will be deleted. It cannot be recovered.",
    "discard-cancel": "Cancel",
    "discard-confirm": "Discard",
    "cli-dialog-title": "Shell command",
    "cli-install-updated": "Updated",
    "cli-install-unchanged": "Already current",
    "cli-install-failed": "Failed",
    "cli-remove-notinstalled": "Not installed",
    "cli-install-none": "No shell profile was changed",
    "cli-remove-none": "Nothing to remove",
    "cli-install-hint": "Restart your shell to pick up the change.",
    "cli-binary-fallback": "The real binary could not be resolved; the path was written instead.",
    "wsl-open-title": "Open a WSL repository",
    "wsl-distro-label": "Distribution",
    "wsl-path-label": "Path",
    "wsl-no-distros": "No distribution found",
    "wsl-refresh-distros": "Refresh",
    "wsl-path-hint": "The Linux path must be absolute, for example /home/dev/repo",
    "wsl-path-not-absolute": "The path must start with /",
    "wsl-checking": "Checking…",
    "wsl-check-ok": "A Git repository",
    "wsl-open-confirm": "Open",
    "settings-title": "Settings",
    "settings-close": "Close",
    "settings-general": "General",
    "settings-appearance": "Appearance",
    "settings-layout": "Layout",
    "settings-shortcuts": "Shortcuts",
    "settings-description": "Preferences are saved as you change them.",
    "language-title": "Language",
    "language-system": "System default",
    "language-chinese": "简体中文",
    "language-english": "English",
    "auto-refresh-on-focus-title": "Refresh on window focus",
    "setting-enabled": "Enabled",
    "setting-disabled": "Disabled",
    "settings-store-location": "Stored in",
    "theme-title": "Theme",
    "theme-github-dark": "GitHub Dark",
    "theme-catppuccin-latte": "Catppuccin Latte",
    "theme-catppuccin-frappe": "Catppuccin Frappé",
    "theme-catppuccin-macchiato": "Catppuccin Macchiato",
    "theme-catppuccin-mocha": "Catppuccin Mocha",
    "font-system-default": "System default",
    "ui-font-title": "Interface font",
    "mono-font-title": "Monospace font",
    "ui-font-size-title": "Interface text size",
    "ui-font-size-description": "Applies to labels, menus, and the status bar.",
    "diff-font-size-title": "Diff text size",
    "diff-font-size-description": "Applies to the diff viewer only.",
    "diff-layout-title": "Diff layout",
    "diff-layout-side-by-side": "Side by side",
    "diff-layout-inline": "Inline",
    "graph-history-title": "History shown",
    "graph-history-all": "All branches",
    "graph-history-current": "Current branch only",
    "graph-history-description": "The current-branch scope limits the graph to the checked-out branch and its upstream.",
    "layout-persistence-description": "Pane sizes are saved when you drag them.",
    "shortcut-edit-description": "Enter a combination such as CmdOrCtrl+Shift+Q, then press Enter.",
    "shortcut-app-quit": "Quit",
    "shortcut-reset": "Reset",
    "shortcut-invalid-combo": "That is not a valid combination",
    "shortcut-app-quit-reset": "Reset to default",
    "about-title": "About",
    "about-tagline": "Desktop Git client",
    "about-author": "Author",
    "about-version": "Version",
    "about-commit": "Commit",
    "app-identifier": "Identifier",
    "cli-command": "Command",
    "app-data-dir": "Data location",
    "about-platform": "Platform",
    "about-cli-hint": "Run { $command } in a terminal to open the current directory.",
    "compare-window-title": "Compare revisions",
    "branch-compare-base": "Base",
    "branch-compare-target": "Compare",
    "branch-compare-run": "Compare",
    "branch-compare-export-patch": "Export patch",
    "branch-compare-export-saving": "Writing the patch…",
    "branch-compare-export-success": "Patch written to { $path }",
    "branch-compare-export-error": "Could not write the patch: { $error }",
    "branch-compare-all-files": "Show all files",
    "branch-compare-loading": "Comparing…",
    "branch-compare-select-hint": "Choose two revisions to compare",
    "branch-compare-select-file": "Choose a file to see its diff",
    "branch-compare-no-changes": "The revisions are identical",
    "branch-compare-revision-placeholder": "Branch, tag, or commit",
    "branch-compare-no-matches": "No match",
    "branch-compare-branches": "Branches",
    "branch-compare-remote": "Remote",
    "branch-compare-tags": "Tags",
    "branch-compare-commits": "Commits",
    "branch-compare-use-commit": "Use commit",
    "branch-compare-invalid-revision": "That is not a branch, tag, or commit",
    "rel-now": "just now",
    "rel-min": "{ $n }m",
    "rel-hour": "{ $n }h",
    "rel-day": "{ $n }d",
    "rel-week": "{ $n }w",
    "rel-month": "{ $n }mo",
    "rel-year": "{ $n }y",
    "branch-selected": "Branch: { $name }",
    "err-unknown": "An unexpected error occurred.",
    "err-repo-closed": "This repository tab is no longer open.",
    "err-no-files": "Select at least one file first.",
    "err-git-run": "Failed to run git: { $detail }",
    "err-not-a-repo": "Not a Git repository: { $detail }",
    "err-path-not-exist": "Path not found: { $detail }",
    "err-wsl-unsupported": "WSL repositories require Windows: { $detail }",
    "patch-apply-success": "Patch applied",
    "patch-apply-failed": "Could not apply the patch",
  };
}

/** The stub's mutable state, so a test can steer it. */
export interface StubOptions {
  /** Repositories the bootstrap reports as already open. */
  open: StubRepo[];
  /** Repositories handed out by `open_repository`, in order. */
  available: StubRepo[];
  /** Fail `open_repository` with this key instead of succeeding. */
  openFailure?: { key: string; detail: string };
  /** Reject `run_action` for these action names. */
  failingActions?: string[];
  /** Overrides for the merge preflight probe. */
  probeMerge?: Record<string, unknown>;
  /** Overrides for the rebase preflight probe. */
  probeRebase?: Record<string, unknown>;
}

export const DEFAULT_OPTIONS: StubOptions = {
  open: [],
  available: [fixtureRepo(), secondFixtureRepo()],
  failingActions: [],
};

/** The script injected into the page before the application loads. */
export function stubSource(options: StubOptions): string {
  return `(${install.toString()})(${JSON.stringify(options)}, ${JSON.stringify(
    catalog(),
  )});`;
}

function install(
  options: {
    open: StubRepo[];
    available: StubRepo[];
    openFailure?: { key: string; detail: string };
    failingActions?: string[];
    probeMerge?: Record<string, unknown>;
    probeRebase?: Record<string, unknown>;
  },
  catalog: Record<string, string>,
): void {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const log: { cmd: string; args: unknown }[] = [];
  const failure = options.openFailure ?? null;
  const failing = new Set(options.failingActions ?? []);
  let opened = 0;
  let requestCounter = 0;
  let compareRequest = 0;

  const config = {
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
    recent_repos: [
      { path: "/Users/dev/projects/augur-git", location: { kind: "local" } },
      { path: "/Users/dev/projects/other-app", location: { kind: "local" } },
    ],
  };

  const workspace = {
    schema_version: 1,
    open_tabs: options.open.map((repo) => ({
      path: repo.path,
      location: repo.location,
    })),
    active_tab: options.open[0]?.path ?? null,
    layout: {
      sidebar_width: 250,
      right_panel_width: 320,
      diff_height: null,
      file_list_ratio: 0.25,
    },
  };

  const build = {
    name: "Augur Git Tauri",
    binary: "augur-git-tauri",
    cli_command: "augurgit-tauri",
    identifier: "com.augur.git.tauri",
    version: "0.1.0",
    authors: "Augur",
    commit: "abc1234",
    version_line: "0.1.0 (abc1234)",
    platform: "macOS aarch64",
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

  /** Push the full status and refs snapshot a freshly opened repository has. */
  function announce(repo: StubRepo): void {
    emit("augur://repo-event", { repoId: repo.id, type: "status", ...repo.status });
    emit("augur://repo-event", { repoId: repo.id, type: "refs", refs: repo.refs });
    emit("augur://repo-event", {
      repoId: repo.id,
      type: "logPage",
      rows: repo.rows,
      replace: true,
      hasMore: false,
    });
  }

  /**
   * A linear lane layout, which is enough for a browser test: the frontend only
   * draws what the backend sends.
   */
  function graphLayout(rows: StubLogRow[]): { graph: unknown[]; labels: Record<string, unknown[]> } {
    const graph = rows.map((row, index) => {
      const lane = row.parents.length > 1 ? 1 : 0;
      return {
        input_lanes: [{ oid: row.oid, color_index: lane }],
        output_lanes: row.parents.map((parent, order) => ({
          oid: parent,
          color_index: order,
        })),
        parent_lanes: row.parents.map((_, order) => order),
        node_lane: 0,
        lane_count: Math.max(1, row.parents.length),
        is_head: index === 0,
        is_merge: row.parents.length > 1,
        has_incoming: index > 0,
        node_color: lane,
        node_input_lanes: index > 0 ? [0] : [],
      };
    });
    const labels: Record<string, unknown[]> = {};
    for (const row of rows) {
      const parsed: { name: string; kind: string }[] = [];
      for (const piece of row.decorations.split(",").map((part) => part.trim())) {
        if (piece.length === 0) {
          continue;
        }
        if (piece.startsWith("HEAD")) {
          parsed.push({ name: "HEAD", kind: "head" });
        } else if (piece.startsWith("tag:")) {
          parsed.push({ name: piece.slice(4), kind: "tag" });
        } else if (piece.includes("/")) {
          parsed.push({ name: piece, kind: "remoteBranch" });
        } else {
          parsed.push({ name: piece, kind: "localBranch" });
        }
      }
      labels[row.oid] = parsed;
    }
    return { graph, labels };
  }

  function diffFor(path: string, language: string | null): Record<string, unknown> {
    return {
      path,
      language,
      rows: [
        {
          kind: "hunk",
          old_no: null,
          new_no: null,
          old_text: null,
          new_text: null,
          old_line_index: null,
          new_line_index: null,
          hunk_header: "@@ -10,6 +10,7 @@ fn run()",
        },
        {
          kind: "context",
          old_no: 10,
          new_no: 10,
          old_text: "    let mut count = 0;",
          new_text: "    let mut count = 0;",
          old_line_index: 0,
          new_line_index: 0,
          hunk_header: null,
        },
        {
          kind: "del",
          old_no: 11,
          new_no: null,
          old_text: "    count += 1;",
          new_text: null,
          old_line_index: 1,
          new_line_index: null,
          hunk_header: null,
        },
        {
          kind: "add",
          old_no: null,
          new_no: 11,
          old_text: null,
          new_text: "    count += 2;",
          old_line_index: null,
          new_line_index: 1,
          hunk_header: null,
        },
      ],
      aligned_rows: [
        {
          kind: "hunk",
          old_no: null,
          new_no: null,
          old_text: null,
          new_text: null,
          old_line_index: null,
          new_line_index: null,
          hunk_header: "@@ -10,6 +10,7 @@ fn run()",
        },
        {
          kind: "context",
          old_no: 10,
          new_no: 10,
          old_text: "    let mut count = 0;",
          new_text: "    let mut count = 0;",
          old_line_index: 0,
          new_line_index: 0,
          hunk_header: null,
        },
        {
          kind: "del",
          old_no: 11,
          new_no: null,
          old_text: "    count += 1;",
          new_text: null,
          old_line_index: 1,
          new_line_index: null,
          hunk_header: null,
        },
        {
          kind: "add",
          old_no: null,
          new_no: 11,
          old_text: null,
          new_text: "    count += 2;",
          old_line_index: null,
          new_line_index: 1,
          hunk_header: null,
        },
      ],
      old_source: null,
      new_source: null,
      inline_old: [[], [], [], []],
      inline_new: [[], [{ start: 13, end: 14 }]],
      binary: false,
      copy_text: "diff --git a/x b/x\\n",
    };
  }

  const handlers: Record<string, (args: any) => unknown> = {
    bootstrap: () => ({
      window: "main",
      config,
      workspace,
      locale: "en-US",
      catalogs: catalog,
      shortcuts: {
        resolved: [{ command: "app.quit", keys: ["CmdOrCtrl+Q"] }],
        overrides: {},
      },
      build,
      store_paths: [
        "~/Library/Application Support/com.augur.git.tauri/settings.json",
        "~/Library/Application Support/com.augur.git.tauri/workspace.json",
      ],
      repositories: options.open.map((repo) => ({
        id: repo.id,
        path: repo.path,
        location: repo.location,
      })),
      has_pending_paths: false,
    }),

    current_config: () => config,

    open_repository: (args: any) => {
      if (failure) {
        return Promise.reject(failure);
      }
      const repo = options.available[opened] ?? options.available[0];
      opened += 1;
      options.open.push(repo);
      announce(repo);
      return {
        id: repo.id,
        path: repo.path,
        location: repo.location,
      };
    },

    close_repository: (args: any) => {
      options.open = options.open.filter((repo) => repo.id !== args.repoId);
      return null;
    },

    repository_summary: (args: any) =>
      options.open.find((repo) => repo.id === args.repoId) ?? null,

    refresh_repository: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      if (repo) {
        announce(repo);
      }
      return null;
    },

    set_log_scope: () => null,
    load_more_log_page: () => null,

    select_commit: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const row = repo?.rows.find((item) => item.oid === args.oid);
      if (!repo || !row) {
        return null;
      }
      emit("augur://repo-event", {
        repoId: repo.id,
        type: "commitFiles",
        oid: row.oid,
        files: [
          {
            path: "src/lib.rs",
            old_path: null,
            new_path: "src/lib.rs",
            status: "modified",
            old_blob: null,
            new_blob: null,
            added: 4,
            deleted: 1,
          },
          {
            path: "src/commands/repo.rs",
            old_path: null,
            new_path: "src/commands/repo.rs",
            status: "added",
            old_blob: null,
            new_blob: null,
            added: 120,
            deleted: 0,
          },
        ],
        merge_parent: row.parents[1] ?? null,
      });
      return null;
    },

    request_commit_message: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const row = repo?.rows.find((item) => item.oid === args.oid);
      if (!row) {
        return null;
      }
      emit("augur://repo-event", {
        repoId: repo.id,
        type: "commitMessage",
        oid: row.oid,
        message: {
          subject: row.subject,
          body: row.message.split("\\n\\n").slice(1).join("\\n\\n"),
          co_authors: [{ name: "Ada", email: "ada@example.com" }],
        },
      });
      return null;
    },

    load_commit_file_diff: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      if (!repo) {
        return null;
      }
      emit("augur://repo-event", {
        repoId: repo.id,
        type: "fileDiff",
        oid: args.oid,
        file: args.file,
        document: diffFor(args.file.new_path, "rust"),
      });
      return null;
    },

    load_working_tree_diff: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const requestId = ++requestCounter;
      if (!repo) {
        return requestId;
      }
      const document = diffFor(args.file.path, "rust");
      // A short delay makes the loading state observable, which is the point of
      // testing it in a browser.
      setTimeout(() => {
        emit("augur://repo-event", {
          repoId: repo.id,
          type: "workingTreeFileDiff",
          requestId,
          kind: args.kind,
          file: args.file,
          document,
        });
      }, 30);
      return requestId;
    },

    working_tree_operation: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      const requestId = ++requestCounter;
      if (repo) {
        setTimeout(() => {
          emit("augur://repo-event", {
            repoId: repo.id,
            type: "workingTreeOperationFinished",
            requestId,
            action: args.action,
            scope: { kind: "workingTree", staged: false, all: args.all },
            success: true,
            detail: "",
          });
        }, 20);
      }
      return requestId;
    },

    graph_layout: (args: any) => graphLayout(args.rows),

    run_action: (args: any) => {
      const repo = options.open.find((item) => item.id === args.repoId);
      if (!repo) {
        return null;
      }
      const name = args.action.action;
      const label =
        {
          fetch: "fetch --all --prune",
          pullMerge: "pull",
          pullRebase: "pull --rebase",
          push: "push",
          pushForce: "push --force",
          pushSetUpstream: "push --set-upstream",
          pushRenameRemote: "push --rename",
          pushDeleteRemote: "push --delete",
          merge: "merge",
          abortMerge: "merge --abort",
          rebase: "rebase",
          abortRebase: "rebase --abort",
          checkout: "checkout",
          createBranch: "branch",
          renameBranch: "branch -m",
          deleteBranch: "branch -d",
          deleteTag: "tag -d",
          stash: "stash push",
          stashPop: "stash pop",
          stashDrop: "stash drop",
          commit: "commit",
          applyPatch: "apply",
        }[name] ?? name;
      const bad = failing.has(name);
      setTimeout(() => {
        emit("augur://repo-event", {
          repoId: repo.id,
          type: "commandStarted",
          label,
          verb: "Working",
        });
        setTimeout(() => {
          emit("augur://repo-event", {
            repoId: repo.id,
            type: "commandDone",
            label,
            success: !bad,
            message: bad ? "fatal: could not read from remote" : "",
          });
        }, 10);
      }, 10);
      return null;
    },

    probe_merge: () => ({
      head: "abc1234",
      merge_head: null,
      rebase_in_progress: false,
      has_changes: false,
      has_conflicts: false,
      already_merged: false,
      ...(options.probeMerge ?? {}),
    }),

    probe_rebase: () => ({
      other_operation_in_progress: false,
      rebase_in_progress: false,
      has_changes: false,
      ...(options.probeRebase ?? {}),
    }),

    start_compare: () => {
      compareRequest += 1;
      const requestId = compareRequest;
      setTimeout(() => {
        emit("augur://repo-event", {
          repoId: 7,
          type: "branchCompareFiles",
          requestId,
          files: [
            {
              path: "src/lib.rs",
              old_path: null,
              new_path: "src/lib.rs",
              status: "modified",
              old_blob: null,
              new_blob: null,
              added: 3,
              deleted: 1,
            },
            {
              path: "src/new.rs",
              old_path: null,
              new_path: "src/new.rs",
              status: "added",
              old_blob: null,
              new_blob: null,
              added: 40,
              deleted: 0,
            },
          ],
        });
        setTimeout(() => {
          emit("augur://repo-event", {
            repoId: 7,
            type: "branchCompareFinished",
            requestId,
          });
        }, 10);
      }, 10);
      return requestId;
    },

    cancel_compare: () => null,
    export_patch: () => ++compareRequest,

    list_font_families: () => ["Inter", "Menlo", "Fira Code"],
    theme_options: () => [
      "github-dark",
      "catppuccin-latte",
      "catppuccin-frappe",
      "catppuccin-macchiato",
      "catppuccin-mocha",
    ],
    run_cli_installer: (args: any) => ({
      operation: args.operation,
      results: [
        { path: "~/.zshrc", outcome: "updated" },
        { path: "~/.config/fish/config.fish", outcome: "unchanged" },
      ],
      fallback_binary: false,
    }),
    list_wsl_distros: () => ["Ubuntu", "Debian"],
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
      return null;
    },
    set_workspace_tabs: (args: any) => {
      workspace.open_tabs = args.tabs;
      workspace.active_tab = args.active;
      return null;
    },
    set_shortcut: (args: any) => ({
      resolved: [{ command: args.command, keys: args.keys ?? ["CmdOrCtrl+Q"] }],
      overrides: args.keys ? { [args.command]: args.keys } : {},
    }),
    validate_shortcut: (args: any) => {
      if (typeof args.value !== "string" || args.value.trim().length === 0) {
        return Promise.reject({ key: "err-invalid-shortcut", detail: args.value ?? "" });
      }
      return args.value.split("+").map((part: string) => part.trim());
    },
    flush_state: () => null,
    open_about_window: () => null,
    open_compare_window: () => "compare-1",
    close_compare_window: () => null,
    focus_main_window: () => null,
    request_open_paths: (args: any) => {
      emit("augur://open-paths", { paths: args.paths });
      return null;
    },
  };

  // The plugin commands the interface reaches through the official JavaScript
  // packages. They are recorded rather than implemented, except for the dialog
  // picker, which returns the first available fixture.
  const pluginHandlers: Record<string, (args: any) => unknown> = {
    "plugin:event|listen": (args: any) => {
      const set = listeners.get(args.event) ?? new Set();
      listeners.set(args.event, set);
      // `handler` is the identifier `transformCallback` allocated, which is the
      // name the property is defined under on the global object.
      const id = args.handler;
      set.add((payload: unknown) => {
        (globalThis as any)[`_${id}`]?.(payload);
      });
      return Promise.resolve(id);
    },
    "plugin:event|unlisten": (args: any) => {
      // Unlistening is a no-op here: a page's subscriptions live as long as the
      // document, and every test drives a fresh page.
      void args;
      return null;
    },
    "plugin:event|emit": (args: any) => {
      emit(args.event, args.payload);
      return null;
    },
    "plugin:dialog|open": () => {
      const next = options.available.find(
        (repo) => !options.open.some((open) => open.path === repo.path),
      );
      return next?.path ?? options.available[0]?.path ?? null;
    },
    "plugin:dialog|save": () => "/tmp/compare.patch",
    "plugin:clipboard-manager|write_text": () => null,
    "plugin:clipboard-manager|read_text": () => "",
    "plugin:window|show": () => null,
    "plugin:window|destroy": () => null,
    "plugin:opener|open_path": () => null,
  };

  let callbackId = 1;
  const callbacks = new Map<number, (payload: unknown) => void>();

  const internals = {
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
        value: (payload: unknown) => callbacks.get(id)?.(payload),
      });
      return id;
    },
    async invoke(cmd: string, args: any = {}): Promise<unknown> {
      log.push({ cmd, args });
      if (cmd in handlers) {
        return handlers[cmd]!(args);
      }
      if (cmd in pluginHandlers) {
        return pluginHandlers[cmd]!(args);
      }
      // An unimplemented command is a real defect in a test: surface it rather
      // than resolving to undefined and failing somewhere else.
      return Promise.reject({ key: "err-unknown", detail: cmd });
    },
  };

  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
    configurable: true,
    value: internals,
  });

  // Exposed so a test can steer the stub and assert what the interface asked
  // for, which is how the command-label protocol and the open failure path are
  // covered.
  Object.defineProperty(globalThis, "__STUB__", {
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
      },
    },
  });
}
