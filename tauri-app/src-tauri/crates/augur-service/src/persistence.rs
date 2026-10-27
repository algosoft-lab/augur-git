//! Durable application state.
//!
//! Two documents (settings and workspace) live behind [`crate::store::DocumentStore`]
//! implementations chosen by the frontend, so the desktop keeps its Tauri store
//! files and the server keeps plain JSON files, while this module owns the
//! interpretation of the document contents for both.
//!
//! Mutations are applied immediately and handed to the store, which decides
//! when to hit the disk. [`Persistence::flush`] forces a final write during
//! shutdown.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use augur_core::config::{AppConfig, LayoutSettings, SCHEMA_VERSION, WorkspaceState};
use augur_core::keymap::{self, ShortcutOverrides, ShortcutState};

use crate::events::{APP_EVENT, AppEvent, AppEventEnvelope};
use crate::sink::{EventSink, emit};
use crate::store::DocumentStore;

/// Problems found while loading, reported once so the interface can warn the
/// user instead of silently replacing their data.
#[derive(Clone, Debug, Default, Serialize)]
pub struct LoadReport {
    /// One human-readable description per unreadable document.
    pub warnings: Vec<String>,
}

/// The stored settings document: preferences plus shortcut overrides.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsDocument {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(flatten)]
    pub config: AppConfig,
    /// Command id to keystrokes. It lives beside the preferences so a single
    /// file owns everything the settings page can change.
    #[serde(default)]
    pub shortcuts: ShortcutOverrides,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for SettingsDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            config: AppConfig::default(),
            shortcuts: BTreeMap::new(),
        }
    }
}

/// The documents plus handles to their stores.
pub struct Persistence {
    settings: Mutex<SettingsDocument>,
    workspace: Mutex<WorkspaceState>,
    sink: Arc<dyn EventSink>,
    settings_store: Arc<dyn DocumentStore>,
    workspace_store: Arc<dyn DocumentStore>,
}

impl Persistence {
    /// Load both documents from the given stores. A missing or malformed
    /// document yields defaults plus a warning; the unreadable file is left on
    /// disk so the user can recover it.
    pub fn load(
        sink: Arc<dyn EventSink>,
        settings_store: Arc<dyn DocumentStore>,
        workspace_store: Arc<dyn DocumentStore>,
    ) -> (Self, LoadReport) {
        let mut warnings = Vec::new();

        let settings =
            match read_document::<SettingsDocument>(&settings_store, SETTINGS_DOCUMENT_KEY) {
                Ok(Some(mut document)) => {
                    document.config.normalize();
                    document
                }
                Ok(None) => SettingsDocument::default(),
                Err(message) => {
                    warnings.push(message);
                    SettingsDocument::default()
                }
            };
        let workspace =
            match read_document::<WorkspaceState>(&workspace_store, WORKSPACE_DOCUMENT_KEY) {
                Ok(Some(mut state)) => {
                    state.normalize();
                    state
                }
                Ok(None) => WorkspaceState::default(),
                Err(message) => {
                    warnings.push(message);
                    WorkspaceState::default()
                }
            };

        log::info!("[store] loaded the settings and workspace documents");
        (
            Self {
                settings: Mutex::new(settings),
                workspace: Mutex::new(workspace),
                sink,
                settings_store,
                workspace_store,
            },
            LoadReport { warnings },
        )
    }

    pub fn settings(&self) -> SettingsDocument {
        self.settings.lock().expect("settings lock").clone()
    }

    pub fn config(&self) -> AppConfig {
        self.settings.lock().expect("settings lock").config.clone()
    }

    pub fn workspace(&self) -> WorkspaceState {
        self.workspace.lock().expect("workspace lock").clone()
    }

    pub fn layout(&self) -> LayoutSettings {
        self.workspace
            .lock()
            .expect("workspace lock")
            .layout
            .clone()
    }

    pub fn shortcuts(&self) -> ShortcutOverrides {
        self.settings
            .lock()
            .expect("settings lock")
            .shortcuts
            .clone()
    }

    /// Effective keys for every editable command: system defaults merged with
    /// the user's overrides, plus the raw overrides so the settings page can
    /// show and reset them.
    pub fn shortcut_state(&self) -> ShortcutState {
        let overrides = self.shortcuts();
        let user = keymap::file_from_overrides(&overrides);
        let system = keymap::system_defaults();
        ShortcutState {
            resolved: keymap::resolve(&system, &user, &keymap::COMMANDS),
            defaults: keymap::resolve(&system, &keymap::ShortcutFile::default(), &keymap::COMMANDS),
            overrides,
        }
    }

