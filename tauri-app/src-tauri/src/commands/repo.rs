//! Repository commands exposed to the webview: thin Tauri wrappers over the
//! shared service layer.
//!
//! Every mutating command takes a repository id and named parameters. None of
//! them accept raw Git arguments: the argv is built on this side from
//! [`augur_service::git_args::GitAction`], and paths travel as structured
//! values so a repository path can never be reinterpreted as command syntax.

use augur_service::commands::repo as service;
use augur_service::error::Result;
use augur_service::git_args::GitAction;
use augur_service::state::AppState;
use tauri::State;

use augur_core::config::GraphHistoryPreference;
use augur_core::git::{
    CheckoutTarget, FileStatus, RebaseTarget, ResetTarget, WorkingTreeDiffKind,
};

pub use augur_service::commands::repo::{
    Bootstrap, BuildInfo, ColumnVisibility, GraphLayout, MergeProbe, RebaseProbe, RepoSummary,
};
pub use augur_service::repo::CompareRevisionArg;
pub use augur_service::state::log_scope;

/// Load the initial state for one window.
#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>, window: tauri::Window) -> Result<Bootstrap> {
    Ok(service::bootstrap(&state, window.label()))
}

/// Open a repository as a new tab and start its Git worker.
#[tauri::command]
pub fn open_repository(
    state: State<'_, AppState>,
    path: String,
    location: augur_core::config::LocationConfig,
) -> Result<RepoSummary> {
    service::open_repository(&state, path, location)
}

/// Stop a repository's worker. The tab list is owned by the webview, which
/// calls this when a tab closes.
#[tauri::command]
pub fn close_repository(state: State<'_, AppState>, repo_id: u64) {
    service::close_repository(&state, repo_id);
}

/// Re-read the repository snapshot: status, refs, and the first log page.
#[tauri::command]
pub fn refresh_repository(state: State<'_, AppState>, repo_id: u64) -> Result<()> {
    service::refresh_repository(&state, repo_id)
}

/// Replace the commit-graph history scope and reload the first page.
#[tauri::command]
pub fn set_log_scope(
    state: State<'_, AppState>,
    repo_id: u64,
    upstream: Option<String>,
) -> Result<()> {
    service::set_log_scope(&state, repo_id, upstream)
}

/// Ask for the next page of the commit graph.
#[tauri::command]
pub fn load_more_log_page(state: State<'_, AppState>, repo_id: u64) -> Result<()> {
    service::load_more_log_page(&state, repo_id)
}

/// Select a commit: request its file list and full message together.
#[tauri::command]
pub fn select_commit(
    state: State<'_, AppState>,
    repo_id: u64,
    request_id: u64,
    oid: String,
) -> Result<()> {
    service::select_commit(&state, repo_id, request_id, oid)
}

/// Request only the full message, for a hover or the message dialog.
#[tauri::command]
pub fn request_commit_message(state: State<'_, AppState>, repo_id: u64, oid: String) -> Result<()> {
    service::request_commit_message(&state, repo_id, oid)
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
    service::load_commit_file_diff(&state, repo_id, request_id, oid, merge_parent, file)
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
    service::load_working_tree_diff(&state, repo_id, request_id, kind, file)
}

/// Load an image side only when its preview is visible in the webview.
#[tauri::command]
pub async fn load_image_preview(
    state: State<'_, AppState>,
    repo_id: u64,
    target: augur_core::git::image_preview::ImagePreviewTarget,
) -> Result<augur_core::git::image_preview::ImagePreview> {
    service::load_image_preview(&state, repo_id, target).await
}

/// Apply a staged/working-tree mutation and return the id the result carries.
#[tauri::command]
pub fn working_tree_operation(
    state: State<'_, AppState>,
    repo_id: u64,
    action: augur_core::git::WorkingTreeAction,
    files: Vec<FileStatus>,
    all: bool,
) -> Result<u64> {
    service::working_tree_operation(&state, repo_id, action, files, all)
}

/// Run one named Git operation.
#[tauri::command]
pub fn run_action(state: State<'_, AppState>, repo_id: u64, action: GitAction) -> Result<()> {
    service::run_action(&state, repo_id, action)
}

/// Check out a branch, tag, or commit.
#[tauri::command]
pub fn checkout(state: State<'_, AppState>, repo_id: u64, target: CheckoutTarget) -> Result<()> {
    service::checkout(&state, repo_id, target)
}

/// Start a revision comparison and return its request id.
#[tauri::command]
pub fn start_compare(
    state: State<'_, AppState>,
    repo_id: u64,
    base: CompareRevisionArg,
    target: CompareRevisionArg,
) -> Result<u64> {
    service::start_compare(&state, repo_id, base, target)
}

/// Cancel an in-flight comparison.
#[tauri::command]
pub fn cancel_compare(state: State<'_, AppState>, repo_id: u64) -> Result<()> {
    service::cancel_compare(&state, repo_id)
}

/// Write the full diff between two revisions to a patch file.
#[tauri::command]
pub fn export_patch(
    state: State<'_, AppState>,
    repo_id: u64,
    base: CompareRevisionArg,
    target: CompareRevisionArg,
    destination: std::path::PathBuf,
) -> Result<u64> {
    service::export_patch(&state, repo_id, base, target, destination)
}

/// Inspect the repository before a merge.
#[tauri::command]
pub async fn probe_merge(
    state: State<'_, AppState>,
    repo_id: u64,
    source: String,
) -> Result<MergeProbe> {
    service::probe_merge(&state, repo_id, source).await
}

/// Inspect the repository before a rebase.
#[tauri::command]
pub async fn probe_rebase(
    state: State<'_, AppState>,
    repo_id: u64,
    target: Option<RebaseTarget>,
) -> Result<RebaseProbe> {
    service::probe_rebase(&state, repo_id, target).await
}

/// Resolve and preview a reset target without changing the repository.
#[tauri::command]
pub async fn preview_reset(
    state: State<'_, AppState>,
    repo_id: u64,
    target: ResetTarget,
) -> Result<augur_core::git::operation_probe::ResetPreview> {
    service::preview_reset(&state, repo_id, target).await
}

/// Read a commit message without touching the working tree, for the clipboard.
#[tauri::command]
pub async fn read_commit_message(
    state: State<'_, AppState>,
    repo_id: u64,
    oid: String,
) -> Result<augur_core::git::CommitMessage> {
    service::read_commit_message(&state, repo_id, oid).await
}

/// Lane layout and ref labels for one page of commits.
#[tauri::command]
pub fn graph_layout(
    rows: Vec<augur_core::graph::LogRow>,
    remote_names: Vec<String>,
) -> GraphLayout {
    service::graph_layout(rows, remote_names)
}

/// Whether the author and message columns fit the available width.
#[tauri::command]
pub fn column_visibility(total_width: f32, tree_width: f32) -> ColumnVisibility {
    service::column_visibility(total_width, tree_width)
}

/// Set the history scope preference and tell the caller whether the loaded
/// graph must be reloaded.
#[tauri::command]
pub fn set_graph_history(
    state: State<'_, AppState>,
    preference: GraphHistoryPreference,
) -> Result<()> {
    service::set_graph_history(&state, preference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use augur_service::error::CommandError;
    use std::path::{Path, PathBuf};

    /// Every `.rs` file under the shell `src` tree and the service crate, since
    /// a key can be produced by either.
    fn command_sources() -> Vec<PathBuf> {
        fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
                .expect("source tree is readable")
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
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/augur-service/src"),
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
