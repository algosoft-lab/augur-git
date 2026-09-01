//! Agent task composer and per-repository Agent session tabs.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};

use crate::agent::{
    AgentSessionState, AgentSettings, ResolvedAgentProfile, ReviewContext,
};
use crate::core::i18n::{self, Locale};
use crate::terminal::{TerminalBackend, TerminalView};

use super::RepoTab;

#[derive(Clone, Debug)]
pub enum AgentTaskEvent {
    Cancel,
    Submit {
        profile_id: String,
        request: String,
        context: ReviewContext,
    },
}

#[derive(Clone, Debug)]
pub enum AgentSessionEvent {
    Exited { id: u64 },
}

/// Modal task editor shared by toolbar and review-panel entry points.
pub struct AgentTaskComposer {
    locale: Locale,
    settings: AgentSettings,
    profiles: Vec<(String, String)>,
    selected_profile: String,
    request: Entity<TextareaState>,
    context: ReviewContext,
    error: Option<String>,
}

impl EventEmitter<AgentTaskEvent> for AgentTaskComposer {}

impl AgentTaskComposer {
    pub fn new(
        settings: AgentSettings,
        context: ReviewContext,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected_profile = settings.default_profile_id();
        let profiles = profile_options(&settings);
        let request = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 12)
                .placeholder(i18n::text(locale, "agent-task-placeholder"))
        });
        Self {
            locale,
            settings,
            profiles,
            selected_profile,
            request,
            context,
            error: None,
        }
    }

    fn select_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        if self.settings.profile(&profile_id).is_some() {
            self.selected_profile = profile_id;
            self.error = None;
            cx.notify();
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let request = self.request.read(cx).value().to_string();
        if request.trim().is_empty() {
            self.error = Some(i18n::text(self.locale, "agent-task-empty"));
            cx.notify();
            return;
        }
        let profile_id = self.selected_profile.clone();
        if self.settings.profile(&profile_id).is_none() {
            self.error = Some(i18n::text(self.locale, "agent-profile-invalid"));
            cx.notify();
            return;
        }
        cx.emit(AgentTaskEvent::Submit {
            profile_id,
            request,
            context: self.context.clone(),
        });
    }

    fn profile_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let selected = self.selected_profile.clone();
        let profiles = self.profiles.clone();
        let label = self
            .profiles
            .iter()
            .find(|(id, _)| id == &selected)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| selected.clone());
        let this = cx.entity();
        Button::new("agent-profile-select")
            .label(label)
            .icon(IconName::ChevronDown)
            .ghost()
            .small()
            .dropdown_menu_with_anchor(
                Anchor::BottomLeft,
                move |menu, _window, _cx| {
                    profiles.iter().fold(menu, |menu, (id, name)| {
                        let id = id.clone();
                        let name = name.clone();
                        let item_entity = this.clone();
                        menu.item(
                            PopupMenuItem::new(name)
                                .checked(id == selected)
                                .on_click(move |_event, _window, cx| {
                                    item_entity.update(cx, |composer, cx| {
                                        composer.select_profile(id.clone(), cx);
                                    });
                                }),
                        )
                    })
                },
            )
    }

    fn context_summary(&self) -> String {
        let selection = match &self.context.selection {
            crate::agent::ReviewSelection::None => {
                i18n::text(self.locale, "agent-context-none")
            }
            crate::agent::ReviewSelection::WorkingTreeFile { path, .. } => {
                format!("working tree: {path}")
            }
            crate::agent::ReviewSelection::Commit { oid } => {
                format!("commit: {}", short_oid(oid))
            }
            crate::agent::ReviewSelection::CommitFile { oid, path } => {
                format!("commit {}: {path}", short_oid(oid))
            }
            crate::agent::ReviewSelection::Comparison {
                base,
                target,
                path,
            } => {
                let suffix = path
                    .as_ref()
                    .map(|path| format!(": {path}"))
                    .unwrap_or_default();
                format!("comparison {base} → {target}{suffix}")
            }
        };
        if self.context.branch.trim().is_empty() {
            selection
        } else {
            format!("{} · branch: {}", selection, self.context.branch)
        }
    }
}

