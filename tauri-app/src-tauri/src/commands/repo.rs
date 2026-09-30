//! Repository commands exposed to the webview.
//!
//! Every mutating command takes a repository id and named parameters. None of
//! them accept raw Git arguments: the argv is built on this side from
//! [`crate::git_args::GitAction`], and paths travel as structured values so a
//! repository path can never be reinterpreted as command syntax.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use augur_core::config::{GraphHistoryPreference, LocationConfig};
use augur_core::git::operation_probe::{MergeState, RebaseState};
use augur_core::git::{
    CheckoutTarget, CommitMessage, FileStatus, WorkingTreeAction, WorkingTreeDiffKind,
    WorkingTreeScope,
};

use crate::git_args::GitAction;
use crate::repo::CompareRevisionArg;
use crate::state::{AppState, log_scope};

/// Why a command was refused. The webview localizes `key` and shows `detail`.
#[derive(Clone, Debug, Serialize)]
pub struct CommandError {
    pub key: String,
    pub detail: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.key, self.detail)
    }
}

impl CommandError {
    pub fn new(key: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// The repository session is gone, usually because its tab closed.
    pub fn missing_repo(repo_id: u64) -> Self {
        Self::new(
            "err-repo-closed",
            format!("repository {repo_id} is no longer open"),
        )
    }
}

impl From<augur_core::git::GitError> for CommandError {
    fn from(error: augur_core::git::GitError) -> Self {
        Self {
            key: error.key.to_string(),
            detail: error.detail,
        }
    }
}

type Result<T> = std::result::Result<T, CommandError>;

/// Everything the webview needs at startup.
#[derive(Clone, Debug, Serialize)]
pub struct Bootstrap {
    pub window: String,
    pub config: augur_core::config::AppConfig,
    pub workspace: augur_core::config::WorkspaceState,
    pub locale: String,
    pub catalogs: std::collections::HashMap<String, String>,
    pub shortcuts: augur_core::keymap::ShortcutState,
    pub build: BuildInfo,
    pub store_paths: Vec<String>,
    /// One entry per open repository so a reloaded window can resubscribe.
    pub repositories: Vec<RepoSummary>,
    pub has_pending_paths: bool,
}

/// Build metadata for the About window and the `--version` output.
#[derive(Clone, Debug, Serialize)]
pub struct BuildInfo {
    pub name: String,
    pub binary: String,
    pub identifier: String,
    pub version: String,
    pub authors: String,
    pub commit: String,
    pub version_line: String,
    pub platform: String,
}

impl BuildInfo {
    pub fn current() -> Self {
        use augur_core::build_info as info;
        Self {
            name: info::APP_NAME.to_string(),
            binary: info::APP_BINARY.to_string(),
            identifier: info::APP_IDENTIFIER.to_string(),
            version: info::APP_VERSION.to_string(),
            authors: info::app_authors_display(),
            commit: info::GIT_COMMIT_SHORT.to_string(),
            version_line: info::version_line(),
            platform: std::env::consts::OS.to_string(),
        }
    }
}

/// One open repository, as reported to a freshly loaded window.
#[derive(Clone, Debug, Serialize)]
pub struct RepoSummary {
    pub id: u64,
    pub path: String,
    pub location: LocationConfig,
}

/// Load the initial state for one window.
#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>, window: tauri::Window) -> Result<Bootstrap> {
    let persistence = state.persistence();
    let config = persistence.config();
    let locale = augur_core::i18n::resolve(&config.language);
    // Flattened so the webview holds one lookup table: a key missing from the
    // active catalog resolves through the English entry.
    let mut catalogs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let english = augur_core::i18n::catalog(augur_core::i18n::Locale::English);
    catalogs.extend(english.clone());
    if locale != augur_core::i18n::Locale::English {
        catalogs.extend(augur_core::i18n::catalog(locale));
    }
    Ok(Bootstrap {
        window: window.label().to_string(),
        config,
        workspace: persistence.workspace(),
        locale: locale.id().to_string(),
        catalogs,
        shortcuts: persistence.shortcut_state(),
        build: BuildInfo::current(),
        store_paths: persistence.store_paths(),
        repositories: repository_summaries(&state),
        has_pending_paths: state.has_pending_paths(),
    })
}

fn repository_summaries(state: &AppState) -> Vec<RepoSummary> {
    state
        .repository_ids()
        .into_iter()
        .filter_map(|id| {
            state.with_repo(id, |session| RepoSummary {
                id: session.id(),
                path: session.path().to_string(),
                location: session.location().clone(),
            })
        })
        .collect()
}

