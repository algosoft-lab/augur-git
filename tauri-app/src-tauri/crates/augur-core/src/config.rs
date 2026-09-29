//! Persisted application settings and workspace state.
//!
//! This application stores user preferences and workspace state in separate
//! documents, each with a single clear owner:
//!
//! - [`AppConfig`] is small user preference state. It is written on every
//!   settings change.
//! - [`WorkspaceState`] is session state: which repositories are open, which
//!   tab is active, and how the panes are sized. It is written on tab and
//!   layout changes.
//!
//! Both documents carry a `schema_version` so a future format change can be
//! detected without guessing from field presence. Values are validated on
//! load: a corrupt document degrades to defaults in memory, and the caller
//! decides whether to report or overwrite it.

use serde::{Deserialize, Serialize};

use crate::git::{GitError, GitRepo, RepoLocation};

/// Bumped whenever the on-disk shape of either document changes.
pub const SCHEMA_VERSION: u32 = 2;

// ===== Preferences =====

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

/// User-selected UI theme. The serialized values are stable configuration
/// keys and must match the `key` field of the shipped theme catalog.
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
    #[serde(rename = "dracula")]
    Dracula,
    #[serde(rename = "tokyo-night")]
    TokyoNight,
    #[serde(rename = "tokyo-night-storm")]
    TokyoNightStorm,
    #[serde(rename = "tokyo-night-light")]
    TokyoNightLight,
    #[serde(rename = "gruvbox-dark")]
    GruvboxDark,
    #[serde(rename = "gruvbox-light")]
    GruvboxLight,
    #[serde(rename = "nord")]
    Nord,
    #[serde(rename = "solarized-dark")]
    SolarizedDark,
    #[serde(rename = "solarized-light")]
    SolarizedLight,
    #[serde(rename = "rose-pine")]
    RosePine,
    #[serde(rename = "rose-pine-moon")]
    RosePineMoon,
    #[serde(rename = "rose-pine-dawn")]
    RosePineDawn,
    #[serde(rename = "ayu-dark")]
    AyuDark,
    #[serde(rename = "ayu-mirage")]
    AyuMirage,
    #[serde(rename = "ayu-light")]
    AyuLight,
    #[serde(rename = "kanagawa-wave")]
    KanagawaWave,
    #[serde(rename = "kanagawa-lotus")]
    KanagawaLotus,
    #[serde(rename = "github-dark-default")]
    GitHubDarkDefault,
    #[serde(rename = "github-light-default")]
    GitHubLightDefault,
    #[serde(rename = "atom-one-dark")]
    AtomOneDark,
    #[serde(rename = "atom-one-light")]
    AtomOneLight,
    #[serde(rename = "everforest-dark")]
    EverforestDark,
    #[serde(rename = "everforest-light")]
    EverforestLight,
    #[serde(rename = "night-owl")]
    NightOwl,
    #[serde(rename = "light-owl")]
    LightOwl,
    #[serde(rename = "claude-dark")]
    ClaudeDark,
    #[serde(rename = "claude-light")]
    ClaudeLight,
}

impl Default for ThemePreference {
    fn default() -> Self {
        Self::ClaudeDark
    }
}

impl ThemePreference {
    /// Every preference in the order the settings list shows them.
    pub const ALL: [ThemePreference; 32] = [
        ThemePreference::GitHubDark,
        ThemePreference::GitHubDarkDefault,
        ThemePreference::GitHubLightDefault,
        ThemePreference::CatppuccinLatte,
        ThemePreference::CatppuccinFrappe,
        ThemePreference::CatppuccinMacchiato,
        ThemePreference::CatppuccinMocha,
        ThemePreference::Dracula,
        ThemePreference::TokyoNight,
        ThemePreference::TokyoNightStorm,
        ThemePreference::TokyoNightLight,
        ThemePreference::GruvboxDark,
        ThemePreference::GruvboxLight,
        ThemePreference::Nord,
        ThemePreference::SolarizedDark,
        ThemePreference::SolarizedLight,
        ThemePreference::RosePine,
        ThemePreference::RosePineMoon,
        ThemePreference::RosePineDawn,
        ThemePreference::AyuDark,
        ThemePreference::AyuMirage,
        ThemePreference::AyuLight,
        ThemePreference::KanagawaWave,
        ThemePreference::KanagawaLotus,
        ThemePreference::AtomOneDark,
        ThemePreference::AtomOneLight,
        ThemePreference::EverforestDark,
        ThemePreference::EverforestLight,
        ThemePreference::NightOwl,
        ThemePreference::LightOwl,
        ThemePreference::ClaudeDark,
        ThemePreference::ClaudeLight,
    ];