impl Render for AgentTaskComposer {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let cancel = this.clone();
        let submit = this.clone();
        let context = self.context_summary();
        v_flex()
            .id("agent-task-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(colors.background.opacity(0.9))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .id("agent-task-card")
                    .w(px(640.))
                    .max_w(relative(0.9))
                    .gap_4()
                    .p_5()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .when(cx.theme().shadow, |element| element.shadow_lg())
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::new(IconName::Bot).size(px(16.)),
                                    )
                                    .child(
                                        div()
                                            .text_color(colors.foreground)
                                            .text_size(px(16.))
                                            .font_weight(FontWeight::BOLD)
                                            .child(SharedString::from(
                                                i18n::text(
                                                    self.locale,
                                                    "agent-task-title",
                                                ),
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("agent-task-close")
                                    .icon(IconName::Close)
                                    .ghost()
                                    .small()
                                    .on_click(move |_event, _window, cx| {
                                        cancel.update(cx, |composer, cx| {
                                            composer.cancel(cx);
                                        });
                                    }),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_color(colors.muted_foreground)
                                    .text_size(px(12.))
                                    .child(SharedString::from(i18n::text(
                                        self.locale,
                                        "agent-profile-label",
                                    ))),
                            )
                            .child(self.profile_button(cx)),
                    )
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .text_size(px(12.))
                            .child(SharedString::from(context)),
                    )
                    .child(Textarea::new(&self.request).w_full())
                    .when_some(self.error.clone(), |element, error| {
                        element.child(
                            div()
                                .text_color(colors.red)
                                .text_size(crate::theme::scaled_text_size(12.))
                                .child(SharedString::from(error)),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("agent-task-cancel")
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-cancel",
                                    ))
                                    .ghost()
                                    .on_click(move |_event, _window, cx| {
                                        this.update(cx, |composer, cx| {
                                            composer.cancel(cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new("agent-task-submit")
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-start",
                                    ))
                                    .primary()
                                    .on_click(move |_event, _window, cx| {
                                        submit.update(cx, |composer, cx| {
                                            composer.submit(cx);
                                        });
                                    }),
                            ),
                    ),
            )
    }
}

impl AgentTaskComposer {
    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(AgentTaskEvent::Cancel);
    }
}

/// One full-size repository subtab containing an interactive Agent terminal.
pub struct AgentSession {
    id: u64,
    title: String,
    profile_name: String,
    state: AgentSessionState,
    terminal: Entity<TerminalView>,
    backend: Arc<TerminalBackend>,
    monitor_task: Option<Task<()>>,
    locale: Locale,
}

impl EventEmitter<AgentSessionEvent> for AgentSession {}