/// Open a repository as a new tab and start its Git worker.
#[tauri::command]
pub fn open_repository(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    location: LocationConfig,
) -> Result<RepoSummary> {
    let id = state.open_repository(&app, path.clone(), location.clone())?;
    Ok(RepoSummary { id, path, location })
}

/// Stop a repository's worker. The tab list is owned by the webview, which
/// calls this when a tab closes.
#[tauri::command]
pub fn close_repository(state: State<'_, AppState>, repo_id: u64) {
    state.close_repository(repo_id);
}

/// Re-read the repository snapshot: status, refs, and the first log page.
#[tauri::command]
pub fn refresh_repository(state: State<'_, AppState>, repo_id: u64) -> Result<()> {
    let session = state
        .with_repo(repo_id, |session| session.refresh())
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;
    Ok(session)
}

/// Replace the commit-graph history scope and reload the first page.
#[tauri::command]
pub fn set_log_scope(
    state: State<'_, AppState>,
    repo_id: u64,
    upstream: Option<String>,
) -> Result<()> {
    let preference = state.config().view.graph_history;
    state
        .with_repo(repo_id, |session| {
            session.set_log_scope(log_scope(preference, upstream));
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Ask for the next page of the commit graph.
#[tauri::command]
pub fn load_more_log_page(state: State<'_, AppState>, repo_id: u64) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.request_more_log_page())
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Select a commit: request its file list and full message together.
#[tauri::command]
pub fn select_commit(
    state: State<'_, AppState>,
    repo_id: u64,
    request_id: u64,
    oid: String,
) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.select_commit(request_id, oid))
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Request only the full message, for a hover or the message dialog.
#[tauri::command]
pub fn request_commit_message(state: State<'_, AppState>, repo_id: u64, oid: String) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.request_commit_message(oid))
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Request one file of a commit diff.
#[tauri::command]
pub fn load_commit_file_diff(
    state: State<'_, AppState>,
    repo_id: u64,
    request_id: u64,
    oid: String,
    merge_parent: Option<String>,
    file: augur_core::diff::FileChange,
) -> Result<()> {
    state
        .with_repo(repo_id, |session| {
            session.commit_file_diff(request_id, oid, merge_parent, file);
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Request a staged or working-tree diff using the caller's request id.
#[tauri::command]
pub fn load_working_tree_diff(
    state: State<'_, AppState>,
    repo_id: u64,
    request_id: u64,
    kind: WorkingTreeDiffKind,
    file: FileStatus,
) -> Result<()> {
    state
        .with_repo(repo_id, |session| {
            session.working_tree_diff(request_id, kind, file)
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Load an image side only when its preview is visible in the webview.
#[tauri::command]
pub async fn load_image_preview(
    state: State<'_, AppState>,
    repo_id: u64,
    target: augur_core::git::image_preview::ImagePreviewTarget,
) -> Result<augur_core::git::image_preview::ImagePreview> {
    let repo = state
        .with_repo(repo_id, |session| session.repo().clone())
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;
    run_blocking(move || Ok(augur_core::git::image_preview::load(&repo, &target))).await
}

/// Apply a staged/working-tree mutation and return the id the result carries.
#[tauri::command]
pub fn working_tree_operation(
    state: State<'_, AppState>,
    repo_id: u64,
    action: WorkingTreeAction,
    files: Vec<FileStatus>,
    all: bool,
) -> Result<u64> {
    let scope = if all {
        WorkingTreeScope::All(files)
    } else {
        let Some(file) = files.into_iter().next() else {
            return Err(CommandError::new(
                "err-no-files",
                "a single-file operation needs exactly one file",
            ));
        };
        WorkingTreeScope::File(file)
    };
    state
        .with_repo(repo_id, |session| {
            session.working_tree_operation(action, scope)
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Run one named Git operation.
#[tauri::command]
pub fn run_action(state: State<'_, AppState>, repo_id: u64, action: GitAction) -> Result<()> {
    let args = action
        .args()
        .map_err(|error| CommandError::new("err-invalid-action", error))?;
    let label = action.label();
    let refresh_after_success = action.refreshes_after_success();
    state
        .with_repo(repo_id, |session| {
            session.run(label, args, refresh_after_success)
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Check out a branch, tag, or commit.
#[tauri::command]
pub fn checkout(state: State<'_, AppState>, repo_id: u64, target: CheckoutTarget) -> Result<()> {
    run_action(state, repo_id, GitAction::Checkout { target })
}

/// Start a revision comparison and return its request id.
#[tauri::command]
pub fn start_compare(
    state: State<'_, AppState>,
    repo_id: u64,
    base: CompareRevisionArg,
    target: CompareRevisionArg,
) -> Result<u64> {
    state
        .with_repo(repo_id, |session| session.start_compare(base, target))
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Cancel an in-flight comparison.
#[tauri::command]
pub fn cancel_compare(state: State<'_, AppState>, repo_id: u64) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.cancel_compare())
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Write the full diff between two revisions to a patch file.
#[tauri::command]
pub fn export_patch(
    state: State<'_, AppState>,
    repo_id: u64,
    base: CompareRevisionArg,
    target: CompareRevisionArg,
    destination: PathBuf,
) -> Result<u64> {
    state
        .with_repo(repo_id, |session| {
            session.export_patch(base, target, destination)
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Merge preflight result. `already_merged` means the source is reachable from
/// HEAD, so the merge would be a no-op and Git would refuse it.
#[derive(Clone, Debug, Serialize)]
pub struct MergeProbe {
    #[serde(flatten)]
    pub state: MergeState,
    pub already_merged: bool,
    pub target_known: bool,
}

/// Rebase preflight result. `other_operation_in_progress` covers merge,
/// cherry-pick, revert, bisect, and sequencer state, none of which may be
/// replaced by a rebase.
#[derive(Clone, Debug, Serialize)]
pub struct RebaseProbe {
    #[serde(flatten)]
    pub state: RebaseState,
    pub other_operation_in_progress: bool,
    pub target_known: bool,
}

/// Inspect the repository before a merge.
#[tauri::command]
pub async fn probe_merge(
    state: State<'_, AppState>,
    repo_id: u64,
    source: String,
) -> Result<MergeProbe> {
    let repo = state
        .with_repo(repo_id, |session| session.repo().clone())
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;
    let probe = run_blocking(move || {
        let target = augur_core::git::operation_probe::resolve_branch_oid(&repo, &source).ok();
        let state = augur_core::git::operation_probe::probe_merge_state(&repo)?;
        let already_merged = match target.as_deref() {
            Some(oid) => augur_core::git::operation_probe::is_target_ancestor_of_head(&repo, oid)
                .unwrap_or(false),
            None => false,
        };
        Ok((state, target.is_some(), already_merged))
    })
    .await?;
    Ok(MergeProbe {
        state: probe.0,
        target_known: probe.1,
        already_merged: probe.2,
    })
}

/// Inspect the repository before a rebase.
#[tauri::command]
pub async fn probe_rebase(
    state: State<'_, AppState>,
    repo_id: u64,
    source: Option<String>,
) -> Result<RebaseProbe> {
    let repo = state
        .with_repo(repo_id, |session| session.repo().clone())
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;
    let probe = run_blocking(move || {
        let target = match source.as_deref() {
            Some(branch) => {
                augur_core::git::operation_probe::resolve_branch_oid(&repo, branch).ok()
            }
            None => None,
        };
        let other = augur_core::git::operation_probe::has_other_git_operation_except_rebase(&repo)?;
        let state = augur_core::git::operation_probe::probe_rebase_state(&repo)?;
        Ok((state, target.is_some(), other))
    })
    .await?;
    Ok(RebaseProbe {
        state: probe.0,
        target_known: probe.1,
        other_operation_in_progress: probe.2,
    })
}

/// Read a commit message without touching the working tree, for the clipboard.
#[tauri::command]
pub async fn read_commit_message(
    state: State<'_, AppState>,
    repo_id: u64,
    oid: String,
) -> Result<CommitMessage> {
    let repo = state
        .with_repo(repo_id, |session| session.repo().clone())
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;
    run_blocking(move || {
        use std::process::Stdio;
        let child = repo
            .command()
            .args([
                "--no-pager",
                "-C",
                repo.path(),
                "show",
                "-s",
                "--no-color",
                "--format=%B",
            ])
            .arg(&oid)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to run git show: {error}"))?;
        let output = child
            .wait_with_output()
            .map_err(|error| format!("failed to read git show output: {error}"))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "git show failed: {}",
                if detail.trim().is_empty() {
                    output.status.to_string()
                } else {
                    detail.trim().to_string()
                }
            ));
        }
        Ok(augur_core::git::parse_commit_message(
            &String::from_utf8_lossy(&output.stdout),
        ))
    })
    .await
    .map_err(|error| CommandError::new("err-commit-message", error.detail))
}

/// Run blocking Git work off the webview's async runtime.
async fn run_blocking<T, F>(body: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> std::result::Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(body)
        .await
        // The blocking pool itself failed, which is not Git's fault and has no
        // detail worth showing.
        .map_err(|error| CommandError::new("err-worker", error.to_string()))?
        // Git ran and said no. `err-git-run` carries the detail, and Git's own
        // explanation is the only useful thing in the message; `err-git` has no
        // placeholder, so using it would print a fixed sentence and throw the
        // reason away.
        .map_err(|detail| CommandError::new("err-git-run", detail))
}

/// Lane layout and ref labels for one page of commits.
///
/// The layout stays in Rust so it is literally the same algorithm the
/// reference application draws with, rather than a second implementation that
/// could drift. A page is 500 commits, so one call per page is negligible.
#[tauri::command]
pub fn graph_layout(
    rows: Vec<augur_core::graph::LogRow>,
    remote_names: Vec<String>,
) -> GraphLayout {
    let graph = augur_core::graph::compute_graph(&rows);
    let labels = rows
        .iter()
        .map(|row| {
            (
                row.oid.clone(),
                augur_core::graph::parse_ref_labels(&row.decorations, &remote_names),
            )
        })
        .collect();
    GraphLayout { graph, labels }
}

/// Lane layout plus the ref decoration of every commit.
#[derive(Clone, Debug, Serialize)]
pub struct GraphLayout {
    pub graph: Vec<augur_core::graph::GraphRow>,
    pub labels: std::collections::HashMap<String, Vec<augur_core::graph::RefLabel>>,
}

/// Whether the author and message columns fit the available width.
///
/// The thresholds live in one place, so the webview reveals columns at exactly
/// the width the reference application does. The frontend asks rather than
/// recomputing, which is what keeps the two definitions from drifting.
#[tauri::command]
pub fn column_visibility(total_width: f32, tree_width: f32) -> ColumnVisibility {
    let (author, message) = augur_core::graph::column_visibility(total_width, tree_width);
    ColumnVisibility { author, message }
}

/// Which of the two optional commit-list columns fit.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ColumnVisibility {
    /// The author column is wide enough to read.
    pub author: bool,
    /// The message column is wide enough to read.
    pub message: bool,
}

/// Set the history scope preference and tell the caller whether the loaded
/// graph must be reloaded.
#[tauri::command]
pub fn set_graph_history(
    state: State<'_, AppState>,
    preference: GraphHistoryPreference,
) -> Result<()> {
    state.update_settings(|settings| settings.config.view.graph_history = preference);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// Every `.rs` file under `src-tauri/src`, since a key can be produced by
    /// any command and the point of the check is that none of them is a typo.
    fn command_sources() -> Vec<PathBuf> {
        fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
                .expect("src-tauri/src is readable")
                .map(|entry| entry.expect("a directory entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    found.push(path);
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(!found.is_empty(), "no command sources were found");
        found
    }

    /// The first string literal of every `CommandError::new(...)` call.
    ///
    /// Parsed rather than listed, because a hand-written list of the same keys
    /// would drift from the code it is supposed to check, and a drifted list
    /// still passes. Each file is cut at its test module, which is where this
    /// scanner's own `CommandError::new("` literals live.
    fn produced_keys() -> Vec<String> {
        const MARKER: &str = "CommandError::new(";
        let mut keys = Vec::new();
        for path in command_sources() {
            let text = std::fs::read_to_string(&path).expect("a readable source file");
            let text = text.split("#[cfg(test)]").next().unwrap_or(text.as_str());
            let mut rest = text;
            while let Some(at) = rest.find(MARKER) {
                rest = &rest[at + MARKER.len()..];
                let Some(open) = rest.find('"') else {
                    break;
                };
                rest = &rest[open + 1..];
                let Some(close) = rest.find('"') else {
                    break;
                };
                keys.push(rest[..close].to_string());
                rest = &rest[close + 1..];
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }

    #[test]
    fn every_error_key_a_command_can_produce_exists_in_the_catalog() {
        let catalog = augur_core::i18n::catalog(augur_core::i18n::Locale::English);
        let keys = produced_keys();
        assert!(
            keys.len() > 5,
            "the scan found almost nothing, so it is not looking at the right code"
        );
        let missing: Vec<&String> = keys
            .iter()
            .filter(|key| !catalog.contains_key(key.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "these keys are not in en-US.ftl, so the webview would print the key itself: {missing:?}"
        );
    }

    #[test]
    fn a_git_failure_keeps_gits_own_explanation() {
        // The key has to carry the detail, or the one message worth reading is
        // thrown away in favour of a fixed sentence.
        let error = CommandError::new("err-git-run", "fatal: not a git repository");
        let rendered = augur_core::i18n::text_args(
            augur_core::i18n::Locale::English,
            &error.key,
            &[("detail", error.detail.as_str())],
        );
        assert!(
            rendered.contains("fatal: not a git repository"),
            "{rendered}"
        );
    }
}