    /// Stable key shared by the preference and the theme catalog.
    pub const fn key(self) -> &'static str {
        match self {
            Self::GitHubDark => "github-dark",
            Self::CatppuccinLatte => "catppuccin-latte",
            Self::CatppuccinFrappe => "catppuccin-frappe",
            Self::CatppuccinMacchiato => "catppuccin-macchiato",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::Dracula => "dracula",
            Self::TokyoNight => "tokyo-night",
            Self::TokyoNightStorm => "tokyo-night-storm",
            Self::TokyoNightLight => "tokyo-night-light",
            Self::GruvboxDark => "gruvbox-dark",
            Self::GruvboxLight => "gruvbox-light",
            Self::Nord => "nord",
            Self::SolarizedDark => "solarized-dark",
            Self::SolarizedLight => "solarized-light",
            Self::RosePine => "rose-pine",
            Self::RosePineMoon => "rose-pine-moon",
            Self::RosePineDawn => "rose-pine-dawn",
            Self::AyuDark => "ayu-dark",
            Self::AyuMirage => "ayu-mirage",
            Self::AyuLight => "ayu-light",
            Self::KanagawaWave => "kanagawa-wave",
            Self::KanagawaLotus => "kanagawa-lotus",
            Self::GitHubDarkDefault => "github-dark-default",
            Self::GitHubLightDefault => "github-light-default",
            Self::AtomOneDark => "atom-one-dark",
            Self::AtomOneLight => "atom-one-light",
            Self::EverforestDark => "everforest-dark",
            Self::EverforestLight => "everforest-light",
            Self::NightOwl => "night-owl",
            Self::LightOwl => "light-owl",
            Self::ClaudeDark => "claude-dark",
            Self::ClaudeLight => "claude-light",
        }
    }
}

/// Layout used to render diffs.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum DiffLayoutPreference {
    #[serde(rename = "inline")]
    Inline,
    #[serde(rename = "side-by-side")]
    SideBySide,
}

impl Default for DiffLayoutPreference {
    fn default() -> Self {
        Self::Inline
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
        // Include remote-tracking branches and their divergence by default.
        Self::AllBranches
    }
}

/// Action the commit button performs.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum CommitActionPreference {
    #[serde(rename = "commit")]
    Commit,
    #[serde(rename = "amend")]
    Amend,
}

impl Default for CommitActionPreference {
    fn default() -> Self {
        Self::Commit
    }
}

/// Strategy the toolbar Pull button uses against the upstream branch.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum PullActionPreference {
    #[serde(rename = "merge")]
    Merge,
    #[serde(rename = "rebase")]
    Rebase,
}

impl Default for PullActionPreference {
    fn default() -> Self {
        Self::Rebase
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ViewSettings {
    pub show_untracked: bool,
    pub auto_follow: bool,
    pub diff_layout: DiffLayoutPreference,
    pub graph_history: GraphHistoryPreference,
    #[serde(alias = "auto_refresh_on_focus")]
    pub auto_refresh: bool,
    pub commit_action: CommitActionPreference,
    pub pull_action: PullActionPreference,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            show_untracked: true,
            auto_follow: true,
            diff_layout: DiffLayoutPreference::Inline,
            graph_history: GraphHistoryPreference::AllBranches,
            auto_refresh: true,
            commit_action: CommitActionPreference::Commit,
            pull_action: PullActionPreference::Rebase,
        }
    }
}

pub const DEFAULT_UI_FONT_SIZE: f32 = 16.0;
pub const MIN_UI_FONT_SIZE: f32 = 12.0;
pub const MAX_UI_FONT_SIZE: f32 = 20.0;
pub const DEFAULT_DIFF_FONT_SIZE: f32 = 16.0;
pub const MIN_DIFF_FONT_SIZE: f32 = 12.0;
pub const MAX_DIFF_FONT_SIZE: f32 = 20.0;

