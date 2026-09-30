//! Application-level commands: settings, layout, shortcuts, windows, and
//! platform helpers.

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use augur_core::config::{
    AppConfig, CommitActionPreference, DiffLayoutPreference, LanguagePreference, LayoutSettings,
    LocationConfig, ThemePreference, TypographySettings, ViewSettings, WindowBounds, WindowMode,
};
use augur_core::git::{GitError, GitRepo};
use augur_core::keymap;

use crate::commands::repo::CommandError;
use crate::events::{AppEvent, OPEN_PATHS_EVENT, OpenPathsPayload, SETTINGS_NAVIGATE_EVENT};
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

fn window_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::new("err-window", error.to_string())
}

fn logical_bounds(window: &WebviewWindow) -> Result<WindowBounds> {
    let scale = window.scale_factor().map_err(window_error)?;
    let position = window.outer_position().map_err(window_error)?;
    let size = window.inner_size().map_err(window_error)?;
    Ok(WindowBounds {
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
    })
}

fn monitor_work_area(window: &WebviewWindow) -> Option<WindowBounds> {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    Some(WindowBounds {
        x: f64::from(work_area.position.x) / scale,
        y: f64::from(work_area.position.y) / scale,
        width: f64::from(work_area.size.width) / scale,
        height: f64::from(work_area.size.height) / scale,
    })
}

fn clamp_window_bounds(
    bounds: WindowBounds,
    mode: WindowMode,
    work_area: Option<WindowBounds>,
) -> WindowBounds {
    let (minimum_width, maximum_width, minimum_height) = match mode {
        WindowMode::Desktop => (860.0, 8192.0, 480.0),
        WindowMode::Sidecar => (360.0, 520.0, 480.0),
    };
    let Some(area) = work_area else {
        return WindowBounds {
            width: bounds.width.max(minimum_width),
            height: bounds.height.max(minimum_height),
            ..bounds
        };
    };
    let width = bounds
        .width
        .clamp(minimum_width, maximum_width)
        .min(area.width.max(minimum_width));
    let height = bounds
        .height
        .max(minimum_height)
        .min(area.height.max(minimum_height));
    WindowBounds {
        x: bounds
            .x
            .clamp(area.x, (area.x + area.width - width).max(area.x)),
        y: bounds
            .y
            .clamp(area.y, (area.y + area.height - height).max(area.y)),
        width,
        height,
    }
}

fn first_sidecar_bounds(window: &WebviewWindow) -> WindowBounds {
    if let Some(area) = monitor_work_area(window) {
        let width = 420.0_f64.min(area.width.max(360.0));
        let height = (area.height - 16.0).clamp(480.0, 900.0);
        WindowBounds {
            x: (area.x + area.width - width - 8.0).max(area.x),
            y: area.y + 8.0,
            width,
            height,
        }
    } else {
        WindowBounds {
            x: 0.0,
            y: 0.0,
            width: 420.0,
            height: 760.0,
        }
    }
}

fn set_window_geometry(window: &WebviewWindow, bounds: WindowBounds) -> Result<()> {
    window
        .set_size(LogicalSize::new(bounds.width, bounds.height))
        .map_err(window_error)?;
    window
        .set_position(LogicalPosition::new(bounds.x, bounds.y))
        .map_err(window_error)
}

fn save_mode_bounds(workspace: &mut augur_core::config::WorkspaceState, bounds: WindowBounds) {
    match workspace.window_mode {
        WindowMode::Desktop => workspace.desktop_window = Some(bounds),
        WindowMode::Sidecar => workspace.sidecar_window = Some(bounds),
    }
}

fn apply_mode_minimum(window: &WebviewWindow, mode: WindowMode) -> Result<()> {
    let (minimum, maximum) = match mode {
        WindowMode::Desktop => (860.0, None),
        WindowMode::Sidecar => (360.0, Some(520.0)),
    };
    window
        .set_min_size(Some(LogicalSize::new(minimum, 480.0)))
        .map_err(window_error)?;
    window
        .set_max_size(maximum.map(|width| LogicalSize::new(width, 8192.0)))
        .map_err(window_error)
}

