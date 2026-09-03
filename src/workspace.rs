//! Application shell and repository tab lifecycle.
//!
//! The workspace owns application-wide state. Each open repository is represented
//! by an independent `RepoTab` entity, including its Git worker and panels.

mod about;
mod agent_commit;
mod agent_connectivity;
mod agent_extension;
mod agent_lifecycle;
mod agent_merge;
mod agent_profiles;
mod agent_rebase;
mod app_menu;
mod extension_runtime;
mod extensions;
mod extensions_window;
mod focus_refresh;
mod keymap;
mod persistence;
mod preferences;
mod repo_tab;
mod settings;
mod tabs;
mod welcome;
mod window_lifecycle;
mod window_state;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, InteractiveElementExt, Root, TitleBar, h_flex,
    theme::{Theme, ThemeMode},
    v_flex,
};

use crate::core::config::{self, AppConfig, UiState};
use crate::core::i18n::{self, Locale};
use crate::extension::{
    AgentSessionRequest, ExtensionDefinition, ExtensionEvent, ExtensionHost,
    ExtensionManager, HostBridge, HostEvent, RepositorySnapshot,
    discover_definitions,
};

use self::agent_lifecycle::PendingWorkspaceClose;
use self::app_menu::{AppMenu, AppMenuEvent};
use self::extensions::ExtensionsPanel;
use self::persistence::{
    installed_font_families, normalize_typography, normalized_path, repo_key,
    welcome_tab_key,
};
use self::repo_tab::{RepoTab, RepoTabEvent};
use self::settings::{SettingsPanel, SettingsPanelEvent};
use self::tabs::{
    RepoTabBar, RepoTabBarEvent, TabId, TabState, TabSummary,
    should_refresh_after_switch,
};
use crate::theme;

pub fn run(app: Application) {
    app.on_reopen(|cx| {
        if !cx.windows().is_empty() {
            log::info!(
                "[app_lifecycle] reopen requested while a window is open; activating it"
            );
            cx.activate(true);
            return;
        }

        log::info!("[app_lifecycle] macOS reopen requested");
        cx.activate(true);
        cx.spawn(async move |cx| {
            let (mut config, ui_state) = cx
                .background_spawn(async {
                    (config::load(), config::load_ui_state())
                })
                .await;
            let result = cx.update(|cx| {
                let fonts = installed_font_families(cx);
                normalize_typography(&mut config, &fonts);
                theme::apply(config.theme, &config.typography, cx);
                open_main_window(cx, config, ui_state, fonts)
            });
            match result {
                Ok(_) => log::info!("[app_lifecycle] reopened main window"),
                Err(error) => log::error!(
                    "[app_lifecycle] failed to reopen main window: {error}"
                ),
            }
        })
        .detach();
    });

    app.run(|cx| {
        gpui_component::init(cx);

        // Load config before the window opens: the persisted theme must be
        // applied before first paint, and Workspace::new receives the config
        // (single read, no double IO).
        let mut config = config::load();
        let font_families = installed_font_families(cx);
        normalize_typography(&mut config, &font_families);
        let ui_state = config::load_ui_state();
        // Create the Theme global before touching ThemeRegistry::global_mut:
        // the registry's global observer reads Theme::global.
        Theme::change(ThemeMode::Dark, None, cx);
        theme::init(config.theme, &config.typography, cx);
        cx.on_action(|_: &app_menu::Quit, cx| {
            if !update_active_workspace(cx, |workspace, cx| {
                workspace.request_application_quit(cx);
            }) {
                cx.quit();
            }
        });
        cx.on_action(|_: &app_menu::OpenAbout, cx| {
            log::info!("[app_menu] routing global open about action");
            update_active_workspace(cx, |workspace, cx| {
                workspace.open_about(cx)
            });
        });
        cx.on_action(|_: &app_menu::OpenSettings, cx| {
            log::info!("[app_menu] routing global open settings action");
            update_active_workspace(cx, |workspace, cx| {
                workspace.open_settings(cx)
            });
        });
        cx.on_action(|_: &app_menu::OpenExtensions, cx| {
            log::info!("[app_menu] routing global open extensions action");
            update_active_workspace(cx, |workspace, cx| {
                workspace.open_extensions(cx)
            });
        });
        // Bind user-customizable shortcuts before menus so native menu
        // key equivalents (for example the macOS Cmd-Q item) are picked up.
        keymap::install(cx);
        app_menu::install_native_menu(i18n::resolve(&config.language), cx);

        cx.activate(true);
        if let Err(error) =
            open_main_window(cx, config, ui_state, font_families)
        {
            log::error!(
                "[app_lifecycle] failed to open initial window: {error}"
            );
            std::process::exit(1);
        }
    });
}

