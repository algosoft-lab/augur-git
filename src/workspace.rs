//! Application shell and repository tab lifecycle.
//!
//! The workspace owns application-wide state. Each open repository is represented
//! by an independent `RepoTab` entity, including its Git worker and panels.

mod repo_tab;
mod tabs;
mod welcome;

use std::path::Path;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, InteractiveElementExt, Root, TitleBar, h_flex,
    input::{InputEvent, InputState},
    theme::{Theme, ThemeMode},
    v_flex,
};

use crate::core::config::{
    self, AppConfig, LanguagePreference, OpenTabConfig, ThemePreference,
};
use crate::core::i18n::{self, Locale};

use self::repo_tab::{RepoTab, RepoTabEvent};
use self::tabs::{
    RepoTabBar, RepoTabBarEvent, TabId, TabSummary, fallback_after_close,
};
use crate::theme;

pub fn run(app: Application) {
    app.run(|cx| {
        gpui_component::init(cx);

        // Load config before the window opens: the persisted theme must be
        // applied before first paint, and Workspace::new receives the config
        // (single read, no double IO).
        let config = config::load();
        // Create the Theme global before touching ThemeRegistry::global_mut:
        // the registry's global observer reads Theme::global.
        Theme::change(ThemeMode::Dark, None, cx);
        theme::init(config.theme, cx);

        cx.spawn(async move |cx| {
            let window_options = cx.update(initial_window_options);
            cx.open_window(window_options, |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(config, window, cx));
                cx.new(|cx| Root::new(workspace, window, cx))
            })
            .unwrap_or_else(|error| {
                log::error!("[workspace] failed to open window: {error}");
                std::process::exit(1);
            })
        })
        .detach();
    });
}

fn initial_window_options(cx: &mut App) -> WindowOptions {
    let desired_size = size(px(1280.), px(800.));
    let primary_display = cx.primary_display();

    let window_bounds = if let Some(display) = primary_display.clone() {
        let visible_bounds = display.visible_bounds();
        let clamped_size = desired_size.min(&visible_bounds.size);
        WindowBounds::Windowed(Bounds::centered_at(
            visible_bounds.center(),
            clamped_size,
        ))
    } else {
        WindowBounds::centered(desired_size, cx)
    };

    WindowOptions {
        window_bounds: Some(window_bounds),
        display_id: primary_display.map(|display| display.id()),
        titlebar: Some(TitleBar::title_bar_options()),
        window_min_size: Some(gpui::Size {
            width: px(860.),
            height: px(480.),
        }),
        ..Default::default()
    }
}

struct TabEntry {
    id: TabId,
    key: String,
    path: String,
    tab: Entity<RepoTab>,
    summary: TabSummary,
    persisted: bool,
}

pub struct Workspace {
    tabs: Vec<TabEntry>,
    active_tab: Option<TabId>,
    next_tab_id: TabId,
    tab_bar: Entity<RepoTabBar>,
    repo_path_input: Entity<InputState>,
    config: AppConfig,
    language_preference: LanguagePreference,
    /// UI theme preference (settings overlay; source of truth is config.theme)
    theme_preference: ThemePreference,
    locale: Locale,
    config_saver: config::ConfigSaveQueue,
    show_settings: bool,
    restoring: bool,
}