pub fn restore_main_window(app: &AppHandle, state: &AppState) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let mut workspace = state.workspace();
    if let Err(error) = apply_mode_minimum(&window, workspace.window_mode) {
        log::warn!("[window] failed to set the initial minimum size: {error}");
    }
    let saved = match workspace.window_mode {
        WindowMode::Desktop => workspace.desktop_window,
        WindowMode::Sidecar => workspace.sidecar_window,
    };
    let bounds = saved
        .or_else(|| {
            (workspace.window_mode == WindowMode::Sidecar).then(|| first_sidecar_bounds(&window))
        })
        .or_else(|| logical_bounds(&window).ok());
    if let Some(bounds) = bounds {
        let bounds = clamp_window_bounds(bounds, workspace.window_mode, monitor_work_area(&window));
        if let Err(error) = set_window_geometry(&window, bounds) {
            log::warn!("[window] failed to restore the initial geometry: {error}");
        }
        save_mode_bounds(&mut workspace, bounds);
        state.update_workspace(|stored| *stored = workspace);
    }
}

/// The bundled themes, in settings-list order.
#[tauri::command]
pub fn theme_options() -> Vec<ThemePreference> {
    ThemePreference::ALL.to_vec()
}

/// Font families available on this machine, for the appearance settings.
#[tauri::command]
pub fn list_font_families() -> Vec<String> {
    crate::fonts::font_families()
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

/// Limit ongoing automatic refresh to the repository shown in the main tab.
#[tauri::command]
pub fn set_auto_refresh_target(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    repo_id: Option<u64>,
    generation: u64,
) -> Result<()> {
    if window.label() != "main" {
        return Err(CommandError::new(
            "err-window",
            "only the main window can select an automatic refresh target",
        ));
    }
    state
        .set_auto_refresh_target(repo_id, generation)
        .map_err(|detail| CommandError::new("err-repo-closed", detail))
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

#[tauri::command]
pub async fn set_window_mode(
    window: WebviewWindow,
    state: State<'_, AppState>,
    mode: WindowMode,
) -> Result<augur_core::config::WorkspaceState> {
    if window.label() != "main" {
        return Err(CommandError::new(
            "err-window",
            "only the main window can change its layout mode",
        ));
    }

    let mut workspace = state.workspace();
    let is_maximized = window.is_maximized().map_err(window_error)?;
    let is_fullscreen = window.is_fullscreen().map_err(window_error)?;
    if !is_maximized && !is_fullscreen {
        let bounds = logical_bounds(&window)?;
        save_mode_bounds(&mut workspace, bounds);
    }

    if workspace.window_mode != mode {
        if is_fullscreen {
            window.set_fullscreen(false).map_err(window_error)?;
        }
        if is_maximized {
            window.unmaximize().map_err(window_error)?;
        }

        let saved = match mode {
            WindowMode::Desktop => workspace.desktop_window,
            WindowMode::Sidecar => workspace.sidecar_window,
        };
        let target = saved
            .or_else(|| (mode == WindowMode::Sidecar).then(|| first_sidecar_bounds(&window)))
            .or_else(|| logical_bounds(&window).ok())
            .unwrap_or(WindowBounds {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 800.0,
            });
        let target = clamp_window_bounds(target, mode, monitor_work_area(&window));
        apply_mode_minimum(&window, mode)?;
        set_window_geometry(&window, target)?;
        workspace.window_mode = mode;
        save_mode_bounds(&mut workspace, target);
    }

    let stored = workspace.clone();
    state.update_workspace(|workspace| *workspace = stored);
    state.persistence().notify(AppEvent::WorkspaceChanged);
    Ok(workspace)
}

#[tauri::command]
pub fn save_window_bounds(window: WebviewWindow, state: State<'_, AppState>) -> Result<()> {
    if window.label() != "main" {
        return Err(CommandError::new(
            "err-window",
            "only the main window can save its geometry",
        ));
    }
    if window.is_maximized().map_err(window_error)?
        || window.is_fullscreen().map_err(window_error)?
    {
        return Ok(());
    }
    let mut bounds = logical_bounds(&window)?;
    let workspace = state.workspace();
    bounds = clamp_window_bounds(bounds, workspace.window_mode, monitor_work_area(&window));
    state.update_workspace(|workspace| save_mode_bounds(workspace, bounds));
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
///
/// Async so the window is created off the webview UI thread: on Windows a
/// synchronous command runs reentrantly inside `WebMessageReceived`, and the
/// nested message pump wry uses while creating the WebView2 controller leaves
/// the controller's widget tree uninitialized — a blank, uncloseable window.
#[tauri::command]
pub async fn open_about_window(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    let label = "about";
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let title = resolved_title(&state.settings().config.language, "app-name");
    let builder = WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::App("index.html?window=about".into()),
    )
    .title(title)
    .inner_size(460.0, 600.0)
    .min_inner_size(420.0, 500.0)
    .resizable(true);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder
        .build()
        .map_err(|error| CommandError::new("err-window", error.to_string()))?;
    Ok(())
}

/// A destination section for opening or navigating the settings window.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingsSection {
    General,
    Appearance,
    Layout,
    Shortcuts,
}

impl SettingsSection {
    fn query_value(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Layout => "layout",
            Self::Shortcuts => "shortcuts",
        }
    }
}

