//! Application-level commands: settings, layout, shortcuts, windows, the
//! shell CLI, and platform helpers.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use augur_core::config::{
    AppConfig, CommitActionPreference, DiffLayoutPreference, LanguagePreference, LayoutSettings,
    LocationConfig, ThemePreference, TypographySettings, ViewSettings,
};
use augur_core::git::{GitError, GitRepo};
use augur_core::keymap;
use augur_core::shell_install::{self, ChangeReport, Operation};

use crate::commands::repo::CommandError;
use crate::events::{AppEvent, OpenPathsPayload, OPEN_PATHS_EVENT};
use crate::state::AppState;

type Result<T> = std::result::Result<T, CommandError>;

/// The window title for a key, in the language the settings carry.
///
/// Auxiliary windows are created by commands, so their titles resolve at
/// creation time; a language change afterwards is picked up when the window is
/// next opened.
fn resolved_title(language: &LanguagePreference, key: &str) -> String {
    augur_core::i18n::text(augur_core::i18n::resolve(language), key)
}

/// The five bundled themes, in settings-list order.
#[tauri::command]
pub fn theme_options() -> Vec<ThemePreference> {
    ThemePreference::ALL.to_vec()
}

/// Font families available on this machine, for the appearance settings.
#[tauri::command]
pub fn list_font_families() -> Vec<String> {
    crate::fonts::font_families()
}

/// Which shell operation the File menu requested.
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CliOperation {
    Install,
    Remove,
}

/// Install or remove the `augurgit-tauri` shell command.
///
/// The core report is returned unchanged. It already serialises, and re-shaping
/// it here would be a second place to lose information: a successful removal
/// collapsed into the same outcome as an install, and a failure lost the reason
/// that makes it actionable.
#[tauri::command]
pub async fn run_cli_installer(operation: CliOperation) -> Result<ChangeReport> {
    let operation = match operation {
        CliOperation::Install => Operation::Install,
        CliOperation::Remove => Operation::Remove,
    };
    let report = tauri::async_runtime::spawn_blocking(move || match operation {
        Operation::Install => shell_install::install(),
        Operation::Remove => shell_install::remove(),
    })
    .await
    .map_err(|error| CommandError::new("err-installer", error.to_string()))?;
    Ok(report)
}

/// Installed WSL distribution names. Empty on other platforms.
#[tauri::command]
pub fn list_wsl_distros() -> Vec<String> {
    augur_core::git::list_wsl_distros()
}

/// Validate a WSL repository before opening it, using the same probe the
/// worker runs.
#[tauri::command]
pub fn probe_wsl_repository(distro: String, path: String) -> std::result::Result<(), GitError> {
    let repo: GitRepo = match LocationConfig::wsl(distro).to_repo(path) {
        Ok(repo) => repo,
        Err(error) => return Err(error),
    };
    augur_core::git::probe_wsl_repository(&repo)
}

/// Every settings field the appearance and general pages can change, sent as one
/// patch so a single round trip keeps the document consistent.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct SettingsPatch {
    pub language: Option<LanguagePreference>,
    pub theme: Option<ThemePreference>,
    pub view: Option<ViewSettings>,
    pub typography: Option<TypographySettings>,
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, patch: SettingsPatch) -> Result<()> {
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
    });
    Ok(())
}

/// Convenience setters for the single-value preferences, so the settings page
/// does not have to construct a full patch.
#[tauri::command]
pub fn set_language(state: State<'_, AppState>, language: LanguagePreference) {
    state.update_settings(|settings| settings.config.language = language);
}

#[tauri::command]
pub fn set_theme(state: State<'_, AppState>, theme: ThemePreference) {
    state.update_settings(|settings| settings.config.theme = theme);
}

#[tauri::command]
pub fn set_view(state: State<'_, AppState>, view: ViewSettings) {
    state.update_settings(|settings| settings.config.view = view);
}

#[tauri::command]
pub fn set_typography(state: State<'_, AppState>, typography: TypographySettings) {
    state.update_settings(|settings| settings.config.typography = typography);
}

#[tauri::command]
pub fn set_commit_action(state: State<'_, AppState>, action: CommitActionPreference) {
    state.update_settings(|settings| settings.config.view.commit_action = action);
}

#[tauri::command]
pub fn set_diff_layout(state: State<'_, AppState>, layout: DiffLayoutPreference) {
    state.update_settings(|settings| settings.config.view.diff_layout = layout);
}

/// Replace the shared pane geometry.
#[tauri::command]
pub fn set_layout(state: State<'_, AppState>, layout: LayoutSettings) -> Result<()> {
    state.update_workspace(|workspace| workspace.layout = layout);
    Ok(())
}

