//! Native repository cloning: thin Tauri wrappers over the shared service
//! layer.

use augur_service::commands::lfs as lfs_service;
use augur_service::error::{CommandError, Result};
use augur_service::state::AppState;
use tauri::State;

pub use augur_service::clone::{CloneProgressEvent, CloneResult};

/// Clone a repository, hydrate its LFS objects, and open it as a new tab.
#[tauri::command]
pub async fn lfs_clone(
    state: State<'_, AppState>,
    remote: String,
    parent: String,
    directory_name: String,
    download_lfs: bool,
    operation_id: String,
) -> Result<CloneResult> {
    lfs_service::lfs_clone(
        &state,
        remote,
        parent,
        directory_name,
        download_lfs,
        operation_id,
    )
    .await
    .map_err(|error: CommandError| error)
}

/// Flag a running clone as cancelled.
#[tauri::command]
pub fn cancel_lfs_clone(operation_id: String) -> bool {
    lfs_service::cancel_lfs_clone(operation_id)
}