#[derive(Clone, serde::Serialize)]
struct SettingsNavigation {
    section: SettingsSection,
}

/// The settings window, opened once and focused on repeat requests.
///
/// Async for the same Windows reentrancy reason as `open_about_window`.
#[tauri::command]
pub async fn open_settings_window(
    app: AppHandle,
    state: State<'_, AppState>,
    section: Option<SettingsSection>,
) -> Result<()> {
    let label = "settings";
    if let Some(window) = app.get_webview_window(label) {
        if let Some(section) = section {
            let _ = window.emit(SETTINGS_NAVIGATE_EVENT, SettingsNavigation { section });
        }
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let title = resolved_title(&state.settings().config.language, "settings-title");
    let url = section.map_or_else(
        || "index.html?window=settings".to_string(),
        |section| {
            format!(
                "index.html?window=settings&section={}",
                section.query_value()
            )
        },
    );
    let builder = WebviewWindowBuilder::new(&app, label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(780.0, 560.0)
        .min_inner_size(780.0, 560.0)
        .resizable(false)
        .decorations(cfg!(target_os = "macos"));
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
///
/// Async for the same Windows reentrancy reason as `open_about_window`.
#[tauri::command]
pub async fn open_compare_window(
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
    let title = resolved_title(&state.settings().config.language, "compare-window-title");
    let url = WebviewUrl::App(format!("index.html?window=compare&repo={repo_id}").into());
    let builder = WebviewWindowBuilder::new(&app, &label, url)
        .title(title)
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
    let _ = app
        .get_webview_window("main")
        .map(|window| window.emit(OPEN_PATHS_EVENT, OpenPathsPayload { paths }));
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
    fn sidecar_bounds_clamp_size_and_position_to_the_monitor() {
        let bounds = clamp_window_bounds(
            WindowBounds {
                x: 2_000.0,
                y: -500.0,
                width: 640.0,
                height: 1_200.0,
            },
            WindowMode::Sidecar,
            Some(WindowBounds {
                x: 100.0,
                y: 50.0,
                width: 1_200.0,
                height: 900.0,
            }),
        );

        assert_eq!(
            bounds,
            WindowBounds {
                x: 780.0,
                y: 50.0,
                width: 520.0,
                height: 900.0,
            }
        );
    }

    #[test]
    fn desktop_bounds_keep_the_minimum_width_on_a_secondary_monitor() {
        let bounds = clamp_window_bounds(
            WindowBounds {
                x: -200.0,
                y: 1_500.0,
                width: 700.0,
                height: 400.0,
            },
            WindowMode::Desktop,
            Some(WindowBounds {
                x: -1_920.0,
                y: 0.0,
                width: 1_280.0,
                height: 720.0,
            }),
        );

        assert_eq!(
            bounds,
            WindowBounds {
                x: -1_500.0,
                y: 240.0,
                width: 860.0,
                height: 480.0,
            }
        );
    }

    #[test]
    fn auxiliary_window_titles_come_from_the_catalog() {
        assert_eq!(
            resolved_title(&LanguagePreference::English, "compare-window-title"),
            "Compare revisions"
        );
        assert_eq!(
            resolved_title(
                &LanguagePreference::SimplifiedChinese,
                "compare-window-title"
            ),
            "比较版本"
        );
        // The About window is titled as the product, which is what the main
        // window's own title in tauri.conf.json says.
        assert_eq!(
            resolved_title(&LanguagePreference::English, "app-name"),
            "Augur Git"
        );
    }

    /// Commands that build a webview window must stay async.
    ///
    /// On Windows a synchronous command runs reentrantly inside the webview's
    /// `WebMessageReceived` callback, and the window created from that context
    /// is born with an invisible, zero-sized WebView2 controller: it shows a
    /// blank page nothing can navigate. Nothing in the type system enforces
    /// the async declaration, so the source shape is asserted instead.
    #[test]
    fn window_building_commands_are_async() {
        let source = include_str!("app.rs");
        for name in [
            "open_about_window",
            "open_settings_window",
            "open_compare_window",
        ] {
            let declaration = format!("pub async fn {name}");
            assert!(
                source.contains(&declaration),
                "{name} must be declared async: a synchronous window-creating command breaks the webview on Windows"
            );
        }
    }
}
