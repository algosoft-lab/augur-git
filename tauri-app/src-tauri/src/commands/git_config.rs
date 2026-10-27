//! Global Git configuration commands: thin Tauri wrappers over the shared
//! service layer.

use augur_core::config::LocationConfig;
use augur_core::git::config_access::{
    self, CommitIdentityProbe, GitConfigEntry, GitConfigSnapshot,
};
use augur_service::error::Result;
use augur_service::state::AppState;
use tauri::State;

/// The global configuration entries the settings page edits.
#[tauri::command]
pub fn read_git_config(location: Option<LocationConfig>) -> GitConfigSnapshot {
    config_access::read_global_all(location.as_ref())
}

/// Validate and apply configuration writes, then return the new snapshot.
///
/// The optional location lets the commit-identity dialog write into a WSL
/// distro's own global configuration; the settings page omits it and edits the
/// host git.
#[tauri::command]
pub fn write_git_config(
    entries: Vec<GitConfigEntry>,
    location: Option<LocationConfig>,
) -> Result<GitConfigSnapshot> {
    augur_service::commands::git_config::write_git_config(entries, location)
}

/// The identity Git would use for a commit in this repository.
///
/// Async for the same reason as the other probes: the `git config` child
/// processes must not block the webview thread.
#[tauri::command]
pub async fn probe_commit_identity(
    state: State<'_, AppState>,
    repo_id: u64,
) -> Result<CommitIdentityProbe> {
    augur_service::commands::git_config::probe_commit_identity(&state, repo_id).await
}
