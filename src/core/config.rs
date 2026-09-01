//! Application configuration and persistence for repository and view settings.
//!
//! The configuration is stored under the platform's standard user config directory.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::agent::AgentSettings;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum LanguagePreference {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en-US", alias = "en")]
    English,
    #[serde(rename = "zh-CN", alias = "zh")]
    SimplifiedChinese,
}

impl Default for LanguagePreference {
    fn default() -> Self {
        Self::System
    }
}

/// User-selected UI theme. String values are stable config keys; the mapping
/// to registered theme names lives in `registry_name`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum ThemePreference {
    #[serde(rename = "github-dark")]
    GitHubDark,
    #[serde(rename = "catppuccin-latte")]
    CatppuccinLatte,
    #[serde(rename = "catppuccin-frappe")]
    CatppuccinFrappe,
    #[serde(rename = "catppuccin-macchiato")]
    CatppuccinMacchiato,
    #[serde(rename = "catppuccin-mocha")]
    CatppuccinMocha,
}

impl Default for ThemePreference {
    fn default() -> Self {
        Self::CatppuccinMocha
    }
}

impl ThemePreference {
    /// Theme name registered in `assets/themes/augur-themes.json` (the
    /// gpui-component theme registry key). Keep byte-identical with the JSON.
    pub const fn registry_name(self) -> &'static str {
        match self {
            Self::GitHubDark => "GitHub Dark",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinMocha => "Catppuccin Mocha",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OpenTabConfig {
    pub path: String,
}

/// Layout used to render commit diffs.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum DiffLayoutPreference {
    #[serde(rename = "inline")]
    Inline,
    #[serde(rename = "side-by-side")]
    SideBySide,
}

impl Default for DiffLayoutPreference {
    fn default() -> Self {
        Self::SideBySide
    }
}

/// History scope used by the commit graph.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum GraphHistoryPreference {
    #[serde(rename = "current-branch")]
    CurrentBranch,
    #[serde(rename = "all-branches")]
    AllBranches,
}

impl Default for GraphHistoryPreference {
    fn default() -> Self {
        // Match the VS Code commit graph default: show all branches,
        // including remote-branch divergence.
        Self::AllBranches
    }
}

/// Serde default for `ViewSettings::auto_refresh_on_focus`: config files
/// written before the field existed keep the feature enabled.
fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ViewSettings {
    pub show_untracked: bool,
    pub auto_follow: bool,
    #[serde(default)]
    pub diff_layout: DiffLayoutPreference,
    #[serde(default)]
    pub graph_history: GraphHistoryPreference,
    #[serde(default = "default_true")]
    pub auto_refresh_on_focus: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            show_untracked: true,
            auto_follow: true,
            diff_layout: DiffLayoutPreference::SideBySide,
            graph_history: GraphHistoryPreference::AllBranches,
            auto_refresh_on_focus: true,
        }
    }
}

pub const DEFAULT_UI_FONT_SIZE: f32 = 16.0;
pub const MIN_UI_FONT_SIZE: f32 = 12.0;
pub const MAX_UI_FONT_SIZE: f32 = 20.0;
pub const DEFAULT_DIFF_FONT_SIZE: f32 = 16.0;
pub const MIN_DIFF_FONT_SIZE: f32 = 12.0;
pub const MAX_DIFF_FONT_SIZE: f32 = 20.0;

/// Keep user-selected UI font sizes within the range supported by the layout.
pub fn normalized_ui_font_size(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE)
    } else {
        DEFAULT_UI_FONT_SIZE
    }
}

/// Keep user-selected Diff font sizes within the range supported by the layout.
pub fn normalized_diff_font_size(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_DIFF_FONT_SIZE, MAX_DIFF_FONT_SIZE)
    } else {
        DEFAULT_DIFF_FONT_SIZE
    }
}

/// User-selected font families and base UI/Diff font sizes. `None` keeps the
/// active theme/platform default and avoids serializing GPUI-specific types
/// into application config.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TypographySettings {
    #[serde(default)]
    pub ui_font_family: Option<String>,
    #[serde(default)]
    pub mono_font_family: Option<String>,
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: f32,
    #[serde(default = "default_diff_font_size")]
    pub diff_font_size: f32,
}

