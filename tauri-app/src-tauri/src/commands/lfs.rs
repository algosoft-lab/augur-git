//! Typed Git LFS commands exposed to the webview.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, State};

use augur_core::git::GitRepo;
use augur_core::lfs::{
    self, LfsEnvironment, LfsFile, LfsLock, LfsRepositoryPreferences, LfsRepositoryState,
};

use crate::commands::repo::CommandError;
use crate::state::AppState;

type Result<T> = std::result::Result<T, CommandError>;
static ACTIVE_TRANSFERS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn repository(state: &AppState, repo_id: u64) -> Result<GitRepo> {
    state
        .with_repo(repo_id, |session| session.repo().clone())
        .ok_or_else(|| CommandError::missing_repo(repo_id))
}

fn lfs_error(detail: impl Into<String>) -> CommandError {
    CommandError::new("err-lfs-operation", detail)
}

fn with_repository_lock<T>(
    repo: &GitRepo,
    operation: impl FnOnce() -> std::result::Result<T, String>,
) -> std::result::Result<T, String> {
    let lock = lfs::operation_lock(repo);
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    operation()
}

#[tauri::command]
pub async fn lfs_snapshot(state: State<'_, AppState>, repo_id: u64) -> Result<LfsSnapshot> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let environment = lfs::probe(&repo);
        let repository = lfs::repository_state(&repo);
        let files = if repository.uses_lfs {
            lfs::files(&repo, None).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(LfsSnapshot {
            environment,
            repository,
            files,
        })
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsSnapshot {
    pub environment: LfsEnvironment,
    pub repository: LfsRepositoryState,
    pub files: Vec<LfsFile>,
}

#[tauri::command]
pub fn lfs_environment() -> LfsEnvironment {
    lfs::probe(&GitRepo::local("."))
}

#[tauri::command]
pub fn lfs_credential_response(request_id: String, response: Option<String>) -> bool {
    crate::askpass::respond(&request_id, response)
}

#[tauri::command]
pub async fn lfs_initialize(
    state: State<'_, AppState>,
    repo_id: u64,
) -> Result<LfsRepositoryState> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::initialize(&repo)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_fetch(
    app: AppHandle,
    state: State<'_, AppState>,
    repo_id: u64,
    operation_id: String,
    all_history: bool,
    remote: Option<String>,
    reference: Option<String>,
) -> Result<String> {
    validate_operation_id(&operation_id)?;
    if all_history && (remote.is_some() || reference.is_some()) {
        return Err(lfs_error(
            "Full-history downloads cannot be combined with a selected reference.",
        ));
    }
    if remote.is_some() != reference.is_some() {
        return Err(lfs_error(
            "Choose both a remote and a reference, or leave both empty.",
        ));
    }
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let transfers = ACTIVE_TRANSFERS.get_or_init(|| Mutex::new(HashMap::new()));
            let mut transfers = transfers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if transfers.contains_key(&operation_id) {
                return Err(lfs_error("An LFS transfer with this operation id is already active."));
            }
            transfers.insert(operation_id.clone(), cancel.clone());
        }
        let result = with_repository_lock(&repo, || {
            let expected_head = lfs::repository_state(&repo).head;
            let report = |progress| {
                let _ = app.emit(
                    crate::events::LFS_PROGRESS_EVENT,
                    LfsProgressEnvelope {
                        repo_id,
                        operation_id: operation_id.clone(),
                        progress,
                    },
                );
            };
            if all_history {
                lfs::fetch_all_cancellable(&repo, &cancel, report)?;
            } else if let (Some(remote), Some(reference)) = (remote.as_deref(), reference.as_deref())
            {
                lfs::fetch_reference_cancellable(&repo, remote, reference, &cancel, report)?;
            } else {
                lfs::fetch_cancellable(&repo, &cancel, report)?;
            }
            if lfs::repository_state(&repo).head != expected_head {
                return Err("The checked out commit changed while LFS objects were downloading. No files were populated; retry for the current branch.".into());
            }
            lfs::checkout(&repo)?;
            Ok("Git LFS files are up to date.".to_string())
        })
        .map_err(lfs_error);
        ACTIVE_TRANSFERS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&operation_id);
        result
    }).await.map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_fsck(state: State<'_, AppState>, repo_id: u64) -> Result<String> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::fsck(&repo)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub fn lfs_cancel(operation_id: String) -> bool {
    ACTIVE_TRANSFERS
        .get()
        .and_then(|transfers| {
            transfers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&operation_id)
                .cloned()
        })
        .is_some_and(|cancel| {
            cancel.store(true, Ordering::Release);
            true
        })
}

fn validate_operation_id(operation_id: &str) -> Result<()> {
    if operation_id.is_empty()
        || operation_id.len() > 128
        || operation_id.chars().any(char::is_control)
    {
        Err(lfs_error("Invalid LFS operation id."))
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn lfs_download_file(
    state: State<'_, AppState>,
    repo_id: u64,
    path: String,
) -> Result<()> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::download_file(&repo, &path)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsProgressEnvelope {
    pub repo_id: u64,
    pub operation_id: String,
    pub progress: lfs::LfsTransferProgress,
}

#[tauri::command]
pub async fn lfs_track(
    state: State<'_, AppState>,
    repo_id: u64,
    pattern: String,
    lockable: bool,
) -> Result<()> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::track(&repo, &pattern, lockable)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_untrack(state: State<'_, AppState>, repo_id: u64, pattern: String) -> Result<()> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::untrack(&repo, &pattern)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_locks(state: State<'_, AppState>, repo_id: u64) -> Result<Vec<LfsLock>> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || lfs::locks(&repo, 200).map_err(lfs_error))
        .await
        .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_lock(state: State<'_, AppState>, repo_id: u64, path: String) -> Result<()> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::lock(&repo, &path)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_unlock(
    state: State<'_, AppState>,
    repo_id: u64,
    id: String,
    force: bool,
) -> Result<()> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::unlock(&repo, &id, force)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_cache_preview(
    state: State<'_, AppState>,
    repo_id: u64,
) -> Result<lfs::LfsCachePreview> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || lfs::cache_preview(&repo).map_err(lfs_error))
        .await
        .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_prune(state: State<'_, AppState>, repo_id: u64) -> Result<String> {
    let repo = repository(&state, repo_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || lfs::prune(&repo)).map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))?
}

#[tauri::command]
pub async fn lfs_update_preferences(
    state: State<'_, AppState>,
    repo_id: u64,
    preferences: LfsRepositoryPreferences,
) -> Result<LfsRepositoryState> {
    let repo = repository(&state, repo_id)?;
    let automatic_download = preferences.automatic_download;
    let preference_key = lfs::repository_key(&repo);
    let result = tauri::async_runtime::spawn_blocking(move || {
        with_repository_lock(&repo, || {
            lfs::apply_preferences(&repo, &preferences)?;
            Ok(lfs::repository_state(&repo))
        })
        .map_err(lfs_error)
    })
    .await
    .map_err(|error| lfs_error(error.to_string()))??;
    if let Some(enabled) = automatic_download {
        state.update_settings(|settings| {
            settings
                .config
                .lfs_repository_auto_download
                .insert(preference_key, enabled);
        });
    }
    Ok(result)
}
