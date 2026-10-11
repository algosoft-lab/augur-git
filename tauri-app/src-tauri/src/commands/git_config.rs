//! Global Git configuration commands.
//!
//! The keys the interface may touch are fixed by
//! [`augur_core::git::config_access`]; this module is only the Tauri boundary:
//! it validates, runs the allow-listed commands, and returns the refreshed
//! snapshot so the settings page never renders a state Git does not have.

use tauri::State;

use augur_core::config::LocationConfig;
use augur_core::git::config_access::{
    self, CommitIdentityProbe, GitConfigEntry, GitConfigSnapshot,
};

use crate::commands::repo::CommandError;
use crate::state::AppState;

type Result<T> = std::result::Result<T, CommandError>;

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
    config_access::apply_global(location.as_ref(), &entries)?;
    Ok(config_access::read_global_all(location.as_ref()))
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
    let repo = state
        .with_repo(repo_id, |session| session.repo().clone())
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;
    tauri::async_runtime::spawn_blocking(move || config_access::probe_commit_identity(&repo))
        .await
        .map_err(|error| CommandError::new("err-worker", error.to_string()))
}
