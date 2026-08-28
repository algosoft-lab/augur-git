//! Application configuration and persistence for repository and view settings.
//!
//! The configuration is stored under the platform's standard user config directory.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

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
        Self::GitHubDark
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
        Self::Inline
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ViewSettings {
    pub show_untracked: bool,
    pub auto_follow: bool,
    #[serde(default)]
    pub diff_layout: DiffLayoutPreference,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            show_untracked: true,
            auto_follow: true,
            diff_layout: DiffLayoutPreference::Inline,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppConfig {
    /// UI theme; missing field falls back to GitHub Dark (the original look).
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
    pub recent_repos: Vec<String>,
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
    recent_repos: Vec<String>,
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
            recent_repos: raw.recent_repos,
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
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("augur-git");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log::warn!("[config] failed to create config directory: {error}");
    }
    dir.join("config.json")
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
    std::fs::write(store_path(), text)?;
    Ok(())
}

const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(150);

/// Serializes configuration writes away from the UI thread and coalesces
/// bursts of updates such as rapid tab switching.
pub struct ConfigSaveQueue {
    sender: Option<Sender<AppConfig>>,
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

        if sender.send(config.clone()).is_err() {
            log::warn!("[config] save worker is unavailable");
        }
    }
}

fn run_save_worker(receiver: Receiver<AppConfig>) {
    while let Ok(mut latest) = receiver.recv() {
        let mut coalesced = 0;
        let deadline = Instant::now() + CONFIG_SAVE_DEBOUNCE;
        let mut disconnected = false;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match receiver.recv_timeout(remaining) {
                Ok(config) => {
                    latest = config;
                    coalesced += 1;
                }
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

        let text = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back.open_tabs, config.open_tabs);
        assert_eq!(back.active_tab_path, config.active_tab_path);
        assert!(!back.view.show_untracked);
    }

    #[test]
    fn defaults_are_sane() {
        let config = AppConfig::default();
        assert!(config.view.show_untracked);
        assert!(config.open_tabs.is_empty());
        assert!(config.recent_repos.is_empty());
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
        assert!(serialized.contains(r#""theme":"github-dark""#));
    }

    #[test]
    fn missing_theme_field_defaults_to_github_dark() {
        let json = r#"{}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.theme, ThemePreference::GitHubDark);
    }

    #[test]
    fn diff_layout_preference_round_trips() {
        let json = r#"{"view":{"show_untracked":true,"auto_follow":true,"diff_layout":"side-by-side"}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.view.diff_layout, DiffLayoutPreference::SideBySide);

        let serialized = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(serialized.contains(r#""diff_layout":"inline""#));
    }

    #[test]
    fn missing_diff_layout_defaults_to_inline() {
        let json = r#"{"view":{"show_untracked":false,"auto_follow":true}}"#;
        let config = AppConfig::from(
            serde_json::from_str::<RawAppConfig>(json).unwrap(),
        );
        assert_eq!(config.view.diff_layout, DiffLayoutPreference::Inline);
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
}