fn default_ui_font_size() -> f32 {
    DEFAULT_UI_FONT_SIZE
}

fn default_diff_font_size() -> f32 {
    DEFAULT_DIFF_FONT_SIZE
}

impl Default for TypographySettings {
    fn default() -> Self {
        Self {
            ui_font_family: None,
            mono_font_family: None,
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            diff_font_size: DEFAULT_DIFF_FONT_SIZE,
        }
    }
}

pub const DEFAULT_WINDOW_WIDTH: u32 = 1280;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 800;
pub const MIN_WINDOW_WIDTH: u32 = 860;
pub const MIN_WINDOW_HEIGHT: u32 = 480;

pub const MIN_SIDEBAR_WIDTH: f32 = 180.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 400.0;
pub const MIN_RIGHT_PANEL_WIDTH: f32 = 250.0;
pub const MAX_RIGHT_PANEL_WIDTH: f32 = 600.0;
pub const MIN_DIFF_HEIGHT: f32 = 100.0;
pub const MAX_DIFF_HEIGHT: f32 = 1000.0;
pub const DEFAULT_FILE_LIST_RATIO: f32 = 0.25;
pub const MIN_FILE_LIST_RATIO: f32 = 0.2;
pub const MAX_FILE_LIST_RATIO: f32 = 0.7;

/// Persisted geometry and panel layout shared by all repository tabs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LayoutSettings {
    pub sidebar_width: f32,
    pub right_panel_width: f32,
    pub diff_height: Option<f32>,
    pub file_list_ratio: f32,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            sidebar_width: 250.0,
            right_panel_width: 320.0,
            diff_height: None,
            file_list_ratio: DEFAULT_FILE_LIST_RATIO,
        }
    }
}

impl LayoutSettings {
    /// Clamp persisted or runtime values before applying them to a layout.
    pub fn normalize(&mut self) {
        self.sidebar_width =
            finite_or(self.sidebar_width, Self::default().sidebar_width)
                .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.right_panel_width = finite_or(
            self.right_panel_width,
            Self::default().right_panel_width,
        )
        .clamp(MIN_RIGHT_PANEL_WIDTH, MAX_RIGHT_PANEL_WIDTH);
        self.diff_height = self.diff_height.map(|height| {
            finite_or(height, Self::default().diff_height.unwrap_or(0.0))
                .clamp(MIN_DIFF_HEIGHT, MAX_DIFF_HEIGHT)
        });
        self.file_list_ratio =
            finite_or(self.file_list_ratio, DEFAULT_FILE_LIST_RATIO)
                .clamp(MIN_FILE_LIST_RATIO, MAX_FILE_LIST_RATIO);
    }
}

/// Window geometry stored independently from application preferences.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            maximized: false,
        }
    }
}