impl Workspace {
    pub fn new(
        config: AppConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme_preference = config.theme;
        let language_preference = config.language;
        let locale = i18n::resolve(&language_preference);
        let config_saver = config::ConfigSaveQueue::new();
        let tab_bar = cx.new(|_cx| RepoTabBar::new());
        let input_default = config.active_tab_path.clone().unwrap_or_default();
        let repo_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::text(locale, "repo-path-placeholder"))
                .default_value(input_default)
        });

        let input = repo_path_input.clone();
        cx.subscribe_in(
            &input,
            window,
            |workspace, _input, event, window, cx| {
                if matches!(
                    event,
                    InputEvent::PressEnter {
                        secondary: false,
                        ..
                    }
                ) {
                    workspace.open_repo_from_input(window, cx);
                }
            },
        )
        .detach();

        let tab_bar_for_events = tab_bar.clone();
        cx.subscribe_in(
            &tab_bar_for_events,
            window,
            |workspace, _bar, event, window, cx| match event {
                RepoTabBarEvent::NewTab => {
                    log::info!("[workspace_tabs] new-tab event received");
                    workspace.pick_repo_folder(window, cx)
                }
                RepoTabBarEvent::Select(id) => workspace.select_tab(*id, cx),
                RepoTabBarEvent::Close(id) => workspace.close_tab(*id, cx),
            },
        )
        .detach();

        let mut workspace = Self {
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 1,
            tab_bar,
            repo_path_input,
            config,
            language_preference,
            theme_preference,
            locale,
            config_saver,
            show_settings: false,
            restoring: true,
        };
        workspace.restore_tabs(window, cx);
        workspace.restore_active_tab();
        if let Some(active) = workspace.active_tab {
            workspace.activate_tab(active, cx);
        }
        workspace.restoring = false;
        workspace.refresh_tab_bar(cx);
        workspace.persist_config();
        workspace
    }

    fn restore_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self
            .config
            .open_tabs
            .iter()
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>();
        for path in paths {
            self.add_repo_tab(path, true, false, window, cx);
        }
    }

    fn restore_active_tab(&mut self) {
        let desired_key = self.config.active_tab_path.as_deref().map(repo_key);
        self.active_tab = desired_key
            .and_then(|key| self.tabs.iter().find(|tab| tab.key == key))
            .map(|tab| tab.id)
            .or_else(|| self.tabs.first().map(|tab| tab.id));
    }

    fn add_repo_tab(
        &mut self,
        requested_path: String,
        restored: bool,
        activate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = normalized_path(&requested_path);
        let key = repo_key(&path);
        if let Some(id) = self
            .tabs
            .iter()
            .find(|tab| tab.key == key)
            .map(|tab| tab.id)
        {
            if activate {
                self.select_tab(id, cx);
            }
            return;
        }

        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        let locale = self.locale;
        let path_for_tab = path.clone();
        let tab =
            cx.new(|cx| RepoTab::new(id, path_for_tab, locale, window, cx));
        let summary = tab.read(cx).summary();
        self.tabs.push(TabEntry {
            id,
            key,
            path,
            tab: tab.clone(),
            summary,
            persisted: restored,
        });
        self.subscribe_to_tab(&tab, cx);
        if activate {
            self.activate_tab(id, cx);
        }
        self.refresh_tab_bar(cx);
        cx.notify();
    }

    fn subscribe_to_tab(
        &mut self,
        tab: &Entity<RepoTab>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(tab, |workspace, _tab, event, cx| {
            workspace.handle_repo_tab_event(event, cx);
        })
        .detach();
    }

    fn handle_repo_tab_event(
        &mut self,
        event: &RepoTabEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            RepoTabEvent::Opened { id, path } => {
                if let Some(entry) =
                    self.tabs.iter_mut().find(|tab| tab.id == *id)
                {
                    entry.persisted = true;
                }
                if !self.restoring {
                    self.config.push_recent(path);
                    self.persist_config();
                }
                self.refresh_tab_bar(cx);
                cx.notify();
            }
            RepoTabEvent::SummaryChanged(summary) => {
                let changed = if let Some(entry) =
                    self.tabs.iter_mut().find(|tab| tab.id == summary.id)
                {
                    if entry.summary != *summary {
                        entry.summary = summary.clone();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if changed {
                    self.refresh_tab_bar(cx);
                    cx.notify();
                }
            }
            RepoTabEvent::RequestSettings => {
                self.show_settings = true;
                cx.notify();
            }
        }
    }

    fn refresh_tab_bar(&mut self, cx: &mut Context<Self>) {
        let summaries = self
            .tabs
            .iter()
            .map(|entry| entry.summary.clone())
            .collect::<Vec<_>>();
        self.tab_bar.update(cx, |bar, cx| {
            bar.set_tabs(summaries, self.active_tab, cx);
        });
    }

    fn select_tab(&mut self, id: TabId, cx: &mut Context<Self>) {
        if self.activate_tab(id, cx) {
            self.persist_config();
            self.refresh_tab_bar(cx);
            cx.notify();
        }
    }

    fn activate_tab(&mut self, id: TabId, cx: &mut Context<Self>) -> bool {
        let Some(next_tab) = self
            .tabs
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.tab.clone())
        else {
            return false;
        };

        let changed = self.active_tab != Some(id);
        if changed {
            if let Some(previous_tab) = self
                .active_tab
                .and_then(|active| {
                    self.tabs.iter().find(|entry| entry.id == active)
                })
                .map(|entry| entry.tab.clone())
            {
                previous_tab.update(cx, |tab, cx| tab.deactivate(cx));
                log::info!("[workspace_tabs] tab deactivated");
            }
            self.active_tab = Some(id);
        }

        next_tab.update(cx, |tab, cx| tab.activate(cx));
        if changed {
            log::info!("[workspace_tabs] tab activated");
        }
        changed
    }

    fn close_tab(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let order = self.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
        let fallback = fallback_after_close(&order, self.active_tab, id);
        let entry = self.tabs.remove(index);
        entry.tab.update(cx, |tab, cx| tab.close(cx));
        self.active_tab = fallback;
        if let Some(active) = fallback {
            self.activate_tab(active, cx);
        }
        self.persist_config();
        self.refresh_tab_bar(cx);
        cx.notify();
    }

    fn active_tab_entity(&self) -> Option<Entity<RepoTab>> {
        self.active_tab.and_then(|active| {
            self.tabs
                .iter()
                .find(|tab| tab.id == active)
                .map(|tab| tab.tab.clone())
        })
    }

    fn emit_sidebar_focus(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_entity() {
            tab.update(cx, |tab, cx| tab.focus_branches(cx));
        }
    }

    fn set_language(
        &mut self,
        preference: LanguagePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.language_preference = preference;
        self.locale = i18n::resolve(&preference);
        let locale = self.locale;
        for entry in &self.tabs {
            entry.tab.update(cx, |tab, cx| {
                tab.set_locale(locale, window, cx);
            });
        }
        self.repo_path_input.update(cx, |input, cx| {
            input.set_placeholder(
                i18n::text(locale, "repo-path-placeholder"),
                window,
                cx,
            );
        });
        self.config.language = preference;
        self.config_saver.schedule(&self.config);
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
    fn set_theme(
        &mut self,
        preference: ThemePreference,
        cx: &mut Context<Self>,
    ) {
        self.theme_preference = preference;
        self.config.theme = preference;
        theme::apply(preference, cx);
        self.config_saver.schedule(&self.config);
        cx.notify();
    }

    fn title_bar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let branch = self
            .active_tab
            .and_then(|active| self.tabs.iter().find(|tab| tab.id == active))
            .and_then(|tab| tab.summary.branch.clone());
        let this = cx.entity();
        let branch_badge = branch.map(|branch| {
            h_flex()
                .id("title-branch")
                .px_2()
                .py_0p5()
                .rounded_md()
                .gap_1()
                .bg(colors.input)
                .hover(|element| element.bg(colors.list_hover))
                .cursor(CursorStyle::PointingHand)
                .text_size(px(11.))
                .text_color(colors.blue)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.blue)
                        .child(crate::git::lucide("git-branch")),
                )
                .child(branch)
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |workspace, cx| {
                        workspace.emit_sidebar_focus(cx);
                    });
                })
        });

        TitleBar::new().child(
            h_flex()
                .id("title-bar-content")
                .w_full()
                .h_full()
                .items_center()
                .px_2()
                .gap_3()
                .on_double_click(|_event, window, _cx| {
                    window.zoom_window();
                })
                .child(
                    h_flex()
                        .items_center()
                        .gap_1p5()
                        .text_color(colors.blue)
                        .child(
                            div()
                                .text_size(px(16.))
                                .child(crate::git::lucide("git-branch")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.foreground)
                                .child("augur-git"),
                        ),
                )
                .child(self.tab_bar.clone())
                .when_some(branch_badge, |element, badge| element.child(badge)),
        )
    }

    fn open_repo_from_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.repo_path_input.read(cx).value().trim().to_string();
        if !path.is_empty() {
            self.open_repo_path(path, false, window, cx);
        }
    }

    fn open_repo_path(
        &mut self,
        path: String,
        restored: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_repo_tab(path, restored, true, window, cx);
    }

    fn pick_repo_folder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("[workspace_tabs] opening repository folder picker");
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from(i18n::text(
                self.locale,
                "repo-folder-prompt",
            ))),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths
                    .first()
                    .map(|path| path.to_string_lossy().into_owned()),
                _ => None,
            };
            let Some(path) = path else {
                log::info!("[workspace_tabs] repository folder picker cancelled");
                return;
            };
            log::info!("[workspace_tabs] repository folder selected");
            match cx.update(|window, app| {
                this.update(app, |workspace, cx| {
                    workspace.open_repo_path(path, false, window, cx);
                })
            }) {
                Ok(Ok(())) => {
                    log::info!("[workspace_tabs] repository tab opened from picker");
                }
                Ok(Err(error)) => {
                    log::warn!(
                        "[workspace_tabs] workspace entity unavailable after picker: {error}"
                    );
                }
                Err(error) => {
                    log::warn!(
                        "[workspace_tabs] window unavailable after repository picker: {error}"
                    );
                }
            }
        })
        .detach();
    }

    fn persist_config(&mut self) {
        self.config.open_tabs = self
            .tabs
            .iter()
            .filter(|tab| tab.persisted)
            .map(|tab| OpenTabConfig {
                path: tab.path.clone(),
            })
            .collect();
        self.config.active_tab_path = self
            .active_tab
            .and_then(|active| self.tabs.iter().find(|tab| tab.id == active))
            .filter(|tab| tab.persisted)
            .map(|tab| tab.path.clone());
        self.config_saver.schedule(&self.config);
    }
}

impl Render for Workspace {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let content = if let Some(tab) = self.active_tab_entity() {
            tab.into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .child(self.welcome(window, cx))
                .child(self.empty_status_bar(cx))
                .into_any_element()
        };

        v_flex()
            .id("workspace")
            .size_full()
            .relative()
            .bg(colors.background)
            .child(self.title_bar(window, cx))
            .child(content)
            .when(self.show_settings, |element| {
                element.child(self.settings_overlay(cx))
            })
    }
}

impl Workspace {
    fn welcome(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        welcome::render_welcome(self, window, cx)
    }

    fn settings_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        welcome::render_settings_overlay(self, cx)
    }

    fn empty_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        h_flex()
            .id("status-bar")
            .w_full()
            .h_6()
            .flex_shrink_0()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.background)
            .px_3()
            .items_center()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .child(i18n::text(self.locale, "status-no-repo-selected")),
            )
    }
}

fn normalized_path(path: &str) -> String {
    let path = Path::new(path);
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn repo_key(path: &str) -> String {
    let mut key = normalized_path(path);
    #[cfg(windows)]
    key.make_ascii_lowercase();
    key
}
