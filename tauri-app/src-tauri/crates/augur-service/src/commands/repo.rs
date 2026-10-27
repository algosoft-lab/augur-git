//! Service-layer command bodies shared by the desktop and server frontends.
//!
//! Every function here is plain Rust over [`AppState`]: no window handles, no
//! framework extractors. Each frontend wraps these with its own boundary —
//! Tauri command attributes in the desktop shell, an HTTP dispatcher in the
//! server — which keeps the two behaviorally identical by construction.

use std::path::PathBuf;

use serde::Serialize;

use augur_core::config::{GraphHistoryPreference, LocationConfig};
use augur_core::git::operation_probe::{MergeState, RebaseState};
use augur_core::git::{
    CheckoutTarget, CommitMessage, FileStatus, RebaseTarget, ResetTarget, WorkingTreeAction,
    WorkingTreeDiffKind, WorkingTreeScope,
};

use crate::error::{CommandError, Result};
use crate::git_args::GitAction;
use crate::repo::CompareRevisionArg;
use crate::state::{AppState, log_scope};

/// Everything the frontend needs at startup.
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
    /// One entry per open repository so a reloaded client can resubscribe.
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

/// One open repository, as reported to a freshly loaded client.
#[derive(Clone, Debug, Serialize)]
pub struct RepoSummary {
    pub id: u64,
    pub path: String,
    pub location: LocationConfig,
}

/// Load the initial state for one client. `window` is the frontend's client
/// label: the desktop uses the Tauri window label, the server the page role.
pub fn bootstrap(state: &AppState, window: &str) -> Bootstrap {
    let persistence = state.persistence();
    let config = persistence.config();
    let locale = augur_core::i18n::resolve(&config.language);
    // Flattened so the frontend holds one lookup table: a key missing from the
    // active catalog resolves through the English entry.
    let mut catalogs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let english = augur_core::i18n::catalog(augur_core::i18n::Locale::English);
    catalogs.extend(english.clone());
    if locale != augur_core::i18n::Locale::English {
        catalogs.extend(augur_core::i18n::catalog(locale));
    }
    Bootstrap {
        window: window.to_string(),
        config,
        workspace: persistence.workspace(),
        locale: locale.id().to_string(),
        catalogs,
        shortcuts: persistence.shortcut_state(),
        build: BuildInfo::current(),
        store_paths: persistence.store_paths(),
        repositories: repository_summaries(state),
        has_pending_paths: state.has_pending_paths(),
    }
}