impl WindowState {
    pub fn normalize(&mut self) {
        self.width = self.width.max(MIN_WINDOW_WIDTH);
        self.height = self.height.max(MIN_WINDOW_HEIGHT);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct UiState {
    pub window: WindowState,
    pub layout: LayoutSettings,
}

impl UiState {
    pub fn normalize(&mut self) {
        self.window.normalize();
        self.layout.normalize();
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppConfig {
    /// UI theme; missing field falls back to Catppuccin Mocha.
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default)]
    pub language: LanguagePreference,
    #[serde(default)]
    pub open_tabs: Vec<OpenTabConfig>,
    #[serde(default)]
    pub active_tab_path: Option<String>,
    #[serde(default)]
    pub view: ViewSettings,
    #[serde(default)]
    pub typography: TypographySettings,
    #[serde(default)]
    pub recent_repos: Vec<String>,
    /// External Agent CLI profiles and executable overrides.
    #[serde(default)]
    pub agent: AgentSettings,
}

impl AppConfig {
    pub fn push_recent(&mut self, path: &str) {
        self.recent_repos.retain(|p| p != path);
        self.recent_repos.insert(0, path.to_string());
        self.recent_repos.truncate(8);
    }

    fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.open_tabs.retain(|tab| {
            if tab.path.is_empty() || seen.iter().any(|path| path == &tab.path)
            {
                false
            } else {
                seen.push(tab.path.clone());
                true
            }
        });

        if self.active_tab_path.as_ref().is_some_and(|path| {
            self.open_tabs.iter().all(|tab| &tab.path != path)
        }) {
            self.active_tab_path = None;
        }
        if self.active_tab_path.is_none() {
            self.active_tab_path =
                self.open_tabs.first().map(|tab| tab.path.clone());
        }
    }
}

#[derive(Deserialize, Default)]
struct RawAppConfig {
    #[serde(default)]
    theme: ThemePreference,
    #[serde(default)]
    language: LanguagePreference,
    #[serde(default)]
    open_tabs: Vec<OpenTabConfig>,
    #[serde(default)]
    active_tab_path: Option<String>,
    #[serde(default)]
    view: ViewSettings,
    #[serde(default)]
    typography: TypographySettings,
    #[serde(default)]
    recent_repos: Vec<String>,
    #[serde(default)]
    agent: AgentSettings,
    #[serde(default)]
    repo: LegacyRepoConfig,
}

#[derive(Deserialize, Default)]
struct LegacyRepoConfig {
    #[serde(default)]
    path: String,
}

impl From<RawAppConfig> for AppConfig {
    fn from(raw: RawAppConfig) -> Self {
        let mut config = Self {
            theme: raw.theme,
            language: raw.language,
            open_tabs: raw.open_tabs,
            active_tab_path: raw.active_tab_path,
            view: raw.view,
            typography: raw.typography,
            recent_repos: raw.recent_repos,
            agent: raw.agent,
        };

        if config.open_tabs.is_empty() && !raw.repo.path.is_empty() {
            config.open_tabs.push(OpenTabConfig {
                path: raw.repo.path.clone(),
            });
        }
        if config.active_tab_path.is_none() && !raw.repo.path.is_empty() {
            config.active_tab_path = Some(raw.repo.path);
        }
        config.normalize();
        config
    }
}

pub fn store_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn ui_state_store_path() -> PathBuf {
    config_dir().join("ui-state.json")
}

fn config_dir() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("augur-git");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log::warn!("[config] failed to create config directory: {error}");
    }
    dir
}

pub fn load() -> AppConfig {
    let path = store_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<RawAppConfig>(&text)
            .map(AppConfig::from)
            .unwrap_or_else(|e| {
                log::warn!(
                    "[config] failed to parse config; using defaults: {e}"
                );
                AppConfig::default()
            }),
        Err(_) => AppConfig::default(),
    }
}

pub fn save(config: &AppConfig) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(config)?;
    write_atomically(&store_path(), &text)?;
    Ok(())
}

pub fn load_ui_state() -> UiState {
    let path = ui_state_store_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<UiState>(&text)
            .map(|mut state| {
                state.normalize();
                state
            })
            .unwrap_or_else(|error| {
                log::warn!(
                    "[ui_state] failed to parse UI state; using defaults: {error}"
                );
                UiState::default()
            }),
        Err(_) => UiState::default(),
    }
}

pub fn save_ui_state(state: &UiState) -> anyhow::Result<()> {
    let mut state = state.clone();
    state.normalize();
    let text = serde_json::to_string_pretty(&state)?;
    write_atomically(&ui_state_store_path(), &text)?;
    Ok(())
}

fn write_atomically(path: &std::path::Path, text: &str) -> anyhow::Result<()> {
    static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let temp_path = path.with_file_name(format!(
        ".{filename}.tmp-{}-{counter}",
        std::process::id(),
    ));
    std::fs::write(&temp_path, text)?;
    match std::fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Windows does not replace an existing destination with rename.
            // Keep the same temporary-file path and use the platform fallback
            // only when the atomic replacement cannot be performed directly.
            #[cfg(windows)]
            {
                if path.exists() {
                    std::fs::remove_file(path)?;
                    std::fs::rename(&temp_path, path)?;
                    return Ok(());
                }
            }
            let _ = std::fs::remove_file(&temp_path);
            Err(error.into())
        }
    }
}

const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(150);

/// Serializes configuration writes away from the UI thread and coalesces
/// bursts of updates such as rapid tab switching.
pub struct ConfigSaveQueue {
    sender: Option<Sender<ConfigSaveRequest>>,
}

