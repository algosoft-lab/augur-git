//! Agent prompt generation: thin Tauri wrapper over the shared service layer.

use augur_core::git::agent_prompt::AgentPromptRequest;
use augur_service::error::Result;
use augur_service::state::AppState;
use tauri::State;

/// Generate a provider-neutral prompt from a fresh, read-only Git snapshot.
#[tauri::command]
pub async fn generate_agent_prompt(
    state: State<'_, AppState>,
    repo_id: u64,
    request: AgentPromptRequest,
) -> Result<String> {
    augur_service::commands::agent_prompt::generate_agent_prompt(&state, repo_id, request).await
}