/// Clamp a persisted or user-supplied UI font size.
pub fn normalized_ui_font_size(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE)
    } else {
        DEFAULT_UI_FONT_SIZE
    }
}

/// Clamp a persisted or user-supplied diff font size.
pub fn normalized_diff_font_size(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_DIFF_FONT_SIZE, MAX_DIFF_FONT_SIZE)
    } else {
        DEFAULT_DIFF_FONT_SIZE
    }
}

/// Font families and base sizes. `None` keeps the platform default.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct TypographySettings {
    pub ui_font_family: Option<String>,
    pub mono_font_family: Option<String>,
    pub ui_font_size: f32,
    pub diff_font_size: f32,
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

impl TypographySettings {
    /// Drop empty family names and clamp the sizes to the supported range.
    pub fn normalize(&mut self) {
        self.ui_font_family = take_non_empty(self.ui_font_family.take());
        self.mono_font_family = take_non_empty(self.mono_font_family.take());
        self.ui_font_size = normalized_ui_font_size(self.ui_font_size);
        self.diff_font_size = normalized_diff_font_size(self.diff_font_size);
    }
}

fn take_non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

/// Where a repository physically lives.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LocationConfig {
    #[default]
    Local,
    #[serde(rename = "wsl")]
    Wsl { distro: String },
}

impl LocationConfig {
    pub fn wsl(distro: impl Into<String>) -> Self {
        Self::Wsl {
            distro: distro.into(),
        }
    }

    pub fn from_location(location: &RepoLocation) -> Self {
        match location {
            RepoLocation::Local => Self::Local,
            RepoLocation::Wsl { distro } => Self::wsl(distro.clone()),
        }
    }

    /// Display text: the path for local repositories, `distro · path` for WSL.
    pub fn label(&self, path: &str) -> String {
        match self {
            Self::Local => path.to_string(),
            Self::Wsl { distro } => format!("{distro} · {path}"),
        }
    }

    /// Resolve this persisted location into an executable repository handle.
    pub fn to_repo(&self, path: impl Into<String>) -> Result<GitRepo, GitError> {
        match self {
            Self::Local => Ok(GitRepo::local(path)),
            Self::Wsl { distro } => {
                #[cfg(windows)]
                {
                    Ok(GitRepo::new(
                        RepoLocation::Wsl {
                            distro: distro.clone(),
                        },
                        path,
                    ))
                }
                #[cfg(not(windows))]
                {
                    let _ = distro;
                    Err(GitError::new("err-wsl-unsupported", String::new()))
                }
            }
        }
    }

    /// Whether the location is usable on the running platform.
    pub fn is_supported(&self) -> bool {
        match self {
            Self::Local => true,
            Self::Wsl { .. } => cfg!(windows),
        }
    }
}

/// One recently opened repository.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RecentRepo {
    pub path: String,
    #[serde(default)]
    pub location: LocationConfig,
}

impl RecentRepo {
    pub fn local(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            location: LocationConfig::Local,
        }
    }

    /// Tab and recent-list identity: a repository opened twice through
    /// different locations is two different entries.
    pub fn key(&self) -> String {
        match &self.location {
            LocationConfig::Local => self.path.clone(),
            LocationConfig::Wsl { distro } => format!("wsl:{distro}:{}", self.path),
        }
    }
}

/// Maximum number of remembered repositories.
pub const MAX_RECENT_REPOS: usize = 8;

/// Small user preference document. Persisted as `settings.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub theme: ThemePreference,
    pub language: LanguagePreference,
    pub view: ViewSettings,
    pub typography: TypographySettings,
    pub recent_repos: Vec<RecentRepo>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: ThemePreference::default(),
            language: LanguagePreference::default(),
            view: ViewSettings::default(),
            typography: TypographySettings::default(),
            recent_repos: Vec::new(),
        }
    }
}