/// Record the current tab list and active tab.
#[tauri::command]
pub fn set_workspace_tabs(
    state: State<'_, AppState>,
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
#[tauri::command]
pub fn set_shortcut(
    state: State<'_, AppState>,
    command: String,
    keys: Option<Vec<String>>,
) -> Result<keymap::ShortcutState> {
    if !keymap::COMMANDS.contains(&command.as_str()) {
        return Err(CommandError::new(
            "err-unknown-command",
            format!("{command} is not a remappable command"),
        ));
    }
    for key in keys.iter().flatten() {
        if !keymap::is_valid_combo(key) {
            return Err(CommandError::new(
                "err-invalid-shortcut",
                format!("{key} is not a valid key combination"),
            ));
        }
    }
    state.update_settings(|settings| {
        match keys {
            Some(keys) => {
                settings.shortcuts.insert(command.clone(), keys);
            }
            None => {
                settings.shortcuts.remove(&command);
            }
        }
    });
    Ok(state.persistence().shortcut_state())
}

/// Validate a key combination without saving it.
#[tauri::command]
pub fn validate_shortcut(value: String) -> std::result::Result<Vec<String>, String> {
    keymap::parse_combo_list(&value)
}

/// Force both documents to disk.
#[tauri::command]
pub fn flush_state(state: State<'_, AppState>) {
    state.persistence().flush();
}

/// The About window, opened once and focused on repeat requests.
#[tauri::command]
pub fn open_about_window(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    let label = "about";
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let builder = WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::App("index.html?window=about".into()),
    )
    .title(resolved_title(&state.settings().config.language, "app-name"))
    .inner_size(400.0, 340.0)
    .min_inner_size(400.0, 340.0)
    .resizable(false);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder
        .build()
        .map_err(|error| CommandError::new("err-window", error.to_string()))?;
    Ok(())
}

/// The standalone comparison window for one repository.
#[tauri::command]
pub fn open_compare_window(
    app: AppHandle,
    state: State<'_, AppState>,
    repo_id: u64,
) -> Result<String> {
    let label = format!("compare-{repo_id}");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(label);
    }
    let url = WebviewUrl::App(format!("index.html?window=compare&repo={repo_id}").into());
    let builder = WebviewWindowBuilder::new(&app, &label, url)
        .title(resolved_title(&state.settings().config.language, "compare-window-title"))
        .inner_size(1280.0, 820.0)
        .min_inner_size(900.0, 560.0)
        .resizable(true)
        .decorations(cfg!(target_os = "macos"));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    let window = builder
        .build()
        .map_err(|error| CommandError::new("err-window", error.to_string()))?;
    // The window starts hidden and is revealed by the webview once the first
    // snapshot is on screen, which avoids a flash of empty content.
    let _ = window.show();
    Ok(label)
}

/// Close the comparison window of one repository.
#[tauri::command]
pub fn close_compare_window(app: AppHandle, repo_id: u64) {
    let label = format!("compare-{repo_id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.close().ok();
    }
}

/// Focus the main window, used when a second launch forwards paths.
#[tauri::command]
pub fn focus_main_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Ask the main window to open a repository. Used by the drag-and-drop and
/// context-menu entry points so the command runs in the window that owns the
/// tab list.
#[tauri::command]
pub fn request_open_paths(app: AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let _ = app.get_webview_window("main").map(|window| {
        window.emit(OPEN_PATHS_EVENT, OpenPathsPayload { paths })
    });
}

/// Collect the repository paths handed over before this window was listening.
///
/// Draining rather than reading, so a path that arrives while the collection is
/// in flight is not handed out twice.
#[tauri::command]
pub fn take_pending_paths(state: State<'_, AppState>) -> Vec<String> {
    state.take_pending_paths()
}

/// Report a notice to every window, for operations that need no further work.
#[tauri::command]
pub fn notify(state: State<'_, AppState>, level: String, message: String) {
    state
        .persistence()
        .notify(AppEvent::Notice { level, message });
}

/// Ask the main window to re-read the active tab after the window regains
/// focus. Returns the repository that should be refreshed.
#[tauri::command]
pub fn repository_summary(state: State<'_, AppState>, repo_id: u64) -> Option<RepoInfo> {
    state.with_repo(repo_id, |session| RepoInfo {
        id: session.id(),
        path: session.path().to_string(),
        location: session.location().clone(),
    })
}

/// Minimal repository description used by window bootstraps.
#[derive(Clone, Debug, Serialize)]
pub struct RepoInfo {
    pub id: u64,
    pub path: String,
    pub location: LocationConfig,
}

/// The current preferences, for windows that opened after a settings change.
#[tauri::command]
pub fn current_config(state: State<'_, AppState>) -> AppConfig {
    state.config()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auxiliary_window_titles_come_from_the_catalog() {
        assert_eq!(
            resolved_title(&LanguagePreference::English, "compare-window-title"),
            "Compare revisions"
        );
        assert_eq!(
            resolved_title(&LanguagePreference::SimplifiedChinese, "compare-window-title"),
            "比较版本"
        );
        // The About window is titled as the product, which is what the main
        // window's own title in tauri.conf.json says.
        assert_eq!(
            resolved_title(&LanguagePreference::English, "app-name"),
            "Augur Git Tauri"
        );
    }
}
