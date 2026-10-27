//! Global Git configuration service commands.
//!
//! The keys the interface may touch are fixed by
//! [`augur_core::git::config_access`]; this module only validates, runs the
//! allow-listed operations, and returns the refreshed snapshot so the settings
//! page never renders a state Git does not have.

use augur_core::config::LocationConfig;
use augur_core::git::config_access::{
    self, CommitIdentityProbe, GitConfigEntry, GitConfigSnapshot,
};

use crate::error::Result;
use crate::state::AppState;

/// The global configuration entries the settings page edits.
pub fn read_git_config(location: Option<LocationConfig>) -> GitConfigSnapshot {
    config_access::read_global_all(location.as_ref())
}

/// Validate and apply configuration writes, then return the new snapshot.
///
/// The optional location lets the commit-identity dialog write into a WSL
/// distro's own global configuration; the settings page omits it and edits the
/// host git.
pub fn write_git_config(
    entries: Vec<GitConfigEntry>,
    location: Option<LocationConfig>,
) -> Result<GitConfigSnapshot> {
    config_access::apply_global(location.as_ref(), &entries)?;
    Ok(config_access::read_global_all(location.as_ref()))
}

/// The identity Git would use for a commit in this repository.
pub async fn probe_commit_identity(state: &AppState, repo_id: u64) -> Result<CommitIdentityProbe> {
    let repo = crate::commands::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking(move || Ok(config_access::probe_commit_identity(&repo))).await
}