#[derive(Clone)]
struct ActiveWorkspace {
    workspace: WeakEntity<Workspace>,
}

impl Global for ActiveWorkspace {}

fn update_active_workspace(
    cx: &mut App,
    update: impl FnOnce(&mut Workspace, &mut Context<Workspace>),
) -> bool {
    let workspace = cx
        .try_global::<ActiveWorkspace>()
        .and_then(|active| active.workspace.upgrade());
    let Some(workspace) = workspace else {
        log::warn!("[app_menu] no active workspace for application action");
        return false;
    };

    workspace.update(cx, update);
    true
}

fn open_main_window(
    cx: &mut App,
    config: AppConfig,
    ui_state: UiState,
    font_families: Vec<String>,
) -> anyhow::Result<WindowHandle<Root>> {
    let window_options =
        window_state::initial_window_options(cx, &ui_state.window);
    let window = cx.open_window(window_options, |window, cx| {
        let workspace = cx.new(|cx| {
            Workspace::new(config, ui_state, font_families, window, cx)
        });
        cx.set_global(ActiveWorkspace {
            workspace: workspace.downgrade(),
        });
        cx.new(|cx| Root::new(workspace, window, cx))
    })?;
    cx.activate(true);
    log::info!("[app_lifecycle] main window created");
    Ok(window)
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
    settings_panel: Entity<SettingsPanel>,
    extensions_panel: Entity<ExtensionsPanel>,
    extensions_window:
        Option<WindowHandle<extensions_window::ExtensionsWindow>>,
    extension_host: HostBridge,
    extension_manager: Option<ExtensionManager>,
    extension_events: Receiver<ExtensionEvent>,
    host_events: Receiver<HostEvent>,
    agent_session_requests: Receiver<AgentSessionRequest>,
    extension_definitions: Vec<ExtensionDefinition>,
    extension_observed_repositories: BTreeMap<u64, RepositorySnapshot>,
    extension_pending_origins: HashMap<u64, (String, u64, Instant)>,
    extension_pending_events:
        HashMap<(String, String), extension_runtime::PendingEventBatch>,
    extension_interval_ticks:
        HashMap<(String, String), chrono::DateTime<chrono::Local>>,
    extension_drafts: BTreeMap<
        String,
        BTreeMap<String, crate::core::extension::SettingValue>,
    >,
    pending_extension_install: Option<(String, PathBuf)>,
    last_extension_tick: chrono::DateTime<chrono::Local>,
    ui_state: UiState,
    locale: Locale,
    config_saver: config::ConfigSaveQueue,
    show_settings: bool,
    pending_close: Option<PendingWorkspaceClose>,
    about_window: Option<WindowHandle<about::AboutWindow>>,
    agent_sessions:
        Vec<(String, WindowHandle<agent_connectivity::AgentSessionWindow>)>,
    agent_preflight_keys: HashSet<String>,
    restoring: bool,
    last_focus_refresh: Option<Instant>,
}