impl AgentSession {
    pub fn new(
        id: u64,
        profile: ResolvedAgentProfile,
        request: String,
        backend: Arc<TerminalBackend>,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Self {
        let terminal = cx.new(|cx| TerminalView::new(backend.clone(), cx));
        let session_entity = cx.entity();
        let terminal_for_monitor = terminal.clone();
        let monitor_task = cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let (exited, last_activity) =
                    terminal_for_monitor.read_with(cx, |terminal, _| {
                        (terminal.completion(), terminal.last_activity())
                    });
                if let Some(result) = exited {
                    session_entity.update(cx, |session, cx| {
                        session.state = match result {
                            Ok(code) => AgentSessionState::Exited { code },
                            Err(summary) => {
                                AgentSessionState::Failed { summary }
                            }
                        };
                        cx.emit(AgentSessionEvent::Exited { id: session.id });
                        cx.notify();
                    });
                    break;
                } else {
                    session_entity.update(cx, |session, cx| {
                        if matches!(session.state, AgentSessionState::Starting)
                        {
                            session.state =
                                AgentSessionState::Running { last_activity };
                        } else if let AgentSessionState::Running {
                            last_activity: activity,
                        } = &mut session.state
                        {
                            *activity = last_activity;
                        }
                        // Refresh the status line periodically so "recent
                        // activity" ages while the terminal remains idle.
                        cx.notify();
                    });
                }
            }
        });
        let title = request
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(truncate_title)
            .unwrap_or_else(|| profile.name.clone());
        log::info!(
            "[agent_terminal] session started: id={}, profile={}",
            id,
            profile.id
        );
        Self {
            id,
            title,
            profile_name: profile.name,
            state: AgentSessionState::Starting,
            terminal,
            backend,
            monitor_task: Some(monitor_task),
            locale,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_running(&self) -> bool {
        matches!(
            self.state,
            AgentSessionState::Starting | AgentSessionState::Running { .. }
        )
    }

    pub fn terminate(&mut self) {
        if self.is_running() {
            log::info!(
                "[agent_terminal] session termination requested: id={}",
                self.id
            );
            self.backend.shutdown();
            self.state = AgentSessionState::Exited { code: None };
        }
        self.monitor_task = None;
    }

    fn state_label(&self) -> String {
        match &self.state {
            AgentSessionState::Starting => {
                i18n::text(self.locale, "agent-status-starting")
            }
            AgentSessionState::Running { .. } => {
                i18n::text(self.locale, "agent-status-running")
            }
            AgentSessionState::Exited { code: Some(code) } => i18n::text_args(
                self.locale,
                "agent-status-exited",
                &[("code", &code.to_string())],
            ),
            AgentSessionState::Exited { code: None } => {
                i18n::text(self.locale, "agent-status-exited-unknown")
            }
            AgentSessionState::Failed { summary } => i18n::text_args(
                self.locale,
                "agent-status-failed",
                &[("error", &summary)],
            ),
        }
    }

    fn recent_activity_label(&self) -> String {
        match self.state {
            AgentSessionState::Starting => {
                i18n::text(self.locale, "agent-status-starting")
            }
            AgentSessionState::Running { last_activity } => {
                let elapsed = last_activity.elapsed().as_secs();
                if elapsed == 0 {
                    i18n::text(self.locale, "agent-activity-now")
                } else {
                    i18n::text_args(
                        self.locale,
                        "agent-activity-seconds",
                        &[("seconds", &elapsed.to_string())],
                    )
                }
            }
            AgentSessionState::Exited { .. }
            | AgentSessionState::Failed { .. } => {
                i18n::text(self.locale, "agent-activity-finished")
            }
        }
    }

    pub(super) fn tab_label(&self) -> String {
        format!("{} · {}", self.profile_name, self.title)
    }
}

impl Render for AgentSession {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        v_flex()
            .id(SharedString::from(format!("agent-session-{}", self.id)))
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .w_full()
                    .h(px(28.))
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .bg(colors.tab_bar)
                    .border_b_1()
                    .border_color(colors.border)
                    .child(Icon::new(IconName::Bot).size(px(13.)))
                    .child(
                        div()
                            .text_color(colors.foreground)
                            .text_size(px(12.))
                            .truncate()
                            .child(SharedString::from(self.title.clone())),
                    )
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .text_size(px(11.))
                            .child(SharedString::from(format!(
                                "{} · {}",
                                self.profile_name,
                                self.state_label(),
                            ))),
                    )
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .text_size(px(10.))
                            .child(SharedString::from(
                                self.recent_activity_label(),
                            )),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.terminal.clone()))
    }
}

impl Drop for AgentSession {
    fn drop(&mut self) {
        self.terminate();
        log::info!("[agent_terminal] session dropped: id={}", self.id);
    }
}

pub fn profile_options(settings: &AgentSettings) -> Vec<(String, String)> {
    let mut options = crate::agent::BuiltInAgent::ALL
        .iter()
        .map(|agent| (agent.id().to_string(), agent.display_name().to_string()))
        .collect::<Vec<_>>();
    for profile in settings.valid_custom_profiles() {
        if options.iter().any(|(id, _)| id == &profile.id) {
            continue;
        }
        options.push((profile.id.clone(), profile.name.clone()));
    }
    options
}