impl AppConfig {
    /// Record a repository as the most recently opened one.
    pub fn push_recent(&mut self, path: &str, location: &LocationConfig) {
        let repo = RecentRepo {
            path: path.to_string(),
            location: location.clone(),
        };
        let key = repo.key();
        self.recent_repos.retain(|entry| entry.key() != key);
        self.recent_repos.insert(0, repo);
        self.recent_repos.truncate(MAX_RECENT_REPOS);
    }

    /// Clamp and de-duplicate anything the frontend or a hand-edited file
    /// could have put into an invalid state.
    pub fn normalize(&mut self) {
        self.schema_version = SCHEMA_VERSION;
        self.typography.normalize();
        self.recent_repos.retain(|repo| !repo.path.is_empty());
        let mut seen: Vec<String> = Vec::new();
        self.recent_repos.retain(|repo| {
            let key = repo.key();
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
        self.recent_repos.truncate(MAX_RECENT_REPOS);
    }
}

// ===== Session state =====

pub const MIN_SIDEBAR_WIDTH: f32 = 180.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 400.0;
pub const MIN_RIGHT_PANEL_WIDTH: f32 = 250.0;
pub const MAX_RIGHT_PANEL_WIDTH: f32 = 600.0;
pub const MIN_DIFF_HEIGHT: f32 = 100.0;
pub const MAX_DIFF_HEIGHT: f32 = 1000.0;
pub const DEFAULT_FILE_LIST_RATIO: f32 = 0.25;
pub const MIN_FILE_LIST_RATIO: f32 = 0.2;
pub const MAX_FILE_LIST_RATIO: f32 = 0.7;

/// Pane geometry shared by every repository tab.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LayoutSettings {
    pub sidebar_width: f32,
    pub right_panel_width: f32,
    /// `None` lets the bottom panel take the remaining height.
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
        self.sidebar_width = finite_or(self.sidebar_width, Self::default().sidebar_width)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.right_panel_width =
            finite_or(self.right_panel_width, Self::default().right_panel_width)
                .clamp(MIN_RIGHT_PANEL_WIDTH, MAX_RIGHT_PANEL_WIDTH);
        self.diff_height = self.diff_height.map(|height| {
            finite_or(height, MIN_DIFF_HEIGHT).clamp(MIN_DIFF_HEIGHT, MAX_DIFF_HEIGHT)
        });
        self.file_list_ratio = finite_or(self.file_list_ratio, DEFAULT_FILE_LIST_RATIO)
            .clamp(MIN_FILE_LIST_RATIO, MAX_FILE_LIST_RATIO);
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

/// One open repository tab.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OpenTabConfig {
    pub path: String,
    #[serde(default)]
    pub location: LocationConfig,
}

impl OpenTabConfig {
    pub fn key(&self) -> String {
        RecentRepo {
            path: self.path.clone(),
            location: self.location.clone(),
        }
        .key()
    }
}

/// Session document. Persisted as `workspace.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct WorkspaceState {
    pub schema_version: u32,
    pub open_tabs: Vec<OpenTabConfig>,
    /// Identity of the active tab; see [`OpenTabConfig::key`].
    pub active_tab: Option<String>,
    pub layout: LayoutSettings,
    pub window_mode: WindowMode,
    pub desktop_window: Option<WindowBounds>,
    pub sidecar_window: Option<WindowBounds>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowMode {
    #[default]
    #[serde(rename = "desktop")]
    Desktop,
    #[serde(rename = "sidecar")]
    Sidecar,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            open_tabs: Vec::new(),
            active_tab: None,
            layout: LayoutSettings::default(),
            window_mode: WindowMode::Desktop,
            desktop_window: None,
            sidecar_window: None,
        }
    }
}