    /// Effective keys for every editable command.
    pub fn resolved_shortcuts(&self) -> Vec<keymap::ResolvedShortcut> {
        self.shortcut_state().resolved
    }

    /// Record a settings change, persist it, and tell every window.
    pub fn update_settings(&self, mutate: impl FnOnce(&mut SettingsDocument)) {
        let document = {
            let mut settings = self.settings.lock().expect("settings lock");
            mutate(&mut settings);
            settings.config.normalize();
            settings.schema_version = SCHEMA_VERSION;
            settings.clone()
        };
        write_document(&self.settings_store, SETTINGS_DOCUMENT_KEY, &document);
        self.notify(AppEvent::SettingsChanged);
    }

    /// Record a session change and persist it.
    pub fn update_workspace(&self, mutate: impl FnOnce(&mut WorkspaceState)) {
        let state = {
            let mut workspace = self.workspace.lock().expect("workspace lock");
            mutate(&mut workspace);
            workspace.normalize();
            workspace.clone()
        };
        write_document(&self.workspace_store, WORKSPACE_DOCUMENT_KEY, &state);
    }

    /// Record a session change and tell every window the tab list moved.
    pub fn update_workspace_visible(&self, mutate: impl FnOnce(&mut WorkspaceState)) {
        self.update_workspace(mutate);
        self.notify(AppEvent::WorkspaceChanged);
    }

    /// Force both documents to disk. Used on shutdown.
    pub fn flush(&self) {
        self.settings_store.save();
        self.workspace_store.save();
    }

    /// Report an application-level notice to every window.
    pub fn notify(&self, event: AppEvent) {
        emit(
            self.sink.as_ref(),
            crate::sink::EventTarget::All,
            APP_EVENT,
            &AppEventEnvelope { event },
        );
    }

    /// Absolute paths of the store files, shown in diagnostics.
    pub fn store_paths(&self) -> Vec<String> {
        [&self.settings_store, &self.workspace_store]
            .iter()
            .filter_map(|store| {
                store
                    .path()
                    .map(|path| path.to_string_lossy().into_owned())
            })
            .collect()
    }
}

/// Keys used inside the two store files.
const SETTINGS_DOCUMENT_KEY: &str = "settings";
const WORKSPACE_DOCUMENT_KEY: &str = "workspace";

fn read_document<T: serde::de::DeserializeOwned>(
    store: &Arc<dyn DocumentStore>,
    key: &str,
) -> std::result::Result<Option<T>, String> {
    match store.get(key) {
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(|error| format!("{key} could not be read: {error}")),
        None => Ok(None),
    }
}

fn write_document<T: Serialize>(store: &Arc<dyn DocumentStore>, key: &str, value: &T) {
    match serde_json::to_value(value) {
        Ok(value) => store.set(key, value),
        Err(error) => log::error!("[store] failed to encode {key}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_document_defaults_to_the_current_schema() {
        let document = SettingsDocument::default();
        assert_eq!(document.schema_version, SCHEMA_VERSION);
        assert_eq!(document.config.theme, AppConfig::default().theme);
        assert!(document.shortcuts.is_empty());
    }

    #[test]
    fn a_settings_document_without_shortcuts_still_parses() {
        let json = serde_json::json!({ "theme": "github-dark" });
        let document: SettingsDocument = serde_json::from_value(json).unwrap();
        assert_eq!(
            document.config.theme,
            augur_core::config::ThemePreference::GitHubDark
        );
        assert!(document.shortcuts.is_empty());
    }

    #[test]
    fn settings_round_trip_preserves_preferences_and_shortcuts() {
        let mut document = SettingsDocument::default();
        document.config.language = augur_core::config::LanguagePreference::SimplifiedChinese;
        document
            .shortcuts
            .insert(keymap::QUIT_COMMAND.to_string(), vec!["ctrl-q".to_string()]);
        let value = serde_json::to_value(&document).unwrap();
        let parsed: SettingsDocument = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.config.language, document.config.language);
        assert_eq!(parsed.shortcuts, document.shortcuts);
    }
}
