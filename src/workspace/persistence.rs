use std::path::Path;

use gpui::{App, AppContext, Context, Task};

use crate::core::config::{
    self, AppConfig, OpenTabConfig, normalized_diff_font_size,
    normalized_ui_font_size,
};

use super::Workspace;
use super::tabs::TabId;

impl Workspace {
    pub(super) fn persist_config(&mut self) {
        self.update_persisted_config();
        self.config_saver.schedule(&self.config);
    }

    fn update_persisted_config(&mut self) {
        self.config.open_tabs = self
            .tabs
            .iter()
            .filter(|tab| tab.persisted)
            .filter_map(|tab| {
                tab.path.clone().map(|path| OpenTabConfig { path })
            })
            .collect();
        self.config.active_tab_path = self
            .active_tab
            .and_then(|active| self.tabs.iter().find(|tab| tab.id == active))
            .filter(|tab| tab.persisted)
            .and_then(|tab| tab.path.clone());
    }

    pub(super) fn persist_on_quit(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if let Some(manager) = &self.extension_manager {
            manager.shutdown();
        }
        self.update_persisted_config();
        let config = self.config.clone();
        let ui_state = self.ui_state.clone();
        let config_flush = self.config_saver.flush(&config);
        cx.background_spawn(async move {
            if let Some(completed) = config_flush {
                let _ = completed.recv();
            } else if let Err(error) = config::save(&config) {
                log::error!(
                    "[config] failed to save final configuration: {error}"
                );
            }
            if let Err(error) = config::save_ui_state(&ui_state) {
                log::error!("[ui_state] failed to save UI state: {error}");
            } else {
                log::debug!("[ui_state] UI state saved on application quit");
            }
        })
    }

    pub(super) fn persist_ui_state(&mut self, cx: &mut Context<Self>) {
        let ui_state = self.ui_state.clone();
        cx.background_spawn(async move {
            if let Err(error) = config::save_ui_state(&ui_state) {
                log::error!("[ui_state] failed to save UI state: {error}");
            } else {
                log::debug!("[ui_state] UI state saved after window close");
            }
        })
        .detach();
    }
}

pub(super) fn normalized_path(path: &str) -> String {
    let path = Path::new(path);
    // `std::fs::canonicalize` returns verbatim `\\?\C:\...` paths on
    // Windows; strip that prefix so repository paths display and persist in
    // their plain form.
    let canonical =
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    crate::core::paths::normalize_extended_path(&canonical)
        .to_string_lossy()
        .into_owned()
}

pub(super) fn repo_key(path: &str) -> String {
    let mut key = normalized_path(path);
    #[cfg(windows)]
    key.make_ascii_lowercase();
    key
}

/// Unique key for a start-page tab. Repository keys are canonical paths, so
/// this prefix cannot collide with them.
pub(super) fn welcome_tab_key(id: TabId) -> String {
    format!("welcome:{id}")
}

pub(super) fn installed_font_families(cx: &App) -> Vec<String> {
    let mut families = cx
        .text_system()
        .all_font_names()
        .into_iter()
        .filter(|family| !family.starts_with('.'))
        .collect::<Vec<_>>();
    families.sort_unstable();
    families.dedup();
    families
}

pub(super) fn normalize_typography(
    config: &mut AppConfig,
    families: &[String],
) {
    for font in [
        &mut config.typography.ui_font_family,
        &mut config.typography.mono_font_family,
    ] {
        let Some(selected) = font.as_ref() else {
            continue;
        };
        if !families.iter().any(|family| family == selected) {
            log::warn!(
                "[settings] configured font is unavailable; using system default"
            );
            *font = None;
        }
    }
    config.typography.ui_font_size =
        normalized_ui_font_size(config.typography.ui_font_size);
    config.typography.diff_font_size =
        normalized_diff_font_size(config.typography.diff_font_size);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{MAX_DIFF_FONT_SIZE, MAX_UI_FONT_SIZE};

    #[test]
    fn unavailable_fonts_are_reset_to_system_defaults() {
        let mut config = AppConfig::default();
        config.typography.ui_font_family = Some("Missing UI Font".into());
        config.typography.mono_font_family = Some("Missing Mono Font".into());

        normalize_typography(&mut config, &["Inter".into()]);

        assert_eq!(config.typography, Default::default());
    }

    #[test]
    fn ui_font_size_is_clamped_during_startup_normalization() {
        let mut config = AppConfig::default();
        config.typography.ui_font_size = 100.0;

        normalize_typography(&mut config, &[]);

        assert_eq!(config.typography.ui_font_size, MAX_UI_FONT_SIZE);
    }

    #[test]
    fn diff_font_size_is_clamped_during_startup_normalization() {
        let mut config = AppConfig::default();
        config.typography.diff_font_size = 100.0;

        normalize_typography(&mut config, &[]);

        assert_eq!(config.typography.diff_font_size, MAX_DIFF_FONT_SIZE);
    }
}
