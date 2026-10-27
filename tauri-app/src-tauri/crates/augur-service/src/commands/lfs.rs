//! Typed Git LFS service commands.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;

use augur_core::git::GitRepo;
use augur_core::lfs::{
    self, LfsEnvironment, LfsFile, LfsLock, LfsRepositoryPreferences, LfsRepositoryState,
};

use crate::commands::clone_cmd::CloneResult;
use crate::error::{CommandError, Result};
use crate::events::LFS_PROGRESS_EVENT;
use crate::sink::{EventTarget, emit};
use crate::state::AppState;

static ACTIVE_TRANSFERS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsProgressEnvelope {
    pub repo_id: u64,
    pub operation_id: String,
    pub progress: lfs::LfsTransferProgress,
}

pub fn lfs_error(detail: impl Into<String>) -> CommandError {
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

pub async fn lfs_snapshot(state: &AppState, repo_id: u64) -> Result<LfsSnapshot> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking(move || {
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
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsSnapshot {
    pub environment: LfsEnvironment,
    pub repository: LfsRepositoryState,
    pub files: Vec<LfsFile>,
}

pub fn lfs_environment() -> LfsEnvironment {
    lfs::probe(&GitRepo::local("."))
}

pub fn lfs_credential_response(request_id: String, response: Option<String>) -> bool {
    crate::askpass::respond(&request_id, response)
}

pub async fn lfs_initialize(state: &AppState, repo_id: u64) -> Result<LfsRepositoryState> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::initialize(&repo)).map_err(lfs_error)
    })
    .await
}

pub async fn lfs_fetch(
    state: &AppState,
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
    let repo = super::repo::repository(state, repo_id)?;
    let sink = state.sink().clone();
    crate::commands::repo::run_blocking_typed(move || {
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
                emit(
                    sink.as_ref(),
                    EventTarget::All,
                    LFS_PROGRESS_EVENT,
                    &LfsProgressEnvelope {
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
    })
    .await
}

pub async fn lfs_fsck(state: &AppState, repo_id: u64) -> Result<String> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::fsck(&repo)).map_err(lfs_error)
    })
    .await
}

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

pub async fn lfs_download_file(state: &AppState, repo_id: u64, path: String) -> Result<()> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::download_file(&repo, &path)).map_err(lfs_error)
    })
    .await
}

pub async fn lfs_track(
    state: &AppState,
    repo_id: u64,
    pattern: String,
    lockable: bool,
) -> Result<()> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::track(&repo, &pattern, lockable)).map_err(lfs_error)
    })
    .await
}

pub async fn lfs_untrack(state: &AppState, repo_id: u64, pattern: String) -> Result<()> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::untrack(&repo, &pattern)).map_err(lfs_error)
    })
    .await
}

pub async fn lfs_locks(state: &AppState, repo_id: u64) -> Result<Vec<LfsLock>> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || lfs::locks(&repo, 200).map_err(lfs_error)).await
}

pub async fn lfs_lock(state: &AppState, repo_id: u64, path: String) -> Result<()> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::lock(&repo, &path)).map_err(lfs_error)
    })
    .await
}

pub async fn lfs_unlock(state: &AppState, repo_id: u64, id: String, force: bool) -> Result<()> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::unlock(&repo, &id, force)).map_err(lfs_error)
    })
    .await
}

pub async fn lfs_cache_preview(
    state: &AppState,
    repo_id: u64,
) -> Result<lfs::LfsCachePreview> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || lfs::cache_preview(&repo).map_err(lfs_error)).await
}

pub async fn lfs_prune(state: &AppState, repo_id: u64) -> Result<String> {
    let repo = super::repo::repository(state, repo_id)?;
    crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || lfs::prune(&repo)).map_err(lfs_error)
    })
    .await
}

pub async fn lfs_update_preferences(
    state: &AppState,
    repo_id: u64,
    preferences: LfsRepositoryPreferences,
) -> Result<LfsRepositoryState> {
    let repo = super::repo::repository(state, repo_id)?;
    let automatic_download = preferences.automatic_download;
    let preference_key = lfs::repository_key(&repo);
    let result = crate::commands::repo::run_blocking_typed(move || {
        with_repository_lock(&repo, || {
            lfs::apply_preferences(&repo, &preferences)?;
            Ok(lfs::repository_state(&repo))
        })
        .map_err(lfs_error)
    })
    .await?;
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

/// Clone a repository over its URL, hydrate LFS, and open it as a new tab.
pub async fn lfs_clone(
    state: &AppState,
    remote: String,
    parent: String,
    directory_name: String,
    download_lfs: bool,
    operation_id: String,
) -> Result<CloneResult> {
    crate::commands::clone_cmd::validate_operation_id(&operation_id)?;
    let target = crate::commands::clone_cmd::validate_clone_target(&remote, &parent, &directory_name)?;
    let cancel = crate::commands::clone_cmd::register_task(&operation_id)?;

    let sink = state.sink().clone();
    let task_operation_id = operation_id.clone();
    let task_remote = remote.trim().to_string();
    let result = crate::commands::repo::run_blocking(move || {
        crate::commands::clone_cmd::clone_repository(
            &sink,
            &task_operation_id,
            &task_remote,
            &target,
            download_lfs,
            &cancel,
        )
    })
    .await;

    crate::commands::clone_cmd::unregister_task(&operation_id);
    let cloned = result?;
    let path = cloned.path.to_string_lossy().into_owned();
    let location = augur_core::config::LocationConfig::Local;
    let repo_id = state
        .open_repository(path.clone(), location)
        .map_err(CommandError::from)?;
    Ok(CloneResult {
        repo_id,
        path,
        uses_lfs: cloned.uses_lfs,
        lfs_available: cloned.lfs_available,
        lfs_error: cloned.lfs_error,
    })
}

pub fn cancel_lfs_clone(operation_id: String) -> bool {
    crate::commands::clone_cmd::cancel_task(&operation_id)
}
