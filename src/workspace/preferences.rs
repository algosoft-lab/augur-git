use gpui::*;

use crate::agent::{AgentLaunchOverrides, BuiltInAgent, CustomAgentProfile};
use crate::core::config::{
    DiffLayoutPreference, GraphHistoryPreference, LanguagePreference,
    ThemePreference, normalized_diff_font_size, normalized_ui_font_size,
};
use crate::core::i18n;
use crate::git::diff_view::DiffLayoutMode;
use crate::theme;

use super::about;
use super::agent_connectivity;
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
        agent_connectivity::set_locale(self, locale, cx);
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

    pub(super) fn set_agent_default_profile(
        &mut self,
        profile_id: String,
        cx: &mut Context<Self>,
    ) {
        if self.config.agent.profile(&profile_id).is_none() {
            log::warn!(
                "[agent_terminal] ignoring unknown default profile: {profile_id}"
            );
            return;
        }
        if self.config.agent.default_profile_id.as_deref() == Some(&profile_id)
        {
            return;
        }
        self.config.agent.default_profile_id = Some(profile_id.clone());
        self.config_saver.schedule(&self.config);
        log::info!("[agent_terminal] default profile changed: {profile_id}");
        cx.notify();
    }

    pub(super) fn set_agent_executable_override(
        &mut self,
        agent: BuiltInAgent,
        executable: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if executable.as_ref().is_some_and(|path| {
            path.as_os_str().is_empty()
                || path.to_string_lossy().chars().any(char::is_control)
        }) {
            log::warn!(
                "[agent_terminal] ignoring invalid executable override for {}",
                agent.id()
            );
            return;
        }
        if executable.as_ref()
            == self.config.agent.executable_overrides.get(&agent)
        {
            return;
        }
        match executable {
            Some(path) => {
                self.config.agent.executable_overrides.insert(agent, path);
            }
            None => {
                self.config.agent.executable_overrides.remove(&agent);
            }
        }
        let settings = self.config.agent.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.update_agent_settings(settings.clone(), cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!(
            "[agent_terminal] executable override changed: {}",
            agent.id()
        );
        cx.notify();
    }

    pub(super) fn set_agent_model_override(
        &mut self,
        agent: BuiltInAgent,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let model = normalize_agent_override(model);
        let current = self
            .config
            .agent
            .launch_overrides
            .get(&agent)
            .and_then(|overrides| overrides.model.clone());
        if current == model {
            return;
        }

        let mut settings = self.config.agent.clone();
        let overrides = settings
            .launch_overrides
            .entry(agent)
            .or_insert_with(AgentLaunchOverrides::default);
        overrides.model = model;
        if let Err(error) = overrides.validate_for(agent) {
            log::warn!(
                "[agent_terminal] ignoring invalid model override for {}: {error}",
                agent.id()
            );
            return;
        }
        if overrides.model.is_none()
            && overrides.reasoning_effort.is_none()
            && overrides.variant.is_none()
        {
            settings.launch_overrides.remove(&agent);
        }
        self.config.agent = settings.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.update_agent_settings(settings, cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!(
            "[agent_terminal] model override changed: agent={}",
            agent.id()
        );
        cx.notify();
    }

    pub(super) fn set_agent_reasoning_override(
        &mut self,
        agent: BuiltInAgent,
        reasoning_effort: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let reasoning_effort = normalize_agent_override(reasoning_effort);
        let current = self
            .config
            .agent
            .launch_overrides
            .get(&agent)
            .and_then(|overrides| overrides.reasoning_effort.clone());
        if current == reasoning_effort {
            return;
        }

        let mut settings = self.config.agent.clone();
        let overrides = settings
            .launch_overrides
            .entry(agent)
            .or_insert_with(AgentLaunchOverrides::default);
        overrides.reasoning_effort = reasoning_effort;
        if let Err(error) = overrides.validate_for(agent) {
            log::warn!(
                "[agent_terminal] ignoring invalid reasoning override for {}: {error}",
                agent.id()
            );
            return;
        }
        if overrides.model.is_none()
            && overrides.reasoning_effort.is_none()
            && overrides.variant.is_none()
        {
            settings.launch_overrides.remove(&agent);
        }
        self.config.agent = settings.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.update_agent_settings(settings, cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!(
            "[agent_terminal] reasoning override changed: agent={}",
            agent.id()
        );
        cx.notify();
    }

    pub(super) fn set_agent_variant_override(
        &mut self,
        agent: BuiltInAgent,
        variant: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let variant = normalize_agent_override(variant);
        let current = self
            .config
            .agent
            .launch_overrides
            .get(&agent)
            .and_then(|overrides| overrides.variant.clone());
        if current == variant {
            return;
        }

        let mut settings = self.config.agent.clone();
        let overrides = settings
            .launch_overrides
            .entry(agent)
            .or_insert_with(AgentLaunchOverrides::default);
        overrides.variant = variant;
        if let Err(error) = overrides.validate_for(agent) {
            log::warn!(
                "[agent_terminal] ignoring invalid variant override for {}: {error}",
                agent.id()
            );
            return;
        }
        if overrides.model.is_none()
            && overrides.reasoning_effort.is_none()
            && overrides.variant.is_none()
        {
            settings.launch_overrides.remove(&agent);
        }
        self.config.agent = settings.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.update_agent_settings(settings, cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!(
            "[agent_terminal] variant override changed: agent={}",
            agent.id()
        );
        cx.notify();
    }

    pub(super) fn save_agent_profile(
        &mut self,
        previous_id: Option<String>,
        profile: CustomAgentProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.config.agent.clone();
        if let Some(previous_id) = previous_id.as_deref() {
            if settings.default_profile_id.as_deref() == Some(previous_id) {
                settings.default_profile_id = Some(profile.id.clone());
            }
            settings
                .custom_profiles
                .retain(|entry| entry.id != previous_id);
        }
        settings.custom_profiles.push(profile.clone());
        if let Err(errors) = settings.validate() {
            log::warn!(
                "[agent_terminal] rejected custom profile update: {}",
                errors
                    .first()
                    .map(String::as_str)
                    .unwrap_or("invalid profile")
            );
            return;
        }
        self.config.agent = settings.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.set_agent_settings(settings.clone(), window, cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!("[agent_terminal] custom profile saved: id={}", profile.id);
        cx.notify();
    }

    pub(super) fn add_agent_builtin(
        &mut self,
        agent: BuiltInAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("[agent_settings] adding built-in agent: {}", agent.id());
        if self.config.agent.enabled_builtins().contains(&agent) {
            return;
        }
        self.config.agent.set_builtin_enabled(agent, true);
        let settings = self.config.agent.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.set_agent_settings(settings.clone(), window, cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!("[agent_settings] built-in agent added: {}", agent.id());
        cx.notify();
    }

    pub(super) fn remove_agent_builtin(
        &mut self,
        agent: BuiltInAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.config.agent.enabled_builtins().contains(&agent) {
            return;
        }
        self.config.agent.set_builtin_enabled(agent, false);
        if self.config.agent.default_profile_id.as_deref() == Some(agent.id()) {
            self.config.agent.default_profile_id = None;
        }
        let settings = self.config.agent.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.set_agent_settings(settings.clone(), window, cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!("[agent_terminal] built-in agent removed: {}", agent.id());
        cx.notify();
    }

    pub(super) fn remove_agent_profile(
        &mut self,
        profile_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.config.agent.custom_profiles.len();
        self.config
            .agent
            .custom_profiles
            .retain(|profile| profile.id != profile_id);
        if self.config.agent.custom_profiles.len() == before {
            return;
        }
        if self.config.agent.default_profile_id.as_deref() == Some(profile_id) {
            self.config.agent.default_profile_id = None;
        }
        let settings = self.config.agent.clone();
        self.settings_panel.update(cx, |panel, cx| {
            panel.set_agent_settings(settings.clone(), window, cx);
        });
        self.config_saver.schedule(&self.config);
        log::info!("[agent_terminal] custom profile removed: id={profile_id}");
        cx.notify();
    }
}

fn normalize_agent_override(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