fn short_oid(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

fn truncate_title(title: &str) -> String {
    let mut result = title.chars().take(80).collect::<String>();
    if title.chars().count() > 80 {
        result.push('…');
    }
    result
}

impl RepoTab {
    pub(super) fn open_agent_composer(
        &mut self,
        context: ReviewContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_composer.is_some() {
            return;
        }
        let composer = cx.new(|cx| {
            AgentTaskComposer::new(
                self.agent_settings.clone(),
                context,
                self.locale,
                window,
                cx,
            )
        });
        cx.subscribe(&composer, |tab, _composer, event, cx| match event {
            AgentTaskEvent::Cancel => {
                tab.agent_composer = None;
                cx.notify();
            }
            AgentTaskEvent::Submit {
                profile_id,
                request,
                context,
            } => {
                tab.agent_composer = None;
                if tab
                    .agent_sessions
                    .iter()
                    .any(|session| session.read(cx).is_running())
                {
                    tab.confirmation =
                        Some(super::PendingConfirmation::AgentSharedTree {
                            profile_id: profile_id.clone(),
                            request: request.clone(),
                            context: context.clone(),
                        });
                    cx.notify();
                } else {
                    tab.start_agent_session(
                        profile_id.clone(),
                        request.clone(),
                        context.clone(),
                        cx,
                    );
                }
            }
        })
        .detach();
        self.agent_composer = Some(composer);
        cx.notify();
    }

    pub(super) fn start_agent_session(
        &mut self,
        profile_id: String,
        request: String,
        context: ReviewContext,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self.agent_settings.profile(&profile_id) else {
            self.status_message =
                Some(i18n::text(self.locale, "agent-profile-invalid"));
            self.status_message_ok = Some(false);
            cx.notify();
            return;
        };
        let id = self.next_agent_session_id;
        self.next_agent_session_id =
            self.next_agent_session_id.wrapping_add(1).max(1);
        let document = crate::agent::task_document(&request, &context);
        let task_file = match self.task_store.write(&document) {
            Ok(task_file) => task_file,
            Err(error) => {
                self.status_message = Some(i18n::text_args(
                    self.locale,
                    "agent-start-failed",
                    &[("error", &first_line(&error.to_string()))],
                ));
                self.status_message_ok = Some(false);
                cx.notify();
                return;
            }
        };
        let spec = profile.launch_spec(task_file.path().to_path_buf());
        match TerminalBackend::spawn(
            &spec,
            Some(task_file),
            None,
            Path::new(&self.repo_path),
            id,
        ) {
            Ok(backend) => {
                let backend = Arc::new(backend);
                let session = cx.new(|cx| {
                    AgentSession::new(
                        id,
                        profile.clone(),
                        request.clone(),
                        backend.clone(),
                        self.locale,
                        cx,
                    )
                });
                cx.subscribe(&session, |tab, _session, event, cx| {
                    let AgentSessionEvent::Exited { id } = event;
                    log::info!("[agent_terminal] session exited: id={id}");
                    tab.refresh_after_agent_exit(cx);
                })
                .detach();
                self.agent_sessions.push(session);
                self.active_agent_session = Some(id);
                cx.notify();
            }
            Err(error) => {
                log::error!(
                    "[agent_terminal] session failed to start: id={id}, profile={profile_id}"
                );
                self.status_message = Some(i18n::text_args(
                    self.locale,
                    "agent-start-failed",
                    &[("error", &first_line(&error.to_string()))],
                ));
                self.status_message_ok = Some(false);
                cx.notify();
            }
        }
    }

    pub(crate) fn set_agent_settings(&mut self, settings: AgentSettings) {
        self.agent_settings = settings;
    }

    pub(super) fn refresh_after_agent_exit(&mut self, cx: &mut Context<Self>) {
        self.request_agent_refresh(cx);
    }

    pub(super) fn confirm_agent_shared_tree(&mut self, cx: &mut Context<Self>) {
        let Some(super::PendingConfirmation::AgentSharedTree {
            profile_id,
            request,
            context,
        }) = self.confirmation.take()
        else {
            return;
        };
        self.start_agent_session(profile_id, request, context, cx);
    }

    pub(super) fn request_agent_close(
        &mut self,
        id: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .agent_sessions
            .iter()
            .find(|session| session.read(cx).id() == id)
        else {
            return;
        };
        if session.read(cx).is_running() {
            self.confirmation =
                Some(super::PendingConfirmation::AgentSessionClose { id });
        } else {
            self.remove_agent_session(id, cx);
        }
        cx.notify();
    }

    pub(super) fn confirm_agent_close(&mut self, cx: &mut Context<Self>) {
        let Some(super::PendingConfirmation::AgentSessionClose { id }) =
            self.confirmation.take()
        else {
            return;
        };
        self.remove_agent_session(id, cx);
    }

    fn remove_agent_session(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(index) = self
            .agent_sessions
            .iter()
            .position(|session| session.read(cx).id() == id)
        {
            let session = self.agent_sessions.remove(index);
            session.update(cx, |session, _| session.terminate());
        }
        if self.active_agent_session == Some(id) {
            self.active_agent_session = self
                .agent_sessions
                .last()
                .map(|session| session.read(cx).id());
        }
        cx.notify();
    }

    pub(crate) fn terminate_agent_sessions(&mut self, cx: &mut Context<Self>) {
        for session in &self.agent_sessions {
            session.update(cx, |session, _| session.terminate());
        }
    }

    pub(crate) fn running_agent_session_count(&self, cx: &App) -> usize {
        self.agent_sessions
            .iter()
            .filter(|session| session.read(cx).is_running())
            .count()
    }

    pub(crate) fn running_agent_session_labels(&self, cx: &App) -> Vec<String> {
        self.agent_sessions
            .iter()
            .filter_map(|session| {
                let session = session.read(cx);
                session.is_running().then(|| session.tab_label())
            })
            .collect()
    }

    pub(super) fn render_agent_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors.clone();
        let tab = self.active_agent_session;
        let this = cx.entity();
        let review_entity = this.clone();
        let review = Button::new("agent-review-tab")
            .label(i18n::text(self.locale, "agent-review-tab"))
            .ghost()
            .small()
            .when(tab.is_none(), |button| button.primary())
            .on_click(move |_event, _window, cx| {
                review_entity.update(cx, |tab, cx| {
                    tab.active_agent_session = None;
                    tab.request_agent_refresh(cx);
                    cx.notify();
                });
            });
        let mut tabs = h_flex()
            .id("agent-subtabs")
            .w_full()
            .h(px(32.))
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_2()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .child(review);
        for session in &self.agent_sessions {
            let session_state = session.read(cx);
            let id = session_state.id();
            let active = tab == Some(id);
            let label = session_state.tab_label();
            let state_label = session_state.state_label();
            let activity_label = session_state.recent_activity_label();
            let select = this.clone();
            let close = this.clone();
            let item = h_flex()
                .id(SharedString::from(format!("agent-tab-{id}")))
                .items_center()
                .gap_1()
                .px_2()
                .rounded_md()
                .when(active, |element| element.bg(colors.input))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(colors.foreground)
                        .truncate()
                        .child(SharedString::from(label)),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(colors.muted_foreground)
                        .truncate()
                        .child(SharedString::from(state_label)),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(colors.muted_foreground)
                        .truncate()
                        .child(SharedString::from(activity_label)),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "agent-tab-close-{id}"
                    )))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .on_click(
                        move |_event, _window, cx| {
                            close.update(cx, |tab, cx| {
                                tab.request_agent_close(id, cx)
                            });
                        },
                    ),
                )
                .on_click(move |_event, _window, cx| {
                    select.update(cx, |tab, cx| {
                        tab.active_agent_session = Some(id);
                        cx.notify();
                    });
                });
            tabs = tabs.child(item);
        }
        let new_task = this.clone();
        tabs = tabs.child(
            Button::new("agent-new-task")
                .icon(IconName::Plus)
                .ghost()
                .small()
                .tooltip(i18n::text(self.locale, "agent-new-task"))
                .on_click(move |_event, window, cx| {
                    new_task.update(cx, |tab, cx| {
                        tab.open_agent_composer(
                            tab.review_context.clone(),
                            window,
                            cx,
                        );
                    });
                }),
        );
        let body = if let Some(id) = self.active_agent_session {
            self.agent_sessions
                .iter()
                .find(|session| session.read(cx).id() == id)
                .map(|session| session.clone().into_any_element())
                .unwrap_or_else(|| {
                    self.main_content(window, cx).into_any_element()
                })
        } else {
            self.main_content(window, cx).into_any_element()
        };
        v_flex()
            .size_full()
            .min_h_0()
            .child(tabs)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}
