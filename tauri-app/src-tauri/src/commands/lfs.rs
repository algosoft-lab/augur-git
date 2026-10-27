//! Typed Git LFS commands: thin Tauri wrappers over the shared service layer.

use augur_service::commands::lfs as service;
use augur_service::error::Result;
use augur_service::state::AppState;
use tauri::State;

use augur_core::lfs::{LfsEnvironment, LfsLock, LfsRepositoryPreferences, LfsRepositoryState};

pub use augur_service::askpass::CredentialRequestEvent;
pub use augur_service::clone::CloneProgressEvent;
pub use augur_service::commands::lfs::{LfsProgressEnvelope, LfsSnapshot};

#[tauri::command]
pub async fn lfs_snapshot(state: State<'_, AppState>, repo_id: u64) -> Result<LfsSnapshot> {
    service::lfs_snapshot(&state, repo_id).await
}

#[tauri::command]
pub fn lfs_environment() -> LfsEnvironment {
    service::lfs_environment()
}

#[tauri::command]
pub fn lfs_credential_response(request_id: String, response: Option<String>) -> bool {
    service::lfs_credential_response(request_id, response)
}

#[tauri::command]
pub async fn lfs_initialize(state: State<'_, AppState>, repo_id: u64) -> Result<LfsRepositoryState> {
    service::lfs_initialize(&state, repo_id).await
}

#[tauri::command]
pub async fn lfs_fetch(
    state: State<'_, AppState>,
    repo_id: u64,
    operation_id: String,
    all_history: bool,
    remote: Option<String>,
    reference: Option<String>,
) -> Result<String> {
    service::lfs_fetch(&state, repo_id, operation_id, all_history, remote, reference).await
}

#[tauri::command]
pub async fn lfs_fsck(state: State<'_, AppState>, repo_id: u64) -> Result<String> {
    service::lfs_fsck(&state, repo_id).await
}

#[tauri::command]
pub fn lfs_cancel(operation_id: String) -> bool {
    service::lfs_cancel(operation_id)
}

#[tauri::command]
pub async fn lfs_download_file(
    state: State<'_, AppState>,
    repo_id: u64,
    path: String,
) -> Result<()> {
    service::lfs_download_file(&state, repo_id, path).await
}

#[tauri::command]
pub async fn lfs_track(
    state: State<'_, AppState>,
    repo_id: u64,
    pattern: String,
    lockable: bool,
) -> Result<()> {
    service::lfs_track(&state, repo_id, pattern, lockable).await
}

#[tauri::command]
pub async fn lfs_untrack(state: State<'_, AppState>, repo_id: u64, pattern: String) -> Result<()> {
    service::lfs_untrack(&state, repo_id, pattern).await
}

#[tauri::command]
pub async fn lfs_locks(state: State<'_, AppState>, repo_id: u64) -> Result<Vec<LfsLock>> {
    service::lfs_locks(&state, repo_id).await
}

#[tauri::command]
pub async fn lfs_lock(state: State<'_, AppState>, repo_id: u64, path: String) -> Result<()> {
    service::lfs_lock(&state, repo_id, path).await
}

#[tauri::command]
pub async fn lfs_unlock(
    state: State<'_, AppState>,
    repo_id: u64,
    id: String,
    force: bool,
) -> Result<()> {
    service::lfs_unlock(&state, repo_id, id, force).await
}

#[tauri::command]
pub async fn lfs_cache_preview(
    state: State<'_, AppState>,
    repo_id: u64,
) -> Result<augur_core::lfs::LfsCachePreview> {
    service::lfs_cache_preview(&state, repo_id).await
}

#[tauri::command]
pub async fn lfs_prune(state: State<'_, AppState>, repo_id: u64) -> Result<String> {
    service::lfs_prune(&state, repo_id).await
}

#[tauri::command]
pub async fn lfs_update_preferences(
    state: State<'_, AppState>,
    repo_id: u64,
    preferences: LfsRepositoryPreferences,
) -> Result<LfsRepositoryState> {
    service::lfs_update_preferences(&state, repo_id, preferences).await
}
