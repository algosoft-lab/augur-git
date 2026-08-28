//! Application shell and repository tab lifecycle.
//!
//! The workspace owns application-wide state. Each open repository is represented
//! by an independent `RepoTab` entity, including its Git worker and panels.

mod about;
mod app_menu;
mod repo_tab;
mod tabs;
mod welcome;

use std::path::Path;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, InteractiveElementExt, Root, TitleBar, h_flex,
    theme::{Theme, ThemeMode},
    v_flex,
};

use crate::core::config::{
    self, AppConfig, LanguagePreference, OpenTabConfig, ThemePreference,
};
use crate::core::i18n::{self, Locale};

use self::app_menu::{AppMenu, AppMenuEvent};
use self::repo_tab::{RepoTab, RepoTabEvent};
use self::tabs::{
    RepoTabBar, RepoTabBarEvent, TabId, TabState, TabSummary,
    fallback_after_close,
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
        app_menu::install_native_menu(i18n::resolve(&config.language), cx);
        cx.on_action(|_: &app_menu::Quit, cx| cx.quit());

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
        // Draw the custom title bar's own window controls on Linux instead of
        // showing them next to server-side decorations. Ignored on macOS and
        // Windows, where the field has no platform implementation.
        window_decorations: Some(WindowDecorations::Client),
        window_min_size: Some(gpui::Size {
            width: px(860.),
            height: px(480.),
        }),
        ..Default::default()
    }
}

/// Content of a workspace tab. A tab either hosts an open repository or is
/// an empty start page that turns into a repository tab once the user opens
/// one from it.
enum TabContent {
    Welcome,
    Repo(Entity<RepoTab>),
}

struct TabEntry {
    id: TabId,
    key: String,
    path: Option<String>,
    content: TabContent,
    summary: TabSummary,
    persisted: bool,
}

pub struct Workspace {
    tabs: Vec<TabEntry>,
    active_tab: Option<TabId>,
    next_tab_id: TabId,
    tab_bar: Entity<RepoTabBar>,
    app_menu: Entity<AppMenu>,
    config: AppConfig,
    language_preference: LanguagePreference,
    /// UI theme preference (settings overlay; source of truth is config.theme)
    theme_preference: ThemePreference,
    locale: Locale,
    config_saver: config::ConfigSaveQueue,
    show_settings: bool,
    about_window: Option<WindowHandle<about::AboutWindow>>,
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
        let app_menu =
            cx.new(|_cx| AppMenu::new(locale, config.recent_repos.clone()));

        let app_menu_for_events = app_menu.clone();
        cx.subscribe_in(
            &app_menu_for_events,
            window,
            |workspace, _menu, event, window, cx| match event {
                AppMenuEvent::OpenRecent(path) => {
                    log::info!("[app_menu] opening recent repository");
                    workspace.open_repo_path(path.clone(), false, window, cx);
                }
            },
        )
        .detach();