impl WorkspaceState {
    /// Drop empty and duplicate tabs, repair the active-tab pointer, and
    /// clamp the layout.
    pub fn normalize(&mut self) {
        self.schema_version = SCHEMA_VERSION;
        let mut seen: Vec<String> = Vec::new();
        self.open_tabs.retain(|tab| {
            let key = tab.key();
            if tab.path.is_empty() || seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
        self.layout.normalize();
        self.desktop_window = self.desktop_window.map(|bounds| bounds.normalized(false));
        self.sidecar_window = self.sidecar_window.map(|bounds| bounds.normalized(true));

        if self
            .active_tab
            .as_ref()
            .is_some_and(|active| !self.open_tabs.iter().any(|tab| &tab.key() == active))
        {
            self.active_tab = None;
        }
        if self.active_tab.is_none() {
            self.active_tab = self.open_tabs.first().map(OpenTabConfig::key);
        }
    }
}

impl WindowBounds {
    fn normalized(self, sidecar: bool) -> Self {
        let finite = |value: f64, fallback: f64| {
            if value.is_finite() { value } else { fallback }
        };
        Self {
            x: finite(self.x, 0.0),
            y: finite(self.y, 0.0),
            width: finite(self.width, if sidecar { 420.0 } else { 1280.0 }).clamp(
                if sidecar { 360.0 } else { 860.0 },
                if sidecar { 520.0 } else { 8192.0 },
            ),
            height: finite(self.height, 800.0).clamp(480.0, 8192.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_reference_application() {
        let config = AppConfig::default();
        assert_eq!(config.theme, ThemePreference::ClaudeDark);
        assert_eq!(config.language, LanguagePreference::System);
        assert_eq!(config.view.diff_layout, DiffLayoutPreference::Inline);
        assert_eq!(
            config.view.graph_history,
            GraphHistoryPreference::AllBranches
        );
        assert!(config.view.auto_refresh);
        assert_eq!(config.view.commit_action, CommitActionPreference::Commit);
        assert_eq!(config.typography.ui_font_size, 16.0);
        assert_eq!(config.typography.diff_font_size, 16.0);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let config: AppConfig = serde_json::from_str(r#"{"theme":"github-dark"}"#).unwrap();
        assert_eq!(config.theme, ThemePreference::GitHubDark);
        assert_eq!(config.view, ViewSettings::default());
        assert_eq!(config.typography, TypographySettings::default());
    }

    #[test]
    fn old_focus_refresh_setting_migrates_to_auto_refresh() {
        let config: AppConfig =
            serde_json::from_str(r#"{"view":{"auto_refresh_on_focus":false}}"#).unwrap();
        assert!(!config.view.auto_refresh);
        let serialized = serde_json::to_value(config).unwrap();
        assert_eq!(serialized["view"]["auto_refresh"], false);
        assert!(serialized["view"].get("auto_refresh_on_focus").is_none());
    }

    #[test]
    fn theme_preferences_have_unique_stable_keys_and_round_trip() {
        let keys: Vec<_> = ThemePreference::ALL.map(ThemePreference::key).into();
        let unique: std::collections::HashSet<_> = keys.iter().copied().collect();
        let expected = [
            "github-dark",
            "github-dark-default",
            "github-light-default",
            "catppuccin-latte",
            "catppuccin-frappe",
            "catppuccin-macchiato",
            "catppuccin-mocha",
            "dracula",
            "tokyo-night",
            "tokyo-night-storm",
            "tokyo-night-light",
            "gruvbox-dark",
            "gruvbox-light",
            "nord",
            "solarized-dark",
            "solarized-light",
            "rose-pine",
            "rose-pine-moon",
            "rose-pine-dawn",
            "ayu-dark",
            "ayu-mirage",
            "ayu-light",
            "kanagawa-wave",
            "kanagawa-lotus",
            "atom-one-dark",
            "atom-one-light",
            "everforest-dark",
            "everforest-light",
            "night-owl",
            "light-owl",
            "claude-dark",
            "claude-light",
        ];
        assert_eq!(keys.len(), 32);
        assert_eq!(unique.len(), keys.len());
        assert_eq!(ThemePreference::ALL.len(), keys.len());
        assert_eq!(keys.as_slice(), expected.as_slice());

        for preference in ThemePreference::ALL {
            let serialized = serde_json::to_string(&preference).unwrap();
            assert_eq!(serialized, format!("\"{}\"", preference.key()));
            let parsed: ThemePreference = serde_json::from_str(&serialized).unwrap();
            assert_eq!(parsed, preference);
        }
    }

    #[test]
    fn settings_round_trip() {
        let mut config = AppConfig::default();
        config.language = LanguagePreference::SimplifiedChinese;
        config.typography.mono_font_family = Some("JetBrains Mono".into());
        let text = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn recent_repositories_are_deduplicated_and_capped() {
        let mut config = AppConfig::default();
        for index in 0..12 {
            config.push_recent(&format!("/repo/{index}"), &LocationConfig::Local);
        }
        config.push_recent("/repo/11", &LocationConfig::Local);
        assert_eq!(config.recent_repos.len(), MAX_RECENT_REPOS);
        assert_eq!(config.recent_repos[0].path, "/repo/11");
        assert_eq!(config.recent_repos[1].path, "/repo/10");
    }

    #[test]
    fn wsl_entries_are_separate_from_local_entries() {
        let mut config = AppConfig::default();
        config.push_recent("/home/dev/repo", &LocationConfig::wsl("Ubuntu"));
        config.push_recent("/home/dev/repo", &LocationConfig::Local);
        assert_eq!(config.recent_repos.len(), 2);
    }

    #[test]
    fn typography_normalization_drops_blank_families() {
        let mut typography = TypographySettings {
            ui_font_family: Some("   ".into()),
            mono_font_family: Some("Menlo".into()),
            ui_font_size: 999.0,
            diff_font_size: f32::NAN,
        };
        typography.normalize();
        assert_eq!(typography.ui_font_family, None);
        assert_eq!(typography.mono_font_family.as_deref(), Some("Menlo"));
        assert_eq!(typography.ui_font_size, MAX_UI_FONT_SIZE);
        assert_eq!(typography.diff_font_size, DEFAULT_DIFF_FONT_SIZE);
    }

    #[test]
    fn layout_is_clamped_to_supported_bounds() {
        let mut layout = LayoutSettings {
            sidebar_width: 5.0,
            right_panel_width: 5_000.0,
            diff_height: Some(9_000.0),
            file_list_ratio: 5.0,
        };
        layout.normalize();
        assert_eq!(layout.sidebar_width, MIN_SIDEBAR_WIDTH);
        assert_eq!(layout.right_panel_width, MAX_RIGHT_PANEL_WIDTH);
        assert_eq!(layout.diff_height, Some(MAX_DIFF_HEIGHT));
        assert_eq!(layout.file_list_ratio, MAX_FILE_LIST_RATIO);
    }

    #[test]
    fn workspace_state_repairs_the_active_tab() {
        let mut state: WorkspaceState = serde_json::from_str(
            r#"{"open_tabs":[{"path":"/a"},{"path":"/a"},{"path":""}],"active_tab":"/missing"}"#,
        )
        .unwrap();
        state.normalize();
        assert_eq!(state.open_tabs.len(), 1);
        assert_eq!(state.active_tab.as_deref(), Some("/a"));
    }

    #[test]
    fn workspace_state_defaults_older_documents_and_clamps_window_bounds() {
        let mut state: WorkspaceState = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "desktop_window": { "x": 8.0, "y": 12.0, "width": 400.0, "height": 300.0 },
            "sidecar_window": { "x": 10.0, "y": 20.0, "width": 320.0, "height": 700.0 }
        }))
        .unwrap();
        state.normalize();

        assert_eq!(state.schema_version, SCHEMA_VERSION);
        assert_eq!(state.window_mode, WindowMode::Desktop);
        assert_eq!(state.desktop_window.unwrap().width, 860.0);
        assert_eq!(state.desktop_window.unwrap().height, 480.0);
        assert_eq!(state.sidecar_window.unwrap().width, 360.0);

        let mut oversized: WorkspaceState = serde_json::from_value(serde_json::json!({
            "sidecar_window": { "x": 0.0, "y": 0.0, "width": 900.0, "height": 700.0 }
        }))
        .unwrap();
        oversized.normalize();
        assert_eq!(oversized.sidecar_window.unwrap().width, 520.0);
    }

    #[test]
    fn wsl_locations_are_rejected_off_windows() {
        let location = LocationConfig::wsl("Ubuntu");
        assert_eq!(location.is_supported(), cfg!(windows));
        if !cfg!(windows) {
            assert!(location.to_repo("/home/dev/repo").is_err());
        }
    }
}