pub fn repository_summaries(state: &AppState) -> Vec<RepoSummary> {
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
pub fn open_repository(
    state: &AppState,
    path: String,
    location: LocationConfig,
) -> Result<RepoSummary> {
    let id = state.open_repository(path.clone(), location.clone())?;
    Ok(RepoSummary { id, path, location })
}

/// Stop a repository's worker. The tab list is owned by the frontend, which
/// calls this when a tab closes.
pub fn close_repository(state: &AppState, repo_id: u64) {
    state.close_repository(repo_id);
}

/// Re-read the repository snapshot: status, refs, and the first log page.
pub fn refresh_repository(state: &AppState, repo_id: u64) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.refresh())
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Replace the commit-graph history scope and reload the first page.
pub fn set_log_scope(state: &AppState, repo_id: u64, upstream: Option<String>) -> Result<()> {
    let preference = state.config().view.graph_history;
    state
        .with_repo(repo_id, |session| {
            session.set_log_scope(log_scope(preference, upstream));
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Ask for the next page of the commit graph.
pub fn load_more_log_page(state: &AppState, repo_id: u64) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.request_more_log_page())
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Select a commit: request its file list and full message together.
pub fn select_commit(state: &AppState, repo_id: u64, request_id: u64, oid: String) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.select_commit(request_id, oid))
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Request only the full message, for a hover or the message dialog.
pub fn request_commit_message(state: &AppState, repo_id: u64, oid: String) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.request_commit_message(oid))
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Request one file of a commit diff.
pub fn load_commit_file_diff(
    state: &AppState,
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
pub fn load_working_tree_diff(
    state: &AppState,
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

/// Load an image side only when its preview is visible in the frontend.
pub async fn load_image_preview(
    state: &AppState,
    repo_id: u64,
    target: augur_core::git::image_preview::ImagePreviewTarget,
) -> Result<augur_core::git::image_preview::ImagePreview> {
    let repo = repository(state, repo_id)?;
    run_blocking(move || Ok(augur_core::git::image_preview::load(&repo, &target))).await
}

/// Apply a staged/working-tree mutation and return the id the result carries.
pub fn working_tree_operation(
    state: &AppState,
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
pub fn run_action(state: &AppState, repo_id: u64, action: GitAction) -> Result<()> {
    if let GitAction::Reset {
        mode,
        expected_head,
        target_oid,
    } = &action
    {
        return state
            .with_repo(repo_id, |session| {
                session.reset(*mode, expected_head.clone(), target_oid.clone());
            })
            .ok_or_else(|| CommandError::missing_repo(repo_id));
    }
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
pub fn checkout(state: &AppState, repo_id: u64, target: CheckoutTarget) -> Result<()> {
    run_action(state, repo_id, GitAction::Checkout { target })
}

/// Start a revision comparison and return its request id.
pub fn start_compare(
    state: &AppState,
    repo_id: u64,
    base: CompareRevisionArg,
    target: CompareRevisionArg,
) -> Result<u64> {
    state
        .with_repo(repo_id, |session| session.start_compare(base, target))
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Cancel an in-flight comparison.
pub fn cancel_compare(state: &AppState, repo_id: u64) -> Result<()> {
    state
        .with_repo(repo_id, |session| session.cancel_compare())
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Write the full diff between two revisions to a patch file.
pub fn export_patch(
    state: &AppState,
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
    pub target_oid: Option<String>,
}

/// Borrow the Git handle of an open repository, or refuse.
pub fn repository(
    state: &AppState,
    repo_id: u64,
) -> Result<augur_core::git::GitRepo> {
    state
        .with_repo(repo_id, |session| session.repo().clone())
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

/// Inspect the repository before a merge.
pub async fn probe_merge(state: &AppState, repo_id: u64, source: String) -> Result<MergeProbe> {
    let repo = repository(state, repo_id)?;
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
pub async fn probe_rebase(
    state: &AppState,
    repo_id: u64,
    target: Option<RebaseTarget>,
) -> Result<RebaseProbe> {
    let repo = repository(state, repo_id)?;
    let probe = run_blocking(move || {
        let target_oid = match target.as_ref() {
            Some(target) => {
                augur_core::git::operation_probe::resolve_rebase_target(&repo, target).ok()
            }
            None => None,
        };
        let other = augur_core::git::operation_probe::has_other_git_operation_except_rebase(&repo)?;
        let state = augur_core::git::operation_probe::probe_rebase_state(&repo)?;
        Ok((state, target_oid, other))
    })
    .await?;
    Ok(RebaseProbe {
        state: probe.0,
        target_known: probe.1.is_some(),
        target_oid: probe.1,
        other_operation_in_progress: probe.2,
    })
}

/// Resolve and preview a reset target without changing the repository.
pub async fn preview_reset(
    state: &AppState,
    repo_id: u64,
    target: ResetTarget,
) -> Result<augur_core::git::operation_probe::ResetPreview> {
    let repo = repository(state, repo_id)?;
    run_blocking(move || augur_core::git::operation_probe::preview_reset(&repo, &target)).await
}

/// Read a commit message without touching the working tree, for the clipboard.
pub async fn read_commit_message(
    state: &AppState,
    repo_id: u64,
    oid: String,
) -> Result<CommitMessage> {
    let repo = repository(state, repo_id)?;
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

/// Run blocking Git work off the caller's async runtime, for closures that
/// fail with a bare detail string. `err-git-run` carries the detail, and Git's
/// own explanation is the only useful thing in the message.
pub async fn run_blocking<T, F>(body: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> std::result::Result<T, String> + Send + 'static,
{
    run_blocking_typed(move || body().map_err(|detail| CommandError::new("err-git-run", detail)))
        .await
}

/// Run blocking Git work whose failure is already a [`CommandError`].
pub async fn run_blocking_typed<T, F>(body: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(body)
        .await
        // The blocking pool itself failed, which is not Git's fault and has no
        // detail worth showing.
        .map_err(|error| CommandError::new("err-worker", error.to_string()))?
}

/// Lane layout and ref labels for one page of commits.
///
/// The layout stays in Rust so it is literally the same algorithm the
/// reference application draws with, rather than a second implementation that
/// could drift. A page is 500 commits, so one call per page is negligible.
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
/// The thresholds live in one place, so the frontend reveals columns at exactly
/// the width the reference application does.
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
pub fn set_graph_history(state: &AppState, preference: GraphHistoryPreference) -> Result<()> {
    state.update_settings(|settings| settings.config.view.graph_history = preference);
    Ok(())
}
