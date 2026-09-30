/**
 * The only module that talks to Tauri.
 *
 * Every invoke and event subscription is declared here so the rest of the
 * interface depends on typed functions instead of string command names, and so
 * the event names exist in exactly one place.
 */

import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppConfig,
  AppEvent,
  AgentPromptRequest,
  Bootstrap,
  CommitActionPreference,
  CommitMessage,
  CompareRevisionArg,
  DiffLayoutPreference,
  FileChange,
  FileStatus,
  GitAction,
  GraphRow,
  LayoutSettings,
  LanguagePreference,
  LocationConfig,
  LogRow,
  MergeProbe,
  OpenTabConfig,
  ColumnVisibility,
  RefLabel,
  RebaseProbe,
  RepoEventEnvelope,
  RepoSummary,
  SettingsSection,
  ShortcutState,
  ThemePreference,
  TypographySettings,
  ViewSettings,
  WindowMode,
  WorkspaceState,
  WorkingTreeAction,
  WorkingTreeDiffKind
} from './types';

/** Event names, matching `src-tauri/src/events.rs`. */
export const REPO_EVENT = 'augur://repo-event';
export const APP_EVENT = 'augur://app-event';
export const MENU_EVENT = 'augur://menu';
export const SETTINGS_NAVIGATE_EVENT = 'augur://settings-navigate';
export const OPEN_PATHS_EVENT = 'augur://open-paths';
export const DROP_EVENT = 'augur://drop-paths';

export interface CommandError {
  key: string;
  detail: string;
}

/** Turn a rejected invoke into a message the interface can show. */
export function describeError(error: unknown): CommandError {
  if (error && typeof error === 'object' && 'key' in error && 'detail' in error) {
    const value = error as { key: string; detail: string };
    return { key: value.key, detail: value.detail };
  }
  if (typeof error === 'string') {
    return { key: 'err-unknown', detail: error };
  }
  return { key: 'err-unknown', detail: String(error) };
}

export async function bootstrap(): Promise<Bootstrap> {
  return invoke<Bootstrap>('bootstrap');
}

export async function openRepository(path: string, location: LocationConfig): Promise<RepoSummary> {
  return invoke<RepoSummary>('open_repository', { path, location });
}

export async function closeRepository(repoId: number): Promise<void> {
  return invoke<void>('close_repository', { repoId });
}

export async function refreshRepository(repoId: number): Promise<void> {
  return invoke<void>('refresh_repository', { repoId });
}

export async function setAutoRefreshTarget(
  repoId: number | null,
  generation: number
): Promise<void> {
  return invoke<void>('set_auto_refresh_target', { repoId, generation });
}

export async function setLogScope(repoId: number, upstream: string | null): Promise<void> {
  return invoke<void>('set_log_scope', { repoId, upstream });
}

export async function loadMoreLogPage(repoId: number): Promise<void> {
  return invoke<void>('load_more_log_page', { repoId });
}

export async function selectCommit(repoId: number, requestId: number, oid: string): Promise<void> {
  return invoke<void>('select_commit', { repoId, requestId, oid });
}

export async function requestCommitMessage(repoId: number, oid: string): Promise<void> {
  return invoke<void>('request_commit_message', { repoId, oid });
}

export async function loadCommitFileDiff(
  repoId: number,
  requestId: number,
  oid: string,
  mergeParent: string | null,
  file: FileChange
): Promise<void> {
  return invoke<void>('load_commit_file_diff', {
    repoId,
    requestId,
    oid,
    mergeParent,
    file
  });
}

export async function loadWorkingTreeDiff(
  repoId: number,
  requestId: number,
  kind: WorkingTreeDiffKind,
  file: FileStatus
): Promise<void> {
  return invoke<void>('load_working_tree_diff', { repoId, requestId, kind, file });
}

export async function workingTreeOperation(
  repoId: number,
  action: WorkingTreeAction,
  files: FileStatus[],
  all: boolean
): Promise<number> {
  return invoke<number>('working_tree_operation', {
    repoId,
    action,
    files,
    all
  });
}

export async function runAction(repoId: number, action: GitAction): Promise<void> {
  return invoke<void>('run_action', { repoId, action });
}

export async function generateAgentPrompt(
  repoId: number,
  request: AgentPromptRequest
): Promise<string> {
  return invoke<string>('generate_agent_prompt', { repoId, request });
}

export async function startCompare(
  repoId: number,
  base: CompareRevisionArg,
  target: CompareRevisionArg
): Promise<number> {
  return invoke<number>('start_compare', { repoId, base, target });
}

export async function cancelCompare(repoId: number): Promise<void> {
  return invoke<void>('cancel_compare', { repoId });
}

export async function exportPatch(
  repoId: number,
  base: CompareRevisionArg,
  target: CompareRevisionArg,
  destination: string
): Promise<number> {
  return invoke<number>('export_patch', {
    repoId,
    base,
    target,
    destination
  });
}

export async function probeMerge(repoId: number, source: string): Promise<MergeProbe> {
  return invoke<MergeProbe>('probe_merge', { repoId, source });
}

export async function probeRebase(repoId: number, source: string | null): Promise<RebaseProbe> {
  return invoke<RebaseProbe>('probe_rebase', { repoId, source });
}

export async function readCommitMessage(repoId: number, oid: string): Promise<CommitMessage> {
  return invoke<CommitMessage>('read_commit_message', { repoId, oid });
}

export interface GraphLayout {
  graph: GraphRow[];
  labels: Record<string, RefLabel[]>;
}

/**
 * Lane layout and ref labels for the commits the webview is about to draw.
 *
 * The layout stays in the backend so it is the same algorithm the reference
 * application uses rather than a second implementation that could drift.
 */
