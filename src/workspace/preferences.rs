use gpui::*;

use crate::core::config::{
    DiffLayoutPreference, GraphHistoryPreference, LanguagePreference,
    ThemePreference, normalized_diff_font_size, normalized_ui_font_size,
};
use crate::core::i18n;
use crate::git::diff_view::DiffLayoutMode;
use crate::theme;

use super::about;
use super::app_menu;
use super::{TabContent, Workspace};

impl Workspace {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.show_settings = true;
        cx.notify();
    }

    pub(super) fn open_about(&mut self, cx: &mut Context<Self>) {
        self.show_settings = false;
        cx.notify();
        about::open_about_window(self, cx);
    }

    pub(super) fn refresh_app_menu(&mut self, cx: &mut Context<Self>) {
        let locale = self.locale;
        let recent_repos = self.config.recent_repos.clone();
        self.app_menu.update(cx, |menu, cx| {
            menu.set_locale(locale);
            menu.set_recent_repos(recent_repos);
            cx.notify();
        });
        app_menu::install_native_menu(locale, cx);
    }

    pub(super) fn set_language(
        &mut self,
        preference: LanguagePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = i18n::resolve(&preference);
        let locale = self.locale;
        for entry in &mut self.tabs {
            match &entry.content {
                TabContent::Repo(tab) => {
                    tab.update(cx, |tab, cx| {
                        tab.set_locale(locale, window, cx);
                    });
                }
                TabContent::Welcome => {
                    entry.summary.title = i18n::text(locale, "tab-new");
                }
            }
        }
        if let Some(about_window) = self.about_window {
            if about_window
                .update(cx, |about, _window, cx| {
                    about.set_locale(locale, cx);
                })
                .is_err()
            {
                self.about_window = None;
            }
        }
        self.config.language = preference;
        self.settings_panel.update(cx, |panel, cx| {
            panel.set_locale(self.locale, window, cx);
        });
        self.config_saver.schedule(&self.config);
        self.refresh_app_menu(cx);
        log::info!(
            "[workspace] language preference: {:?}, locale: {}",
            preference,
            self.locale.id()
        );
        cx.notify();
    }

    /// Switch the UI theme: applies the embedded theme immediately and
    /// persists the choice. Panels read colors from `cx.theme()` on every
    /// render, so no per-panel fan-out is needed.
    pub(super) fn set_theme(
        &mut self,
        preference: ThemePreference,
        cx: &mut Context<Self>,
    ) {
        self.config.theme = preference;
        theme::apply(preference, &self.config.typography, cx);
        self.config_saver.schedule(&self.config);
        cx.notify();
    }

    pub(super) fn set_ui_font(
        &mut self,
        font: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.config.typography.ui_font_family == font {
            return;
        }
        self.config.typography.ui_font_family = font;
        theme::apply(self.config.theme, &self.config.typography, cx);
        self.config_saver.schedule(&self.config);
        log::info!("[settings] UI font preference changed");
        cx.notify();
    }

    pub(super) fn set_mono_font(
        &mut self,
        font: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.config.typography.mono_font_family == font {
            return;
        }
        self.config.typography.mono_font_family = font;
        theme::apply(self.config.theme, &self.config.typography, cx);
        self.config_saver.schedule(&self.config);
        log::info!("[settings] mono font preference changed");
        cx.notify();
    }

    pub(super) fn set_ui_font_size(
        &mut self,
        size: f32,
        cx: &mut Context<Self>,
    ) {
        let size = normalized_ui_font_size(size);
        if (self.config.typography.ui_font_size - size).abs() <= f32::EPSILON {
            return;
        }
        self.config.typography.ui_font_size = size;
        theme::apply(self.config.theme, &self.config.typography, cx);
        self.config_saver.schedule(&self.config);
        log::info!("[settings] UI font size preference changed: {size}px");
        cx.notify();
    }

    pub(super) fn set_diff_font_size(
        &mut self,
        size: f32,
        cx: &mut Context<Self>,
    ) {
        let size = normalized_diff_font_size(size);
        if (self.config.typography.diff_font_size - size).abs() <= f32::EPSILON
        {
            return;
        }
        self.config.typography.diff_font_size = size;
        theme::apply(self.config.theme, &self.config.typography, cx);
        self.config_saver.schedule(&self.config);
        log::info!("[settings] Diff font size preference changed: {size}px");
        cx.notify();
    }

    /// Switch the commit diff layout: persists the choice and applies it to
    /// every open repository tab immediately.
    pub(super) fn set_diff_layout(
        &mut self,
        preference: DiffLayoutPreference,
        cx: &mut Context<Self>,
    ) {
        if self.config.view.diff_layout == preference {
            return;
        }
        self.config.view.diff_layout = preference;
        let layout = DiffLayoutMode::from(preference);
        for entry in &self.tabs {
            if let TabContent::Repo(tab) = &entry.content {
                tab.update(cx, |tab, cx| tab.set_diff_layout(layout, cx));
            }
        }
        self.config_saver.schedule(&self.config);
        log::info!("[workspace] diff layout preference: {preference:?}");
        cx.notify();
    }

    /// Switch the commit graph history scope and apply it to every open tab.
    pub(super) fn set_graph_history(
        &mut self,
        preference: GraphHistoryPreference,
        cx: &mut Context<Self>,
    ) {
        if self.config.view.graph_history == preference {
            return;
        }
        self.config.view.graph_history = preference;
        for entry in &self.tabs {
            if let TabContent::Repo(tab) = &entry.content {
                tab.update(cx, |tab, cx| tab.set_graph_history(preference, cx));
            }
        }
        self.config_saver.schedule(&self.config);
        log::info!("[workspace] graph history preference: {preference:?}");
        cx.notify();
    }

    /// Enable or disable the focus-triggered repository refresh: persists
    /// the choice. The flag is read whenever the window is activated, so no
    /// fan-out to open tabs is needed.
    pub(super) fn set_auto_refresh_on_focus(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if self.config.view.auto_refresh_on_focus == enabled {
            return;
        }
        self.config.view.auto_refresh_on_focus = enabled;
        self.config_saver.schedule(&self.config);
        log::info!("[workspace] auto refresh on focus: {enabled}");
        cx.notify();
    }
}