        let tab_bar_for_events = tab_bar.clone();
        cx.subscribe_in(
            &tab_bar_for_events,
            window,
            |workspace, _bar, event, _window, cx| match event {
                RepoTabBarEvent::NewTab => {
                    log::info!("[workspace_tabs] new-tab event received");
                    workspace.add_welcome_tab(cx)
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
            app_menu,
            config,
            language_preference,
            theme_preference,
            locale,
            config_saver,
            show_settings: false,
            about_window: None,
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
            path: Some(path),
            content: TabContent::Repo(tab.clone()),
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

    /// Open an empty start-page tab; it turns into a repository tab when the
    /// user opens a repository from it. Start-page tabs are never persisted.
    fn add_welcome_tab(&mut self, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        log::info!("[workspace_tabs] opening start-page tab {id}");
        self.tabs.push(TabEntry {
            id,
            key: welcome_tab_key(id),
            path: None,
            content: TabContent::Welcome,
            summary: TabSummary {
                id,
                title: i18n::text(self.locale, "tab-new"),
                branch: None,
                state: TabState::Ready,
            },
            persisted: false,
        });
        self.activate_tab(id, cx);
        self.refresh_tab_bar(cx);
        cx.notify();
    }

    /// Load a repository into an existing start-page tab, keeping its slot.
    fn open_repo_in_tab(
        &mut self,
        id: TabId,
        requested_path: String,
        restored: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = normalized_path(&requested_path);
        let Some(entry) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let locale = self.locale;
        let tab =
            cx.new(|cx| RepoTab::new(id, path.clone(), locale, window, cx));
        let summary = tab.read(cx).summary();
        entry.key = repo_key(&path);
        entry.path = Some(path);
        entry.content = TabContent::Repo(tab.clone());
        entry.summary = summary;
        entry.persisted = restored;
        self.subscribe_to_tab(&tab, cx);
        self.activate_tab(id, cx);
        self.refresh_tab_bar(cx);
        cx.notify();
    }

    fn active_welcome_tab_id(&self) -> Option<TabId> {
        let active = self.active_tab?;
        self.tabs
            .iter()
            .find(|tab| tab.id == active)
            .filter(|tab| matches!(tab.content, TabContent::Welcome))
            .map(|tab| tab.id)
    }

    /// Remove a tab entry without changing the active-tab selection.
    fn remove_tab_entry(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let entry = self.tabs.remove(index);
        if let TabContent::Repo(tab) = &entry.content {
            tab.update(cx, |tab, cx| tab.close(cx));
        }
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
                self.refresh_app_menu(cx);
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
                self.open_settings(cx);
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
        let Some(entry) = self.tabs.iter().find(|entry| entry.id == id) else {
            return false;
        };
        let next_tab = match &entry.content {
            TabContent::Repo(tab) => Some(tab.clone()),
            TabContent::Welcome => None,
        };

        let changed = self.active_tab != Some(id);
        if changed {
            let previous_tab = self
                .active_tab
                .and_then(|active| {
                    self.tabs.iter().find(|entry| entry.id == active)
                })
                .and_then(|entry| match &entry.content {
                    TabContent::Repo(tab) => Some(tab.clone()),
                    TabContent::Welcome => None,
                });
            if let Some(previous_tab) = previous_tab {
                previous_tab.update(cx, |tab, cx| tab.deactivate(cx));
                log::info!("[workspace_tabs] tab deactivated");
            }
            self.active_tab = Some(id);
        }

        if let Some(next_tab) = next_tab {
            next_tab.update(cx, |tab, cx| tab.activate(cx));
        }
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
        if let TabContent::Repo(tab) = &entry.content {
            tab.update(cx, |tab, cx| tab.close(cx));
        }
        self.active_tab = fallback;
        if let Some(active) = fallback {
            self.activate_tab(active, cx);
        }
        self.persist_config();
        self.refresh_tab_bar(cx);
        cx.notify();
    }

    fn active_tab_entity(&self) -> Option<Entity<RepoTab>> {
        let active = self.active_tab?;
        match &self.tabs.iter().find(|tab| tab.id == active)?.content {
            TabContent::Repo(tab) => Some(tab.clone()),
            TabContent::Welcome => None,
        }
    }

    fn emit_sidebar_focus(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_entity() {
            tab.update(cx, |tab, cx| tab.focus_branches(cx));
        }
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.show_settings = true;
        cx.notify();
    }

    fn open_about(&mut self, cx: &mut Context<Self>) {
        self.show_settings = false;
        cx.notify();
        about::open_about_window(self, cx);
    }

    fn refresh_app_menu(&mut self, cx: &mut Context<Self>) {
        let locale = self.locale;
        let recent_repos = self.config.recent_repos.clone();
        self.app_menu.update(cx, |menu, cx| {
            menu.set_locale(locale);
            menu.set_recent_repos(recent_repos);
            cx.notify();
        });
        app_menu::install_native_menu(locale, cx);
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

        let title_bar = if cfg!(target_os = "macos") {
            TitleBar::new()
        } else {
            TitleBar::new().pl_0()
        };

        title_bar.child(
            h_flex()
                .id("title-bar-content")
                .w_full()
                .h_full()
                .items_center()
                .pl_0()
                .pr_2()
                .gap_3()
                .on_double_click(|_event, window, _cx| {
                    window.zoom_window();
                })
                .child(self.app_menu.clone())
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

    fn handle_open_repository(
        &mut self,
        _: &app_menu::OpenRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("[app_menu] open repository action");
        self.pick_repo_folder(window, cx);
    }

    fn handle_new_tab(
        &mut self,
        _: &app_menu::NewTab,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("[app_menu] new tab action");
        self.add_welcome_tab(cx);
    }

    fn handle_open_settings(
        &mut self,
        _: &app_menu::OpenSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("[app_menu] open settings action");
        self.open_settings(cx);
    }

    fn handle_open_about(
        &mut self,
        _: &app_menu::OpenAbout,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("[app_menu] open about action");
        self.open_about(cx);
    }

    fn open_repo_path(
        &mut self,
        requested_path: String,
        restored: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = repo_key(&requested_path);
        if let Some(existing) = self
            .tabs
            .iter()
            .find(|tab| tab.key == key)
            .map(|tab| tab.id)
        {
            // The repository is already open: drop the start-page tab that
            // triggered the request instead of duplicating the repository.
            if let Some(welcome) = self.active_welcome_tab_id() {
                self.remove_tab_entry(welcome, cx);
            }
            self.select_tab(existing, cx);
            return;
        }
        if let Some(welcome) = self.active_welcome_tab_id() {
            self.open_repo_in_tab(
                welcome,
                requested_path,
                restored,
                window,
                cx,
            );
            return;
        }
        self.add_repo_tab(requested_path, restored, true, window, cx);
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
            .filter_map(|tab| {
                tab.path.clone().map(|path| OpenTabConfig { path })
            })
            .collect();
        self.config.active_tab_path = self
            .active_tab
            .and_then(|active| self.tabs.iter().find(|tab| tab.id == active))
            .filter(|tab| tab.persisted)
            .and_then(|tab| tab.path.clone());
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
            .on_action(cx.listener(Self::handle_open_repository))
            .on_action(cx.listener(Self::handle_new_tab))
            .on_action(cx.listener(Self::handle_open_settings))
            .on_action(cx.listener(Self::handle_open_about))
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

/// Unique key for a start-page tab. Repository keys are canonical paths, so
/// this prefix cannot collide with them.
fn welcome_tab_key(id: TabId) -> String {
    format!("welcome:{id}")
}