enum ConfigSaveRequest {
    Save(AppConfig),
    Flush {
        config: AppConfig,
        completed: Sender<()>,
    },
}

impl ConfigSaveQueue {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("augur-config-save".to_string())
            .spawn(move || run_save_worker(receiver));

        match worker {
            Ok(_) => Self {
                sender: Some(sender),
            },
            Err(error) => {
                log::error!("[config] failed to start save worker: {error}");
                Self { sender: None }
            }
        }
    }

    /// Queue the latest configuration snapshot for persistence.
    pub fn schedule(&self, config: &AppConfig) {
        let Some(sender) = &self.sender else {
            let config = config.clone();
            let _ = thread::Builder::new()
                .name("augur-config-save-fallback".to_string())
                .spawn(move || {
                    if let Err(error) = save(&config) {
                        log::error!(
                            "[config] failed to save configuration: {error}"
                        );
                    }
                });
            return;
        };

        if sender
            .send(ConfigSaveRequest::Save(config.clone()))
            .is_err()
        {
            log::warn!("[config] save worker is unavailable");
        }
    }

    /// Queue the final snapshot and return a receiver that completes after it
    /// has been written. This prevents an older debounced snapshot from
    /// racing with the final application-quit write.
    pub fn flush(&self, config: &AppConfig) -> Option<Receiver<()>> {
        let sender = self.sender.as_ref()?;
        let (completed, receiver) = mpsc::channel();
        if sender
            .send(ConfigSaveRequest::Flush {
                config: config.clone(),
                completed,
            })
            .is_err()
        {
            log::warn!("[config] save worker is unavailable during flush");
            return None;
        }
        Some(receiver)
    }
}

