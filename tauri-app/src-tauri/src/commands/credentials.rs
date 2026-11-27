//! Typed credential commands: thin Tauri wrappers over the shared service.

use augur_core::git::repo_credentials::RepoLocalCredentialInput;
use augur_service::commands::credentials as service;
use augur_service::commands::credentials::{
    CredentialLookup, ImportOutcome, RepoCredentialSnapshot, RepoCredentialVerification,
};
use augur_service::error::Result;
use augur_service::state::AppState;
use tauri::State;

pub use augur_service::askpass::{CredentialLoginAnswer, CredentialLoginResult};
pub use augur_service::credentials::model::{
    CredentialInput, CredentialSettingsPatch, CredentialSnapshot, CredentialTest,
};

#[tauri::command]
pub fn credential_snapshot(state: State<'_, AppState>) -> CredentialSnapshot {
    service::snapshot(&state)
}

#[tauri::command]
pub fn update_credential_settings(
    state: State<'_, AppState>,
    patch: CredentialSettingsPatch,
) -> Result<CredentialSnapshot> {
    service::update_settings(&state, patch)
}

#[tauri::command]
pub fn save_credential(
    state: State<'_, AppState>,
    input: CredentialInput,
) -> Result<CredentialSnapshot> {
    service::save_entry(&state, input)
}

#[tauri::command]
pub fn delete_credential(state: State<'_, AppState>, id: String) -> Result<CredentialSnapshot> {
    service::delete_entry(&state, id)
}

#[tauri::command]
pub fn credential_login_response(
    state: State<'_, AppState>,
    request_id: String,
    answer: CredentialLoginAnswer,
) -> CredentialLoginResult {
    service::login_response(&state, request_id, answer)
}

#[tauri::command]
pub fn credential_lookup(
    state: State<'_, AppState>,
    url: String,
    repo_id: Option<u64>,
) -> CredentialLookup {
    service::lookup(&state, url, repo_id)
}

#[tauri::command]
pub fn import_git_credentials(state: State<'_, AppState>) -> Result<ImportOutcome> {
    service::import_git_store(&state)
}

#[tauri::command]
pub async fn test_credential(state: State<'_, AppState>, id: String) -> Result<CredentialTest> {
    service::test_entry(&state, id).await
}

#[tauri::command]
pub async fn repo_credential_snapshot(
    state: State<'_, AppState>,
    repo_id: u64,
) -> Result<RepoCredentialSnapshot> {
    service::repo_snapshot(&state, repo_id).await
}

#[tauri::command]
pub async fn save_repo_credential_config(
    state: State<'_, AppState>,
    repo_id: u64,
    input: RepoLocalCredentialInput,
) -> Result<RepoCredentialSnapshot> {
    service::save_repo_config(&state, repo_id, input).await
}

#[tauri::command]
pub async fn verify_repo_credential(
    state: State<'_, AppState>,
    repo_id: u64,
    remote_name: String,
) -> Result<RepoCredentialVerification> {
    service::verify_repo_credential(&state, repo_id, remote_name).await
}

#[tauri::command]
pub async fn test_repo_remote_credential(
    state: State<'_, AppState>,
    repo_id: u64,
    remote_name: String,
) -> Result<CredentialTest> {
    service::test_repo_remote(&state, repo_id, remote_name).await
}
