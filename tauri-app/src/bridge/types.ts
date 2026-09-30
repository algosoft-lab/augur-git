/**
 * Typed mirror of the Rust data model.
 *
 * Every type here crosses the Tauri boundary, so the field names must match the
 * serde output of the backend exactly. Rust enums serialize as camelCase
 * strings; `Option<T>` becomes `T | null`.
 */

// ===== Configuration =====

export type LanguagePreference = 'system' | 'en-US' | 'zh-CN';

export type ThemePreference =
  | 'github-dark'
  | 'catppuccin-latte'
  | 'catppuccin-frappe'
  | 'catppuccin-macchiato'
  | 'catppuccin-mocha'
  | 'dracula'
  | 'tokyo-night'
  | 'tokyo-night-storm'
  | 'tokyo-night-light'
  | 'gruvbox-dark'
  | 'gruvbox-light'
  | 'nord'
  | 'solarized-dark'
  | 'solarized-light'
  | 'rose-pine'
  | 'rose-pine-moon'
  | 'rose-pine-dawn'
  | 'ayu-dark'
  | 'ayu-mirage'
  | 'ayu-light'
  | 'kanagawa-wave'
  | 'kanagawa-lotus'
  | 'github-dark-default'
  | 'github-light-default'
  | 'atom-one-dark'
  | 'atom-one-light'
  | 'everforest-dark'
  | 'everforest-light'
  | 'night-owl'
  | 'light-owl'
  | 'claude-dark'
  | 'claude-light';

export type DiffLayoutPreference = 'inline' | 'side-by-side';

export type GraphHistoryPreference = 'current-branch' | 'all-branches';

export type CommitActionPreference = 'commit' | 'amend';

export type PullActionPreference = 'merge' | 'rebase';

export interface ViewSettings {
  show_untracked: boolean;
  auto_follow: boolean;
  diff_layout: DiffLayoutPreference;
  diff_soft_wrap: boolean;
  graph_history: GraphHistoryPreference;
  auto_refresh: boolean;
  commit_action: CommitActionPreference;
  pull_action: PullActionPreference;
}

export interface TypographySettings {
  ui_font_family: string | null;
  mono_font_family: string | null;
  ui_font_size: number;
  diff_font_size: number;
}

export interface LocationConfig {
  kind: 'local' | 'wsl';
  distro?: string;
}

export type AgentPromptRequest =
  | { kind: 'commit'; amend: boolean }
  | { kind: 'merge'; source: string; noFf: boolean }
  | { kind: 'rebase'; source: string }
  | { kind: 'pull'; rebase: boolean }
  | { kind: 'resolveConflicts'; origin?: 'merge' | 'rebase' | 'stashPop' }
  | { kind: 'applyPatch'; path: string; failure?: string };

export interface RecentRepo {
  path: string;
  location: LocationConfig;
}

export interface AppConfig {
  schema_version: number;
  auto_check_updates: boolean;
  dismissed_update_commit: string | null;
  theme: ThemePreference;
  language: LanguagePreference;
  view: ViewSettings;
  typography: TypographySettings;
  recent_repos: RecentRepo[];
}

export interface LayoutSettings {
  sidebar_width: number;
  right_panel_width: number;
  diff_height: number | null;
  file_list_ratio: number;
}

export interface OpenTabConfig {
  path: string;
  location: LocationConfig;
}

export interface WorkspaceState {
  schema_version: number;
  open_tabs: OpenTabConfig[];
  active_tab: string | null;
  layout: LayoutSettings;
  window_mode: WindowMode;
  desktop_window: WindowBounds | null;
  sidecar_window: WindowBounds | null;
}

export type WindowMode = 'desktop' | 'sidecar';
export type SettingsSection = 'general' | 'appearance' | 'layout' | 'shortcuts';

export interface WindowBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ResolvedShortcut {
  command: string;
  keys: string[];
}

export interface ShortcutState {
  resolved: ResolvedShortcut[];
  defaults: ResolvedShortcut[];
  overrides: Record<string, string[]>;
}

// ===== Git model =====

export interface FileStatus {
  index: string;
  worktree: string;
  path: string;
  old_path: string | null;
}

export interface DiffLineStats {
  added: number;
  deleted: number;
}

export interface WorkingTreeDiffStats {
  staged: DiffLineStats | null;
  unstaged: DiffLineStats | null;
  untracked: DiffLineStats | null;
}

export interface BranchInfo {
  name: string;
  is_head: boolean;
}

export interface CompareRevision {
  name: string;
  full_name: string;
  kind: 'local' | 'remote' | 'tag' | 'commit';
}

export interface StashInfo {
  reference: string;
  description: string;
}

export interface RefsInfo {
  remotes: string[];
  remote_branches: string[];
  tags: string[];
  stashes: StashInfo[];
  comparison_revisions: CompareRevision[];
}