export async function graphLayout(rows: LogRow[], remoteNames: string[]): Promise<GraphLayout> {
  return invoke<GraphLayout>('graph_layout', { rows, remoteNames });
}

/**
 * Ask which commit-list columns fit.
 *
 * The thresholds are the backend's, so a column appears at exactly the width it
 * does in the reference application.
 */
export async function columnVisibility(
  totalWidth: number,
  treeWidth: number
): Promise<ColumnVisibility> {
  return invoke<ColumnVisibility>('column_visibility', {
    totalWidth,
    treeWidth
  });
}

export async function listFontFamilies(): Promise<string[]> {
  return invoke<string[]>('list_font_families');
}

export async function themeOptions(): Promise<ThemePreference[]> {
  return invoke<ThemePreference[]>('theme_options');
}

export async function listWslDistros(): Promise<string[]> {
  return invoke<string[]>('list_wsl_distros');
}

export async function probeWslRepository(distro: string, path: string): Promise<void> {
  return invoke<void>('probe_wsl_repository', { distro, path });
}

export async function setLanguage(language: LanguagePreference): Promise<void> {
  return invoke<void>('set_language', { language });
}

export async function setTheme(theme: ThemePreference): Promise<void> {
  return invoke<void>('set_theme', { theme });
}

export async function setView(view: ViewSettings): Promise<void> {
  return invoke<void>('set_view', { view });
}

export async function setTypography(typography: TypographySettings): Promise<void> {
  return invoke<void>('set_typography', { typography });
}

export async function setCommitAction(action: CommitActionPreference): Promise<void> {
  return invoke<void>('set_commit_action', { action });
}

export async function setDiffLayout(layout: DiffLayoutPreference): Promise<void> {
  return invoke<void>('set_diff_layout', { layout });
}

export async function setLayout(layout: LayoutSettings): Promise<void> {
  return invoke<void>('set_layout', { layout });
}

export async function setWindowMode(mode: WindowMode): Promise<WorkspaceState> {
  return invoke<WorkspaceState>('set_window_mode', { mode });
}

export async function saveWindowBounds(): Promise<void> {
  return invoke<void>('save_window_bounds');
}

export async function setWorkspaceTabs(
  tabs: OpenTabConfig[],
  active: string | null
): Promise<void> {
  return invoke<void>('set_workspace_tabs', { tabs, active });
}

export async function setShortcut(command: string, keys: string[] | null): Promise<ShortcutState> {
  return invoke<ShortcutState>('set_shortcut', { command, keys });
}

export async function validateShortcut(value: string): Promise<string[]> {
  return invoke<string[]>('validate_shortcut', { value });
}

export async function flushState(): Promise<void> {
  return invoke<void>('flush_state');
}

export async function openAboutWindow(): Promise<void> {
  return invoke<void>('open_about_window');
}

export async function openSettingsWindow(section?: SettingsSection): Promise<void> {
  return invoke<void>('open_settings_window', { section: section ?? null });
}

export async function openCompareWindow(repoId: number): Promise<string> {
  return invoke<string>('open_compare_window', { repoId });
}

export async function closeCompareWindow(repoId: number): Promise<void> {
  return invoke<void>('close_compare_window', { repoId });
}

export async function focusMainWindow(): Promise<void> {
  return invoke<void>('focus_main_window');
}

export async function requestOpenPaths(paths: string[]): Promise<void> {
  return invoke<void>('request_open_paths', { paths });
}

/**
 * Collect the paths handed over before this window was listening.
 *
 * A path from a second launch can arrive while the window is still booting, and
 * an event emitted to a window that has not yet subscribed is dropped without a
 * trace, so the backend also holds them until asked.
 */
export async function takePendingPaths(): Promise<string[]> {
  return invoke<string[]>('take_pending_paths');
}

export async function currentConfig(): Promise<AppConfig> {
  return invoke<AppConfig>('current_config');
}

export async function repositorySummary(repoId: number): Promise<RepoSummary | null> {
  return invoke<RepoSummary | null>('repository_summary', { repoId });
}

// ===== Event subscriptions =====

export function onRepoEvent(handler: (event: RepoEventEnvelope) => void): Promise<UnlistenFn> {
  return listen<RepoEventEnvelope>(REPO_EVENT, (event) => handler(event.payload));
}

export function onAppEvent(handler: (event: AppEvent) => void): Promise<UnlistenFn> {
  return listen<AppEvent>(APP_EVENT, (event) => handler(event.payload));
}

export function onMenuEvent(handler: (id: string) => void): Promise<UnlistenFn> {
  return listen<{ id: string }>(MENU_EVENT, (event) => handler(event.payload.id));
}

export function onSettingsNavigate(
  handler: (section: SettingsSection) => void
): Promise<UnlistenFn> {
  return listen<{ section: SettingsSection }>(SETTINGS_NAVIGATE_EVENT, (event) =>
    handler(event.payload.section)
  );
}

export function onOpenPaths(handler: (paths: string[]) => void): Promise<UnlistenFn> {
  return listen<{ paths: string[] }>(OPEN_PATHS_EVENT, (event) => handler(event.payload.paths));
}

export function onDropPaths(handler: (paths: string[]) => void): Promise<UnlistenFn> {
  return listen<{ paths: string[] }>(DROP_EVENT, (event) => handler(event.payload.paths));
}

/** Tell the backend to open paths in the window that owns the tab list. */
export async function emitOpenPaths(paths: string[]): Promise<void> {
  await emit(OPEN_PATHS_EVENT, { paths });
}