fn run_save_worker(receiver: Receiver<ConfigSaveRequest>) {
    while let Ok(request) = receiver.recv() {
        let (mut latest, mut completed) = match request {
            ConfigSaveRequest::Save(config) => (config, None),
            ConfigSaveRequest::Flush { config, completed } => {
                (config, Some(completed))
            }
        };
        let mut coalesced = 0;
        let deadline = Instant::now() + CONFIG_SAVE_DEBOUNCE;
        let mut disconnected = false;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match receiver.recv_timeout(remaining) {
                Ok(request) => match request {
                    ConfigSaveRequest::Save(config) => {
                        latest = config;
                        coalesced += 1;
                    }
                    ConfigSaveRequest::Flush {
                        config,
                        completed: ack,
                    } => {
                        latest = config;
                        completed = Some(ack);
                        coalesced += 1;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if let Err(error) = save(&latest) {
            log::error!("[config] failed to save configuration: {error}");
        } else {
            log::debug!(
                "[config] configuration saved (coalesced_updates={coalesced})"
            );
        }
        if let Some(completed) = completed {
            let _ = completed.send(());
        }

        if disconnected {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let mut config = AppConfig::default();
        config.open_tabs.push(OpenTabConfig {
            path: r"D:\dev\gitee\augur-git".into(),
        });
        config.active_tab_path = Some(r"D:\dev\gitee\augur-git".into());
        config.view.show_untracked = false;
        config.typography.ui_font_family = Some("Inter".into());
        config.typography.mono_font_family = Some("JetBrains Mono".into());
        config.typography.ui_font_size = 18.0;
        config.typography.diff_font_size = 14.0;

        let text = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back.open_tabs, config.open_tabs);
        assert_eq!(back.active_tab_path, config.active_tab_path);
        assert!(!back.view.show_untracked);
        assert_eq!(back.typography, config.typography);
    }

    #[test]
    fn defaults_are_sane() {
        let config = AppConfig::default();
        assert!(config.view.show_untracked);
        assert!(config.open_tabs.is_empty());
        assert!(config.recent_repos.is_empty());
        assert_eq!(config.typography, TypographySettings::default());
        assert_eq!(config.typography.ui_font_size, DEFAULT_UI_FONT_SIZE);
        assert_eq!(config.typography.diff_font_size, DEFAULT_DIFF_FONT_SIZE);
    }

    #[test]
    fn recent_repos_dedup_and_truncate() {
        let mut config = AppConfig::default();
        for i in 0..10 {
            config.push_recent(&format!("repo{i}"));
        }
        assert_eq!(config.recent_repos.len(), 8);
        config.push_recent("repo3");
        assert_eq!(
            config.recent_repos.first().map(String::as_str),
            Some("repo3")
        );
        assert_eq!(
            config.recent_repos.iter().filter(|p| *p == "repo3").count(),
            1
        );
    }

    #[test]
    fn language_preference_round_trips() {
        let json = r#"{"language":"zh-CN"}"#;
        let raw: RawAppConfig = serde_json::from_str(json).unwrap();
        let config = AppConfig::from(raw);
        assert_eq!(config.language, LanguagePreference::SimplifiedChinese);

        let serialized = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(serialized.contains(r#""system""#));
    }

    #[test]
    fn accepts_language_alias() {
        let json = r#"{"language":"zh"}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.language, LanguagePreference::SimplifiedChinese);
    }

    #[test]
    fn missing_agent_settings_migrate_to_built_in_default() {
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(r#"{}"#).unwrap(),
        );
        assert_eq!(config.agent.default_profile_id(), "codex");
        assert!(config.agent.custom_profiles.is_empty());
    }

    #[test]
    fn agent_settings_round_trip_with_custom_profile() {
        let json = r#"{
            "agent": {
                "default_profile_id": "reviewer",
                "custom_profiles": [{
                    "id": "reviewer",
                    "name": "Reviewer",
                    "executable": "review-agent",
                    "args": ["--interactive"],
                    "prompt_mode": {"Flag": "--prompt"}
                }]
            }
        }"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.agent.default_profile_id(), "reviewer");
        let profile = config.agent.profile("reviewer").expect("profile");
        assert_eq!(profile.args, vec!["--interactive"]);
        assert_eq!(
            profile.launch_spec_for_prompt("diagnostic").args,
            vec!["--interactive", "--prompt", "diagnostic"]
        );
    }

    #[test]
    fn migrates_legacy_single_repository() {
        let json = r#"{"repo":{"path":"D:\\repo-a"}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.open_tabs.len(), 1);
        assert_eq!(config.open_tabs[0].path, r"D:\repo-a");
        assert_eq!(config.active_tab_path.as_deref(), Some(r"D:\repo-a"));
    }

    #[test]
    fn normalizes_duplicate_tabs_and_active_path() {
        let json = r#"{
            "open_tabs":[{"path":"repo-a"},{"path":"repo-a"},{"path":"repo-b"}],
            "active_tab_path":"missing"
        }"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(
            config
                .open_tabs
                .iter()
                .map(|tab| tab.path.as_str())
                .collect::<Vec<_>>(),
            vec!["repo-a", "repo-b"]
        );
        assert_eq!(config.active_tab_path.as_deref(), Some("repo-a"));
    }

    #[test]
    fn theme_preference_round_trips() {
        let json = r#"{"theme":"catppuccin-latte"}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.theme, ThemePreference::CatppuccinLatte);
        assert_eq!(config.theme.registry_name(), "Catppuccin Latte");

        let serialized = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(serialized.contains(r#""theme":"catppuccin-mocha""#));
    }

    #[test]
    fn missing_theme_field_defaults_to_catppuccin_mocha() {
        let json = r#"{}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.theme, ThemePreference::CatppuccinMocha);
    }

    #[test]
    fn missing_typography_fields_default_to_system_fonts() {
        let json = r#"{}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.typography, TypographySettings::default());
    }

    #[test]
    fn legacy_typography_config_defaults_ui_font_size() {
        let json = r#"{"typography":{"ui_font_family":"Inter"}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.typography.ui_font_family.as_deref(), Some("Inter"));
        assert_eq!(config.typography.ui_font_size, DEFAULT_UI_FONT_SIZE);
        assert_eq!(config.typography.diff_font_size, DEFAULT_DIFF_FONT_SIZE);
    }

    #[test]
    fn normalizes_ui_font_size_to_supported_range() {
        assert_eq!(normalized_ui_font_size(f32::NAN), DEFAULT_UI_FONT_SIZE);
        assert_eq!(
            normalized_ui_font_size(MIN_UI_FONT_SIZE - 1.0),
            MIN_UI_FONT_SIZE
        );
        assert_eq!(
            normalized_ui_font_size(MAX_UI_FONT_SIZE + 1.0),
            MAX_UI_FONT_SIZE
        );
        assert_eq!(normalized_ui_font_size(18.0), 18.0);
    }

    #[test]
    fn normalizes_diff_font_size_to_supported_range() {
        assert_eq!(normalized_diff_font_size(f32::NAN), DEFAULT_DIFF_FONT_SIZE);
        assert_eq!(
            normalized_diff_font_size(MIN_DIFF_FONT_SIZE - 1.0),
            MIN_DIFF_FONT_SIZE
        );
        assert_eq!(
            normalized_diff_font_size(MAX_DIFF_FONT_SIZE + 1.0),
            MAX_DIFF_FONT_SIZE
        );
        assert_eq!(normalized_diff_font_size(18.0), 18.0);
    }

    #[test]
    fn diff_layout_preference_round_trips() {
        let json = r#"{"view":{"show_untracked":true,"auto_follow":true,"diff_layout":"side-by-side"}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.view.diff_layout, DiffLayoutPreference::SideBySide);

        let serialized = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(serialized.contains(r#""diff_layout":"side-by-side""#));
    }

    #[test]
    fn graph_history_preference_round_trips() {
        let json = r#"{"view":{"show_untracked":true,"auto_follow":true,"graph_history":"all-branches"}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(
            config.view.graph_history,
            GraphHistoryPreference::AllBranches
        );

        let serialized = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(serialized.contains(r#""graph_history":"all-branches""#));
    }

    #[test]
    fn missing_graph_history_defaults_to_all_branches() {
        let json = r#"{"view":{"show_untracked":false,"auto_follow":true}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(
            config.view.graph_history,
            GraphHistoryPreference::AllBranches
        );
    }

    #[test]
    fn missing_diff_layout_defaults_to_side_by_side() {
        let json = r#"{"view":{"show_untracked":false,"auto_follow":true}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.view.diff_layout, DiffLayoutPreference::SideBySide);
    }

    #[test]
    fn missing_auto_refresh_on_focus_defaults_to_enabled() {
        let json = r#"{"view":{"show_untracked":false,"auto_follow":true}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert!(config.view.auto_refresh_on_focus);
    }

    #[test]
    fn auto_refresh_on_focus_round_trips() {
        let json = r#"{"view":{"show_untracked":true,"auto_follow":true,"auto_refresh_on_focus":false}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert!(!config.view.auto_refresh_on_focus);

        let serialized = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(serialized.contains(r#""auto_refresh_on_focus":true"#));
    }

    #[test]
    fn theme_survives_legacy_repo_migration() {
        let json =
            r#"{"theme":"catppuccin-mocha","repo":{"path":"D:\\repo-a"}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.theme, ThemePreference::CatppuccinMocha);
        assert_eq!(config.open_tabs.len(), 1);
    }

    #[test]
    fn ui_state_roundtrips_and_normalizes_layout() {
        let mut state = UiState {
            window: WindowState {
                x: Some(12),
                y: Some(24),
                width: 640,
                height: 400,
                maximized: true,
            },
            layout: LayoutSettings {
                sidebar_width: 1000.0,
                right_panel_width: 100.0,
                diff_height: Some(10.0),
                file_list_ratio: 0.9,
            },
        };
        state.normalize();
        let text = serde_json::to_string(&state).unwrap();
        let back: UiState = serde_json::from_str(&text).unwrap();
        assert_eq!(back, state);
        assert_eq!(back.window.width, MIN_WINDOW_WIDTH);
        assert_eq!(back.window.height, MIN_WINDOW_HEIGHT);
        assert_eq!(back.layout.sidebar_width, MAX_SIDEBAR_WIDTH);
        assert_eq!(back.layout.right_panel_width, MIN_RIGHT_PANEL_WIDTH);
        assert_eq!(back.layout.diff_height, Some(MIN_DIFF_HEIGHT));
        assert_eq!(back.layout.file_list_ratio, MAX_FILE_LIST_RATIO);
    }
}
