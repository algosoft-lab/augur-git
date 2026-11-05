//! Application-level service commands: settings, layout, shortcuts, and
//! session data. Window management and platform helpers stay in the frontends.

use augur_core::config::{
    AppConfig, CommitActionPreference, DiffLayoutPreference, LanguagePreference, LayoutSettings,
    LocationConfig, ThemePreference, TypographySettings, ViewSettings,
};
use augur_core::keymap;

use crate::error::{CommandError, Result};
use crate::events::{AppEvent, OpenPathsPayload};
use crate::sink::{EventSink, EventTarget, emit};
use super::repo::RepoSummary;
use crate::state::AppState;

/// Every settings field the appearance and general pages can change, sent as one
/// patch so a single round trip keeps the document consistent.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct SettingsPatch {
    pub language: Option<LanguagePreference>,
    pub theme: Option<ThemePreference>,
    pub view: Option<ViewSettings>,
    pub typography: Option<TypographySettings>,
    pub lfs_auto_download: Option<bool>,
    pub lfs_download_on_open: Option<bool>,
}

pub fn update_settings(state: &AppState, patch: SettingsPatch) -> Result<()> {
    state.update_settings(|settings| {
        if let Some(language) = patch.language {
            settings.config.language = language;
        }
        if let Some(theme) = patch.theme {
            settings.config.theme = theme;
        }
        if let Some(view) = patch.view {
            settings.config.view = view;
        }
        if let Some(typography) = patch.typography {
            settings.config.typography = typography;
        }
        if let Some(enabled) = patch.lfs_auto_download {
            settings.config.lfs_auto_download = enabled;
        }
        if let Some(enabled) = patch.lfs_download_on_open {
            settings.config.lfs_download_on_open = enabled;
        }
    });
    Ok(())
}

pub fn set_lfs_download_preferences(state: &AppState, automatic: bool, on_open: bool) {
    state.update_settings(|settings| {
        settings.config.lfs_auto_download = automatic;
        settings.config.lfs_download_on_open = on_open;
    });
}

/// Convenience setters for the single-value preferences, so the settings page
/// does not have to construct a full patch.
pub fn set_language(state: &AppState, language: LanguagePreference) {
    state.update_settings(|settings| settings.config.language = language);
}

pub fn set_theme(state: &AppState, theme: ThemePreference) {
    state.update_settings(|settings| settings.config.theme = theme);
}

pub fn set_view(state: &AppState, view: ViewSettings) {
    state.update_settings(|settings| settings.config.view = view);
}

/// Limit ongoing automatic refresh to the repository shown in the primary tab.
pub fn set_auto_refresh_target(
    state: &AppState,
    repo_id: Option<u64>,
    generation: u64,
) -> Result<()> {
    state
        .set_auto_refresh_target(repo_id, generation)
        .map_err(|error| error)
}

pub fn set_typography(state: &AppState, typography: TypographySettings) {
    state.update_settings(|settings| settings.config.typography = typography);
}

pub fn set_commit_action(state: &AppState, action: CommitActionPreference) {
    state.update_settings(|settings| settings.config.view.commit_action = action);
}

pub fn set_diff_layout(state: &AppState, layout: DiffLayoutPreference) {
    state.update_settings(|settings| settings.config.view.diff_layout = layout);
}

/// Replace the shared pane geometry.
pub fn set_layout(state: &AppState, layout: LayoutSettings) -> Result<()> {
    state.update_workspace(|workspace| workspace.layout = layout);
    Ok(())
}

/// Record the current tab list and active tab.
pub fn set_workspace_tabs(
    state: &AppState,
    tabs: Vec<augur_core::config::OpenTabConfig>,
    active: Option<String>,
) -> Result<()> {
    state.update_workspace_visible(|workspace| {
        workspace.open_tabs = tabs;
        workspace.active_tab = active;
    });
    Ok(())
}

/// Replace the shortcut overrides for one command. An empty key list unbinds
/// the command.
pub fn set_shortcut(
    state: &AppState,
    command: String,
    keys: Option<Vec<String>>,
) -> Result<keymap::ShortcutState> {
    if !keymap::COMMANDS.contains(&command.as_str()) {
        return Err(CommandError::new(
            "err-unknown-command",
            format!("{command} is not a remappable command"),
        ));
    }
    let keys = keys
        .map(|keys| {
            keys.into_iter()
                .map(|key| {
                    keymap::normalize_combo(&key).ok_or_else(|| {
                        CommandError::new(
                            "err-invalid-shortcut",
                            format!("{key} is not a valid key combination"),
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let persistence = state.persistence();
    let mut user = keymap::file_from_overrides(&persistence.shortcuts());
    keymap::set_user_command(&mut user, &command, keys.clone());
    let resolved = keymap::resolve(&keymap::system_defaults(), &user, &keymap::COMMANDS);
    if let Some((left, right, key)) = keymap::find_conflict(&resolved) {
        return Err(CommandError::new(
            "err-shortcut-conflict",
            format!("{key} is assigned to both {left} and {right}"),
        ));
    }
    state.update_settings(|settings| match keys {
        Some(keys) => {
            settings.shortcuts.insert(command.clone(), keys);
        }
        None => {
            settings.shortcuts.remove(&command);
        }
    });
    Ok(state.persistence().shortcut_state())
}

/// Validate a key combination without saving it.
pub fn validate_shortcut(value: String) -> std::result::Result<Vec<String>, String> {
    keymap::parse_combo_list(&value)
}

/// Force both documents to disk.
pub fn flush_state(state: &AppState) {
    state.persistence().flush();
    // Usage timestamps are memory-only until a flush, so the same call that
    // saves the documents saves the credential document.
    state.credentials().flush();
}

/// Collect the repository paths handed over before this client was listening.
///
/// Draining rather than reading, so a path that arrives while the collection is
/// in flight is not handed out twice.
pub fn take_pending_paths(state: &AppState) -> Vec<String> {
    state.take_pending_paths()
}

/// Report a notice to every client, for operations that need no further work.
pub fn notify(state: &AppState, level: String, message: String) {
    state
        .persistence()
        .notify(AppEvent::Notice { level, message });
}

/// Hand repository paths to the primary client, queued as well as emitted.
pub fn deliver_open_paths(state: &AppState, sink: &dyn EventSink, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    state.queue_paths(paths.clone());
    emit(
        sink,
        EventTarget::Main,
        crate::events::OPEN_PATHS_EVENT,
        &OpenPathsPayload { paths },
    );
}

/// Minimal repository description used by window bootstraps.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RepoInfo {
    pub id: u64,
    pub path: String,
    pub location: LocationConfig,
}

/// The repository summary of one open repository.
pub fn repository_summary(state: &AppState, repo_id: u64) -> Option<RepoInfo> {
    state.with_repo(repo_id, |session| RepoInfo {
        id: session.id(),
        path: session.path().to_string(),
        location: session.location().clone(),
    })
}

/// One open repository as a summary, shared with the repo command set.
pub fn repository_summary_full(state: &AppState, repo_id: u64) -> Option<RepoSummary> {
    state.with_repo(repo_id, |session| RepoSummary {
        id: session.id(),
        path: session.path().to_string(),
        location: session.location().clone(),
    })
}

/// The current preferences, for clients that opened after a settings change.
pub fn current_config(state: &AppState) -> AppConfig {
    state.config()
}