export interface CoAuthor {
  name: string;
  email: string;
}

export interface CommitMessage {
  subject: string;
  body: string;
  co_authors: CoAuthor[];
}

export type FileChangeStatus =
  'added' | 'copied' | 'deleted' | 'modified' | 'renamed' | 'typeChanged' | 'unmerged' | 'unknown';

export interface FileChange {
  path: string;
  old_path: string | null;
  new_path: string;
  status: FileChangeStatus;
  old_blob: string | null;
  new_blob: string | null;
  added: number | null;
  deleted: number | null;
}

export type CheckoutTarget =
  | { kind: 'localBranch'; localBranch: string }
  | { kind: 'remoteBranch'; remoteBranch: string }
  | { kind: 'tag'; tag: string }
  | { kind: 'commit'; commit: string };

export interface LogRow {
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

export type RefKind = 'head' | 'localBranch' | 'remoteBranch' | 'tag';

export interface RefLabel {
  name: string;
  kind: RefKind;
}

export interface GraphLane {
  oid: string;
  color_index: number;
}

export interface GraphRow {
  input_lanes: GraphLane[];
  output_lanes: GraphLane[];
  parent_lanes: number[];
  node_lane: number;
  node_input_lanes: number[];
  lane_count: number;
  node_color: number;
  has_incoming: boolean;
  is_head: boolean;
  is_merge: boolean;
}

export interface SourceText {
  text: string;
  line_starts: number[];
  lines: string[];
}

export type DiffLineKind = 'meta' | 'hunk' | 'add' | 'del' | 'context';

export interface DiffRow {
  kind: DiffLineKind;
  old_no: number | null;
  new_no: number | null;
  old_text: string | null;
  new_text: string | null;
  old_line_index: number | null;
  new_line_index: number | null;
  hunk_header: string | null;
}

/** A half-open byte range inside one line. */
export type CharRange = { start: number; end: number };

/**
 * A parsed single-file diff.
 *
 * Both row lists are sent because the pairing rule lives in the core crate; the
 * viewer only chooses which one to mount. The character ranges arrive with it
 * so inline highlighting needs no second round trip.
 */
export interface DiffPayload {
  path: string;
  language: string | null;
  rows: DiffRow[];
  aligned_rows: DiffRow[];
  old_source: SourceText | null;
  new_source: SourceText | null;
  inline_old: CharRange[][];
  inline_new: CharRange[][];
  binary: boolean;
  copy_text: string;
}

/** The fields the viewer needs, named for the component that consumes them. */
export type DiffDocument = DiffPayload;

export type ImagePreviewTarget =
  | { kind: 'change'; file: FileChange }
  | { kind: 'workingTree'; diffKind: WorkingTreeDiffKind; file: FileStatus };

export type ImagePreviewSide =
  | { status: 'absent' }
  | { status: 'available'; mimeType: string; data: string }
  | { status: 'unavailable'; reason: 'unsupported' | 'tooLarge' | 'unreadable' };

export interface ImagePreview {
  old: ImagePreviewSide;
  new: ImagePreviewSide;
}

export type WorkingTreeDiffKind = 'staged' | 'unstaged';
export type WorkingTreeAction = 'stage' | 'unstage' | 'discard';
export type WorkingTreeScopeKind = 'file' | 'all';

export interface GitError {
  key: string;
  detail: string;
}

export type CompareRevisionArg =
  | { kind: 'local'; name: string }
  | { kind: 'remote'; name: string }
  | { kind: 'tag'; name: string }
  | { kind: 'commit'; name: string };

export type GitAction =
  | { action: 'fetch' }
  | { action: 'pullMerge' }
  | { action: 'pullRebase' }
  | { action: 'push' }
  | { action: 'pushForce' }
  | { action: 'pushSetUpstream'; remote: string; branch: string }
  | { action: 'pushRenameRemote'; remote: string; old: string; new: string }
  | { action: 'pushDeleteRemote'; remote: string; branch: string }
  | { action: 'stash'; message: string }
  | { action: 'stashPop'; stashRef: string | null }
  | { action: 'stashDrop'; stashRef: string }
  | { action: 'applyPatch'; path: string }
  | { action: 'checkout'; target: CheckoutTarget }
  | { action: 'createBranch'; name: string }
  | { action: 'renameBranch'; old: string; new: string }
  | { action: 'deleteBranch'; name: string; force: boolean }
  | { action: 'deleteTag'; name: string }
  | { action: 'merge'; source: string; noFf: boolean }
  | { action: 'rebase'; source: string }
  | { action: 'abortMerge' }
  | { action: 'abortRebase' }
  | { action: 'abortStashApply' }
  | { action: 'commit'; message: string; amend: boolean }
  | { action: 'copyCommitMessage'; oid: string };

// ===== Events =====

export type RepoEvent =
  | {
      type: 'status';
      branch: string;
      head: string | null;
      upstream: string | null;
      ahead: number;
      behind: number;
      files: FileStatus[];
      diff_stats: WorkingTreeDiffStats;
      branches: BranchInfo[];
    }
  | { type: 'logPage'; rows: LogRow[]; replace: boolean; hasMore: boolean }
  | { type: 'refs'; refs: RefsInfo }
  | {
      type: 'commitFiles';
      requestId: number;
      oid: string;
      files: FileChange[];
      merge_parent: string | null;
    }
  | { type: 'commitFilesError'; requestId: number; oid: string; error: GitError }
  | { type: 'commitMessage'; oid: string; message: CommitMessage }
  | {
      type: 'fileDiff';
      requestId: number;
      oid: string;
      file: FileChange;
      document: DiffDocument;
    }
  | {
      type: 'fileDiffError';
      requestId: number;
      oid: string;
      file: FileChange;
      error: GitError;
    }
  | {
      type: 'workingTreeFileDiff';
      requestId: number;
      kind: WorkingTreeDiffKind;
      file: FileStatus;
      document: DiffDocument;
    }
  | {
      type: 'workingTreeFileDiffError';
      requestId: number;
      kind: WorkingTreeDiffKind;
      file: FileStatus;
      detail: string;
    }
  | {
      type: 'workingTreeOperationFinished';
      requestId: number;
      action: WorkingTreeAction;
      scope: WorkingTreeScopeKind;
      success: boolean;
      detail: string;
    }
  | { type: 'branchCompareFiles'; requestId: number; files: FileChange[] }
  | {
      type: 'branchCompareFileDiff';
      requestId: number;
      file: FileChange;
      document: DiffDocument;
    }
  | {
      type: 'branchCompareError';
      requestId: number;
      file: FileChange | null;
      detail: string;
    }
  | { type: 'branchCompareFinished'; requestId: number }
  | {
      type: 'branchComparePatchExported';
      requestId: number;
      destination: string;
      bytes: number;
    }
  | { type: 'branchComparePatchError'; requestId: number; detail: string }
  | { type: 'commandStarted'; label: string; verb: string }
  | { type: 'commandDone'; label: string; success: boolean; message: string }
  | { type: 'openFailed'; error: GitError }
  | { type: 'statusError'; error: GitError }
  | { type: 'error'; error: GitError };

export type RepoEventEnvelope = RepoEvent & { repoId: number };

export type AppEvent =
  | { type: 'settingsChanged' }
  | { type: 'workspaceChanged' }
  | { type: 'notice'; level: string; message: string };

export type UpdateState =
  'idle' | 'checking' | 'up-to-date' | 'available' | 'downloading' | 'downloaded' | 'error';

export type InstallChannel = 'windows-installer' | 'homebrew-cask' | 'manual';

export interface UpdateStatus {
  state: UpdateState;
  currentVersion: string;
  latestVersion: string | null;
  latestCommitSha: string | null;
  canInstall: boolean;
  progress: number | null;
  error: string | null;
  installChannel: InstallChannel;
}

export interface UpdateNotice {
  commitSha: string;
  version: string;
}

export interface UpdateSnapshot {
  status: UpdateStatus;
  notice: UpdateNotice | null;
}

export type UpdateEvent =
  { type: 'status'; status: UpdateStatus } | { type: 'notice'; notice: UpdateNotice | null };

// ===== Commands =====

export interface BuildInfo {
  name: string;
  binary: string;
  identifier: string;
  version: string;
  authors: string;
  commit: string;
  version_line: string;
  platform: string;
}

export interface RepoSummary {
  id: number;
  path: string;
  location: LocationConfig;
}

/** Which optional commit-list columns fit the available width. */
export interface ColumnVisibility {
  author: boolean;
  message: boolean;
}

export interface Bootstrap {
  window: string;
  config: AppConfig;
  workspace: WorkspaceState;
  locale: string;
  catalogs: Record<string, string>;
  shortcuts: ShortcutState;
  build: BuildInfo;
  store_paths: string[];
  repositories: RepoSummary[];
  has_pending_paths: boolean;
}

export interface MergeState {
  head: string | null;
  merge_head: string | null;
  rebase_in_progress: boolean;
  has_changes: boolean;
  has_conflicts: boolean;
}

export interface MergeProbe extends MergeState {
  already_merged: boolean;
  target_known: boolean;
}

export interface RebaseState {
  head: string | null;
  rebase_head: string | null;
  rebase_in_progress: boolean;
  has_changes: boolean;
  has_conflicts: boolean;
}

export interface RebaseProbe extends RebaseState {
  other_operation_in_progress: boolean;
  target_known: boolean;
}

export type ThemeTokens = Record<string, string>;