impl Workspace {
    pub fn new(
        mut config: AppConfig,
        ui_state: UiState,
        font_families: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if let Err(errors) = config.agent.validate() {
            for error in errors {
                log::warn!(
                    "[agent_terminal] invalid configured profile: {error}"
                );
            }
        }
        let locale = i18n::resolve(&config.language);
        let config_saver = config::ConfigSaveQueue::new();
        let tab_bar = cx.new(|_cx| RepoTabBar::new());
        let app_menu =
            cx.new(|_cx| AppMenu::new(locale, config.recent_repos.clone()));
        let settings_panel =
            cx.new(|cx| SettingsPanel::new(&config, font_families, window, cx));
        let extension_definitions = discover_definitions();
        for definition in &extension_definitions {
            let id = definition.package.manifest.id.clone();
            let entry = config.extensions.entry(id).or_insert_with(|| {
                let mut settings =
                    crate::core::extension::ExtensionSettings::with_defaults(
                        &definition.package.manifest,
                    );
                // Bundled packages are reviewed with the application and do
                // not need a second trust prompt; they remain disabled.
                settings.trusted = definition.package.bundled;
                settings
            });
            *entry = entry.normalized_for(&definition.package.manifest);
            entry.last_seen_fingerprint =
                Some(definition.package.fingerprint.clone());
        }
        let (extension_host, host_events, agent_session_requests) =
            HostBridge::new(config.agent.clone());
        let extension_host_for_manager: Arc<dyn ExtensionHost> =
            Arc::new(extension_host.clone());
        let (extension_manager, extension_events) = match ExtensionManager::new(
            extension_definitions.clone(),
            extension_host_for_manager,
        ) {
            Ok((manager, events)) => (Some(manager), events),
            Err(error) => {
                log::error!("[extensions] failed to start runtime: {error}");
                let (_tx, events) = std::sync::mpsc::channel();
                (None, events)
            }
        };
        let extensions_panel = cx.new(|cx| {
            ExtensionsPanel::new(
                extension_definitions.clone(),
                &config,
                locale,
                window,
                cx,
            )
        });

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

        let settings_panel_for_events = settings_panel.clone();
        cx.subscribe_in(
            &settings_panel_for_events,
            window,
            |workspace, _panel, event, window, cx| match event {
                SettingsPanelEvent::Close => {
                    workspace.show_settings = false;
                    cx.notify();
                }
                SettingsPanelEvent::LanguageChanged(preference) => {
                    workspace.set_language(*preference, window, cx);
                }
                SettingsPanelEvent::AutoRefreshOnFocusChanged(enabled) => {
                    workspace.set_auto_refresh_on_focus(*enabled, cx);
                }
                SettingsPanelEvent::ThemeChanged(preference) => {
                    workspace.set_theme(*preference, cx);
                }
                SettingsPanelEvent::DiffLayoutChanged(preference) => {
                    workspace.set_diff_layout(*preference, cx);
                }
                SettingsPanelEvent::GraphHistoryChanged(preference) => {
                    workspace.set_graph_history(*preference, cx);
                }
                SettingsPanelEvent::UiFontChanged(font) => {
                    workspace.set_ui_font(font.clone(), cx);
                }
                SettingsPanelEvent::MonoFontChanged(font) => {
                    workspace.set_mono_font(font.clone(), cx);
                }
                SettingsPanelEvent::UiFontSizeChanged(size) => {
                    workspace.set_ui_font_size(*size, cx);
                }
                SettingsPanelEvent::DiffFontSizeChanged(size) => {
                    workspace.set_diff_font_size(*size, cx);
                }
                SettingsPanelEvent::ShortcutChanged { command, keys } => {
                    workspace.set_shortcut(
                        command.clone(),
                        keys.clone(),
                        window,
                        cx,
                    );
                }
                SettingsPanelEvent::ShortcutReset(command) => {
                    workspace.reset_shortcut(command.clone(), window, cx);
                }
                SettingsPanelEvent::AgentDefaultProfileChanged(profile_id) => {
                    workspace.set_agent_default_profile(profile_id.clone(), cx);
                }
                SettingsPanelEvent::AgentExecutableOverrideChanged {
                    agent,
                    executable,
                } => {
                    workspace.set_agent_executable_override(
                        *agent,
                        executable.clone(),
                        cx,
                    );
                }
                SettingsPanelEvent::AgentModelOverrideChanged {
                    agent,
                    model,
                } => {
                    workspace.set_agent_model_override(
                        *agent,
                        model.clone(),
                        cx,
                    );
                }
                SettingsPanelEvent::AgentReasoningOverrideChanged {
                    agent,
                    reasoning_effort,
                } => {
                    workspace.set_agent_reasoning_override(
                        *agent,
                        reasoning_effort.clone(),
                        cx,
                    );
                }
                SettingsPanelEvent::AgentVariantOverrideChanged {
                    agent,
                    variant,
                } => {
                    workspace.set_agent_variant_override(
                        *agent,
                        variant.clone(),
                        cx,
                    );
                }
                SettingsPanelEvent::AgentConnectivityTestRequested(
                    profile_id,
                ) => {
                    agent_connectivity::open(workspace, profile_id.clone(), cx);
                }
                SettingsPanelEvent::AgentProfileSaved {
                    previous_id,
                    profile,
                } => {
                    workspace.save_agent_profile(
                        previous_id.clone(),
                        profile.clone(),
                        window,
                        cx,
                    );
                }
                SettingsPanelEvent::AgentProfileRemoved(profile_id) => {
                    workspace.remove_agent_profile(profile_id, window, cx);
                }
                SettingsPanelEvent::AgentBuiltinAddRequested(agent) => {
                    log::info!(
                        "[agent_settings] built-in add event received: {}",
                        agent.id()
                    );
                    workspace.add_agent_builtin(*agent, window, cx);
                }
                SettingsPanelEvent::AgentBuiltinRemoveRequested(agent) => {
                    workspace.remove_agent_builtin(*agent, window, cx);
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
            settings_panel,
            extensions_panel,
            extensions_window: None,
            extension_host,
            extension_manager,
            extension_events,
            host_events,
            agent_session_requests,
            extension_definitions,
            extension_observed_repositories: BTreeMap::new(),
            extension_pending_origins: HashMap::new(),
            extension_pending_events: HashMap::new(),
            extension_interval_ticks: HashMap::new(),
            extension_drafts: BTreeMap::new(),
            pending_extension_install: None,
            last_extension_tick: chrono::Local::now(),
            ui_state,
            locale,
            config_saver,
            show_settings: false,
            pending_close: None,
            about_window: None,
            agent_sessions: Vec::new(),
            agent_preflight_keys: HashSet::new(),
            restoring: true,
            // The startup load starts here (restore_tabs -> open), so the
            // activation delivered right after window creation must not
            // trigger a duplicate refresh.
            last_focus_refresh: Some(Instant::now()),
        };
        let workspace_for_close = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_window, app| {
            workspace_for_close
                .update(app, |workspace, cx| workspace.request_window_close(cx))
                .unwrap_or(true)
        });
        workspace.restore_tabs(window, cx);
        workspace.restore_active_tab();
        if let Some(active) = workspace.active_tab {
            workspace.activate_tab(active, cx);
        }
        workspace.restoring = false;
        workspace.refresh_tab_bar(cx);
        workspace.sync_extension_repositories(cx);
        workspace.start_extension_polling(cx);
        workspace.persist_config();
        cx.observe_window_bounds(window, |workspace, window, _cx| {
            window_state::update_ui_state_window(
                &mut workspace.ui_state,
                window,
            );
        })
        .detach();
        cx.observe_window_activation(window, |workspace, window, cx| {
            workspace.handle_window_activation(window, cx);
        })
        .detach();
        cx.on_app_quit(|workspace, cx| workspace.persist_on_quit(cx))
            .detach();
        #[cfg(target_os = "macos")]
        window_lifecycle::install_window_close_observer(cx);
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
        let tab = cx.new(|cx| {
            RepoTab::new(
                id,
                path_for_tab,
                locale,
                self.config.view.diff_layout.into(),
                self.config.view.graph_history,
                self.config.view.commit_action.into(),
                self.ui_state.layout.clone(),
                window,
                cx,
            )
        });
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
        let tab = cx.new(|cx| {
            RepoTab::new(
                id,
                path.clone(),
                locale,
                self.config.view.diff_layout.into(),
                self.config.view.graph_history,
                self.config.view.commit_action.into(),
                self.ui_state.layout.clone(),
                window,
                cx,
            )
        });
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
            RepoTabEvent::RequestExtensions => {
                self.open_extensions(cx);
            }
            RepoTabEvent::LayoutChanged(layout) => {
                self.ui_state.layout = layout.clone();
                self.ui_state.layout.normalize();
                for entry in &self.tabs {
                    if let TabContent::Repo(tab) = &entry.content {
                        tab.update(cx, |tab, cx| {
                            tab.set_layout(self.ui_state.layout.clone(), cx);
                        });
                    }
                }
                cx.notify();
            }
            RepoTabEvent::CommitActionChanged(action) => {
                self.set_commit_action((*action).into(), cx);
            }
            RepoTabEvent::AgentCommitRequested {
                id,
                repo_path,
                hint,
            } => {
                agent_connectivity::open_commit(
                    self,
                    *id,
                    repo_path.clone(),
                    hint.clone(),
                    cx,
                );
            }
            RepoTabEvent::AgentMergeRequested {
                id,
                repo_path,
                source,
            } => {
                agent_connectivity::open_merge(
                    self,
                    *id,
                    repo_path.clone(),
                    source.clone(),
                    cx,
                );
            }
            RepoTabEvent::AgentMergeResolveRequested {
                id,
                repo_path,
                merge_head,
                baseline_head,
            } => {
                agent_connectivity::open_merge_resolution(
                    self,
                    *id,
                    repo_path.clone(),
                    merge_head.clone(),
                    baseline_head.clone(),
                    cx,
                );
            }
            RepoTabEvent::AgentRebaseRequested {
                id,
                repo_path,
                source,
            } => {
                agent_connectivity::open_rebase(
                    self,
                    *id,
                    repo_path.clone(),
                    source.clone(),
                    cx,
                );
            }
            RepoTabEvent::AgentRebaseResolveRequested {
                id,
                repo_path,
                rebase_head,
                upstream_oid,
                baseline_head,
            } => {
                agent_connectivity::open_rebase_resolution(
                    self,
                    *id,
                    repo_path.clone(),
                    rebase_head.clone(),
                    upstream_oid.clone(),
                    baseline_head.clone(),
                    cx,
                );
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
            let was_opened = next_tab.update(cx, |tab, cx| tab.activate(cx));
            if should_refresh_after_switch(changed, was_opened) {
                let refresh_requested = next_tab
                    .update(cx, |tab, cx| tab.refresh_on_tab_switch(cx));
                if refresh_requested {
                    log::debug!(
                        "[tab_switch_refresh] refresh requested for tab {id}"
                    );
                } else {
                    log::debug!(
                        "[tab_switch_refresh] refresh skipped for tab {id}: tab busy"
                    );
                }
            } else if changed {
                log::debug!(
                    "[tab_switch_refresh] refresh skipped for tab {id}: initial load"
                );
            }
        }
        if changed {
            log::info!("[workspace_tabs] tab activated");
        }
        changed
    }

    fn close_tab(&mut self, id: TabId, cx: &mut Context<Self>) {
        self.request_tab_close(id, cx);
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
                .text_size(crate::theme::scaled_text_size(11.))
                .text_color(colors.blue)
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
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
                                .text_size(crate::theme::scaled_text_size(16.))
                                .child(crate::git::lucide("git-branch")),
                        )
                        .child(
                            div()
                                .text_size(crate::theme::scaled_text_size(12.))
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

    fn handle_open_extensions(
        &mut self,
        _: &app_menu::OpenExtensions,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("[app_menu] open extensions action");
        self.open_extensions(cx);
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

    /// Open the first dropped folder as a repository tab. A drop that contains
    /// no directory is ignored so an accidental file drop cannot create an
    /// invalid repository tab.
    pub(super) fn open_dropped_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = paths.iter().find(|path| path.is_dir()) else {
            log::info!(
                "[workspace_drop] ignored drop of {} path(s); no directory present",
                paths.len()
            );
            return;
        };
        let path = path.to_string_lossy().into_owned();
        log::info!("[workspace_drop] opening dropped folder");
        self.open_repo_path(path, false, window, cx);
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
}

impl Render for Workspace {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        // Modals registered via `window.open_dialog` are drawn by the app:
        // Root::render does not include the dialog layer (checkout
        // confirmation, commit message viewer, ...).
        let dialog_layer = Root::render_dialog_layer(window, cx);
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
            .on_action(cx.listener(Self::handle_open_extensions))
            .on_action(cx.listener(Self::handle_open_about))
            .child(self.title_bar(window, cx))
            .child(content)
            .when(self.show_settings, |element| {
                element.child(self.settings_overlay())
            })
            .when(self.pending_close.is_some(), |element| {
                element.child(self.close_confirmation_overlay(cx))
            })
            .children(dialog_layer)
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

    fn settings_overlay(&self) -> impl IntoElement {
        self.settings_panel.clone()
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
                    .text_size(crate::theme::scaled_text_size(11.))
                    .text_color(colors.muted_foreground)
                    .child(i18n::text(self.locale, "status-no-repo-selected")),
            )
    }
}
