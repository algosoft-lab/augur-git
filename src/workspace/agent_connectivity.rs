//! Visible sessions for configured external Agent CLIs.
//!
//! Connectivity tests and Git operations are deliberately normal interactive
//! PTY windows. The user can see and use provider login, approval, and
//! follow-up prompts while Augur Git coordinates only process lifecycle.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, TitleBar, h_flex, v_flex,
};

use crate::agent::{
    AgentConnectivityChallenge, AgentLaunchSpec, AgentOperation,
    AgentTestDirectory, ResolvedAgentProfile,
};
use crate::core::i18n::{self, Locale};
use crate::terminal::{
    TerminalBackend, TerminalView, normalize_working_directory,
};

use super::Workspace;
use super::tabs::TabId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentSessionKind {
    Connectivity,
    Commit,
}

#[derive(Clone)]
struct CommitCompletion {
    workspace: WeakEntity<Workspace>,
    tab_id: TabId,
    session_id: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum ConnectivityState {
    Starting,
    WaitingForResponse,
    ResponseReceived,
    Exited {
        code: Option<i32>,
        response_received: bool,
    },
    Failed(String),
}

/// Root view for one standalone, visible external Agent session window.
pub(super) struct AgentSessionWindow {
    kind: AgentSessionKind,
    locale: Locale,
    profile: ResolvedAgentProfile,
    spec: AgentLaunchSpec,
    prompt_preview: String,
    test_directory: Option<AgentTestDirectory>,
    working_directory: PathBuf,
    commit_completion: Option<CommitCompletion>,
    backend: Option<Arc<TerminalBackend>>,
    terminal: Option<Entity<TerminalView>>,
    state: ConnectivityState,
    response_received: bool,
    stop_requested: bool,
    // Keep the polling task owned by the window for its whole lifetime.
    _monitor_task: Option<Task<()>>,
}

impl AgentSessionWindow {
    fn new_connectivity(
        locale: Locale,
        profile: ResolvedAgentProfile,
        spec: AgentLaunchSpec,
        challenge: AgentConnectivityChallenge,
        test_directory: Option<AgentTestDirectory>,
        startup_error: Option<String>,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let working_directory = test_directory
            .as_ref()
            .map(|directory| directory.path().to_path_buf())
            .unwrap_or_default();
        Self::new_inner(
            AgentSessionKind::Connectivity,
            locale,
            profile,
            spec,
            challenge.prompt.clone(),
            test_directory,
            working_directory,
            None,
            startup_error,
            Some(challenge),
            window_id,
            cx,
        )
    }

    fn new_commit(
        locale: Locale,
        profile: ResolvedAgentProfile,
        spec: AgentLaunchSpec,
        prompt: String,
        working_directory: PathBuf,
        completion: Option<CommitCompletion>,
        startup_error: Option<String>,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_inner(
            AgentSessionKind::Commit,
            locale,
            profile,
            spec,
            prompt,
            None,
            working_directory,
            completion,
            startup_error,
            None,
            window_id,
            cx,
        )
    }

    fn new_inner(
        kind: AgentSessionKind,
        locale: Locale,
        profile: ResolvedAgentProfile,
        spec: AgentLaunchSpec,
        prompt_preview: String,
        test_directory: Option<AgentTestDirectory>,
        working_directory: PathBuf,
        commit_completion: Option<CommitCompletion>,
        startup_error: Option<String>,
        challenge: Option<AgentConnectivityChallenge>,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut backend = None;
        let mut terminal = None;
        let mut state = ConnectivityState::Starting;
        let mut monitor_task = None;
        let working_directory = normalize_working_directory(&working_directory);

        if let Some(error) = startup_error {
            state = ConnectivityState::Failed(error);
        } else {
            match TerminalBackend::spawn(
                &spec,
                test_directory.clone(),
                &working_directory,
                window_id,
            ) {
                Ok(value) => {
                    let value = Arc::new(value);
                    let terminal_entity =
                        cx.new(|cx| TerminalView::new(value.clone(), cx));
                    let window_entity = cx.entity();
                    let terminal_for_monitor = terminal_entity.clone();
                    let backend_for_monitor = value.clone();
                    let expected_response = challenge
                        .as_ref()
                        .map(|challenge| challenge.expected_response.clone());
                    let profile_id_for_monitor = profile.id.clone();
                    let kind_for_monitor = kind;
                    monitor_task = Some(cx.spawn(async move |_, cx| {
                        loop {
                            cx.background_executor()
                                .timer(Duration::from_millis(100))
                                .await;
                            let completion = terminal_for_monitor
                                .read_with(cx, |terminal, _| {
                                    terminal.completion()
                                });
                            let response_received = expected_response
                                .as_deref()
                                .is_some_and(|expected| {
                                    backend_for_monitor.contains_text(expected)
                                });
                            let finished = completion.is_some();
                            let mut commit_completion = None;
                            let _ = window_entity.update(cx, |window, cx| {
                                if response_received && !window.response_received {
                                    window.response_received = true;
                                    if kind_for_monitor
                                        == AgentSessionKind::Connectivity
                                    {
                                        log::info!(
                                            "[agent_terminal] connectivity test response received: profile={}",
                                            profile_id_for_monitor
                                        );
                                    }
                                }
                                if let Some(result) = completion.clone() {
                                    window.state = match result {
                                        Ok(code) => {
                                            log::info!(
                                                "[agent_terminal] {} exited: profile={}, code={code:?}",
                                                session_kind_label(kind_for_monitor),
                                                profile_id_for_monitor
                                            );
                                            ConnectivityState::Exited {
                                                code,
                                                response_received: window
                                                    .response_received,
                                            }
                                        }
                                        Err(summary) => {
                                            log::error!(
                                                "[agent_terminal] {} failed: profile={}",
                                                session_kind_label(kind_for_monitor),
                                                profile_id_for_monitor
                                            );
                                            ConnectivityState::Failed(summary)
                                        }
                                    };
                                    if kind_for_monitor
                                        == AgentSessionKind::Commit
                                    {
                                        let code = match completion.as_ref() {
                                            Some(Ok(code)) => *code,
                                            _ => None,
                                        };
                                        commit_completion = window
                                            .commit_completion
                                            .take()
                                            .map(|completion| {
                                                (completion, code)
                                            });
                                    }
                                } else if window.response_received {
                                    window.state =
                                        ConnectivityState::ResponseReceived;
                                } else if matches!(
                                    window.state,
                                    ConnectivityState::Starting
                                ) {
                                    window.state =
                                        ConnectivityState::WaitingForResponse;
                                }
                                cx.notify();
                            });
                            if let Some((completion, code)) = commit_completion {
                                let tab_id = completion.tab_id;
                                let session_id = completion.session_id;
                                let _ = completion.workspace.update(
                                    cx,
                                    move |workspace, cx| {
                                        workspace.finish_agent_commit(
                                            tab_id,
                                            session_id,
                                            code,
                                            cx,
                                        );
                                    },
                                );
                            }
                            if finished {
                                break;
                            }
                        }
                    }));
                    backend = Some(value);
                    terminal = Some(terminal_entity);
                    log::info!(
                        "[agent_terminal] {} started: profile={}",
                        session_kind_label(kind),
                        profile.id
                    );
                }
                Err(error) => {
                    state = ConnectivityState::Failed(
                        first_line(&error.to_string()).to_string(),
                    );
                    if let Some(directory) = test_directory.as_ref() {
                        if directory.cleanup().is_err() {
                            log::debug!(
                                "[agent_terminal] temporary test directory cleanup deferred"
                            );
                        }
                    }
                }
            }
        }

        Self {
            kind,
            locale,
            profile,
            spec,
            prompt_preview,
            test_directory,
            working_directory,
            commit_completion,
            backend,
            terminal,
            state,
            response_received: false,
            stop_requested: false,
            _monitor_task: monitor_task,
        }
    }

    pub(super) fn is_running(&self) -> bool {
        matches!(
            self.state,
            ConnectivityState::Starting
                | ConnectivityState::WaitingForResponse
                | ConnectivityState::ResponseReceived
        ) && !self.stop_requested
    }

    pub(super) fn label(&self) -> String {
        match self.kind {
            AgentSessionKind::Connectivity => {
                format!("{} connectivity test", self.profile.name)
            }
            AgentSessionKind::Commit => format!("{} commit", self.profile.name),
        }
    }

    pub(super) fn started(&self) -> bool {
        self.backend.is_some()
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.is_running() {
            return;
        }
        self.stop_requested = true;
        if let Some(backend) = &self.backend {
            backend.shutdown();
        }
        self.state = ConnectivityState::Exited {
            code: None,
            response_received: self.response_received,
        };
        if let Some(completion) = self.commit_completion.take() {
            let tab_id = completion.tab_id;
            let session_id = completion.session_id;
            let _ = cx
                .spawn(async move |_, cx| {
                    let _ = completion.workspace.update(
                        cx,
                        move |workspace, cx| {
                            workspace.finish_agent_commit(
                                tab_id, session_id, None, cx,
                            );
                        },
                    );
                })
                .detach();
        }
        log::info!(
            "[agent_terminal] {} termination requested: profile={}",
            session_kind_label(self.kind),
            self.profile.id,
        );
        cx.notify();
    }

    fn state_label(&self) -> String {
        if self.kind == AgentSessionKind::Commit {
            return match &self.state {
                ConnectivityState::Starting => {
                    i18n::text(self.locale, "agent-commit-status-starting")
                }
                ConnectivityState::WaitingForResponse
                | ConnectivityState::ResponseReceived => {
                    i18n::text(self.locale, "agent-commit-status-running")
                }
                ConnectivityState::Exited { code, .. } => {
                    let suffix = code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| {
                            i18n::text(self.locale, "agent-test-exit-unknown")
                        });
                    i18n::text_args(
                        self.locale,
                        "agent-commit-status-exited",
                        &[("code", &suffix)],
                    )
                }
                ConnectivityState::Failed(summary) => i18n::text_args(
                    self.locale,
                    "agent-commit-status-failed",
                    &[("error", summary)],
                ),
            };
        }
        match &self.state {
            ConnectivityState::Starting => {
                i18n::text(self.locale, "agent-test-status-starting")
            }
            ConnectivityState::WaitingForResponse => {
                i18n::text(self.locale, "agent-test-status-waiting")
            }
            ConnectivityState::ResponseReceived => {
                i18n::text(self.locale, "agent-test-status-response")
            }
            ConnectivityState::Exited {
                code,
                response_received,
            } => {
                let suffix = code
                    .map(|code| {
                        i18n::text_args(
                            self.locale,
                            "agent-test-exit-code",
                            &[("code", &code.to_string())],
                        )
                    })
                    .unwrap_or_else(|| {
                        i18n::text(self.locale, "agent-test-exit-unknown")
                    });
                if *response_received {
                    format!(
                        "{} · {} · {}",
                        i18n::text(self.locale, "agent-test-status-response"),
                        i18n::text(self.locale, "agent-test-status-exited"),
                        suffix
                    )
                } else {
                    format!(
                        "{} · {}",
                        i18n::text(self.locale, "agent-test-status-exited"),
                        suffix
                    )
                }
            }
            ConnectivityState::Failed(summary) => i18n::text_args(
                self.locale,
                "agent-test-status-failed",
                &[("error", summary)],
            ),
        }
    }

    fn metadata_row(
        &self,
        label: String,
        value: String,
        colors: &gpui_component::theme::ThemeColor,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_start()
            .gap_3()
            .child(
                div()
                    .w(px(150.))
                    .flex_shrink_0()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.foreground)
                    .whitespace_normal()
                    .child(SharedString::from(value)),
            )
    }
}

impl Drop for AgentSessionWindow {
    fn drop(&mut self) {
        if let Some(backend) = &self.backend {
            backend.shutdown();
        }
        if let Some(directory) = &self.test_directory {
            if directory.cleanup().is_err() {
                log::debug!(
                    "[agent_terminal] temporary test directory cleanup deferred"
                );
            }
        }
    }
}

impl Render for AgentSessionWindow {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let stop = this.clone();
        let status = self.state_label();
        let can_stop = self.is_running();
        let cwd = if self.working_directory.as_os_str().is_empty() {
            i18n::text(self.locale, "agent-test-temp-directory-unavailable")
        } else {
            self.working_directory.display().to_string()
        };
        let argv = if self.spec.args.is_empty() {
            vec![i18n::text(self.locale, "agent-test-no-arguments")]
        } else {
            self.spec
                .args
                .iter()
                .enumerate()
                .map(|(index, argument)| format!("[{index}] {argument}"))
                .collect()
        };
        let terminal = self
            .terminal
            .clone()
            .map(|terminal| terminal.into_any_element())
            .unwrap_or_else(|| {
                v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .child(div().text_color(colors.muted_foreground).child(
                        i18n::text(
                            self.locale,
                            "agent-test-terminal-unavailable",
                        ),
                    ))
                    .into_any_element()
            });
        let error = match &self.state {
            ConnectivityState::Failed(summary) => Some(summary.clone()),
            ConnectivityState::Exited {
                response_received: false,
                ..
            } if self.kind == AgentSessionKind::Connectivity => {
                Some(i18n::text(self.locale, "agent-test-no-response"))
            }
            _ => None,
        };

        v_flex()
            .id("agent-session-window")
            .size_full()
            .bg(colors.background)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::Bot).size(px(15.)))
                        .child(
                            div()
                                .text_color(colors.foreground)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(
                                    if self.kind == AgentSessionKind::Commit {
                                        i18n::text(
                                            self.locale,
                                            "agent-commit-window-title",
                                        )
                                    } else {
                                        i18n::text(
                                            self.locale,
                                            "agent-test-window-title",
                                        )
                                    },
                                ),
                        ),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .p_4()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_color(colors.muted_foreground)
                                    .text_size(crate::theme::scaled_text_size(
                                        12.,
                                    ))
                                    .child(i18n::text(
                                        self.locale,
                                        "agent-test-profile",
                                    )),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(colors.foreground)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.profile.name.clone()),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_color(colors.muted_foreground)
                                    .text_size(crate::theme::scaled_text_size(
                                        12.,
                                    ))
                                    .child(i18n::text(
                                        self.locale,
                                        "agent-test-status-label",
                                    )),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .text_color(
                                        if matches!(
                                            self.state,
                                            ConnectivityState::Failed(_)
                                        ) {
                                            colors.red
                                        } else if self.response_received {
                                            colors.green
                                        } else {
                                            colors.foreground
                                        },
                                    )
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(status)),
                            )
                            .child(
                                Button::new("agent-connectivity-stop")
                                    .label(
                                        if self.kind == AgentSessionKind::Commit
                                        {
                                            i18n::text(
                                                self.locale,
                                                "agent-commit-stop",
                                            )
                                        } else {
                                            i18n::text(
                                                self.locale,
                                                "agent-test-stop",
                                            )
                                        },
                                    )
                                    .danger()
                                    .small()
                                    .disabled(!can_stop)
                                    .on_click(move |_event, _window, cx| {
                                        stop.update(cx, |test, cx| {
                                            test.stop(cx)
                                        });
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("agent-connectivity-details")
                            .w_full()
                            .max_h(px(140.))
                            .flex_shrink_0()
                            .overflow_y_scroll()
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        self.metadata_row(
                                            i18n::text(
                                                self.locale,
                                                "agent-test-executable",
                                            ),
                                            self.spec
                                                .executable
                                                .display()
                                                .to_string(),
                                            &colors,
                                        ),
                                    )
                                    .child(self.metadata_row(
                                        i18n::text(
                                            self.locale,
                                            "agent-test-arguments",
                                        ),
                                        argv.join("\n"),
                                        &colors,
                                    ))
                                    .child(self.metadata_row(
                                        i18n::text(
                                            self.locale,
                                            "agent-test-working-directory",
                                        ),
                                        cwd,
                                        &colors,
                                    ))
                                    .child(self.metadata_row(
                                        i18n::text(
                                            self.locale,
                                            "agent-test-prompt",
                                        ),
                                        self.prompt_preview.clone(),
                                        &colors,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .text_size(crate::theme::scaled_text_size(11.))
                            .child(i18n::text(
                                self.locale,
                                if self.kind == AgentSessionKind::Commit {
                                    "agent-commit-note"
                                } else {
                                    "agent-test-temp-note"
                                },
                            )),
                    )
                    .when_some(error, |element, error| {
                        element.child(
                            div()
                                .text_color(colors.red)
                                .text_size(crate::theme::scaled_text_size(12.))
                                .child(SharedString::from(error)),
                        )
                    })
                    .child(
                        div()
                            .id("agent-connectivity-terminal")
                            .flex_1()
                            .min_h(px(220.))
                            .min_w_0()
                            .border_1()
                            .border_color(colors.border)
                            .child(terminal),
                    ),
            )
    }
}

/// Open or activate a connectivity window for one configured profile.
pub(super) fn open(
    workspace: &mut Workspace,
    profile_id: String,
    cx: &mut Context<Workspace>,
) {
    let key = connectivity_key(&profile_id);
    workspace
        .agent_sessions
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    if let Some((_, handle)) =
        workspace.agent_sessions.iter().find(|(id, _)| id == &key)
    {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }

    let profile = workspace.config.agent.profile(&profile_id);
    let Some(profile) = profile else {
        log::warn!(
            "[agent_terminal] connectivity test ignored invalid profile: profile={}",
            profile_id
        );
        return;
    };
    let locale = workspace.locale;
    let challenge = AgentConnectivityChallenge::new();
    let (spec, launch_error) =
        launch_for_profile(workspace, &profile, &challenge.prompt, cx);
    let (working_directory, startup_error) = match AgentTestDirectory::create()
    {
        Ok(directory) => (Some(directory), launch_error),
        Err(error) => (None, Some(first_line(&error.to_string()).to_string())),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            size(px(1120.), px(760.)),
            cx,
        )),
        is_resizable: true,
        kind: WindowKind::Normal,
        window_decorations: Some(WindowDecorations::Client),
        window_min_size: Some(size(px(760.), px(520.))),
        ..TitleBar::window_options()
    };
    let profile_for_window = profile.clone();
    let spec_for_window = spec.clone();
    let challenge_for_window = challenge.clone();
    let directory_for_window = working_directory.clone();
    let startup_error_for_window = startup_error.clone();
    log::info!(
        "[agent_terminal] opening connectivity test: profile={}",
        profile.id
    );
    match cx.open_window(options, move |window, cx| {
        let test = cx.new(|cx| {
            AgentSessionWindow::new_connectivity(
                locale,
                profile_for_window,
                spec_for_window,
                challenge_for_window,
                directory_for_window,
                startup_error_for_window,
                window.window_handle().window_id().as_u64(),
                cx,
            )
        });
        let weak_test = test.downgrade();
        window.on_window_should_close(cx, move |_window, app| {
            let _ = weak_test.update(app, |test, cx| test.stop(cx));
            true
        });
        window.activate_window();
        test
    }) {
        Ok(handle) => workspace.agent_sessions.push((key, handle)),
        Err(_error) => {
            log::error!(
                "[agent_terminal] failed to open connectivity test window: profile={profile_id}"
            );
            if let Some(directory) = working_directory {
                if directory.cleanup().is_err() {
                    log::debug!(
                        "[agent_terminal] temporary test directory cleanup deferred"
                    );
                }
            }
        }
    }
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_session_id() -> u64 {
    SESSION_COUNTER.fetch_add(1, Ordering::Relaxed).max(1)
}

fn session_kind_label(kind: AgentSessionKind) -> &'static str {
    match kind {
        AgentSessionKind::Connectivity => "connectivity test",
        AgentSessionKind::Commit => "commit session",
    }
}

fn connectivity_key(profile_id: &str) -> String {
    format!("connectivity:{profile_id}")
}

fn commit_key(repo_path: &str) -> String {
    format!("commit:{repo_path}")
}

fn launch_for_profile(
    workspace: &Workspace,
    profile: &ResolvedAgentProfile,
    prompt: &str,
    cx: &App,
) -> (AgentLaunchSpec, Option<String>) {
    let overrides = workspace.config.agent.launch_overrides_for(profile);
    let variant_unsupported = profile.built_in
        == Some(crate::agent::BuiltInAgent::OpenCode)
        && overrides.variant.is_some()
        && workspace
            .settings_panel
            .read(cx)
            .agent_supports_interactive_variant(&profile.id)
            == Some(false);
    let (mut spec, mut startup_error) = if variant_unsupported {
        (
            profile.launch_spec_for_prompt(prompt),
            Some(i18n::text(
                workspace.locale,
                "agent-opencode-variant-unsupported",
            )),
        )
    } else {
        match profile.launch_spec_for_prompt_with_overrides(prompt, &overrides)
        {
            Ok(spec) => (spec, None),
            Err(error) => (
                profile.launch_spec_for_prompt(prompt),
                Some(first_line(&error.to_string()).to_string()),
            ),
        }
    };
    if startup_error.is_none() {
        startup_error = match crate::agent::resolve_executable(&spec.executable)
        {
            Ok(executable) => {
                spec.executable = executable;
                None
            }
            Err(error) => Some(first_line(&error.to_string()).to_string()),
        };
    }
    (spec, startup_error)
}

fn open_session_window(
    workspace: &mut Workspace,
    key: String,
    kind: AgentSessionKind,
    locale: Locale,
    profile: ResolvedAgentProfile,
    spec: AgentLaunchSpec,
    prompt_preview: String,
    test_directory: Option<AgentTestDirectory>,
    working_directory: PathBuf,
    completion: Option<CommitCompletion>,
    startup_error: Option<String>,
    begin_agent: Option<(TabId, u64)>,
    cx: &mut Context<Workspace>,
) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            size(px(1120.), px(760.)),
            cx,
        )),
        is_resizable: true,
        kind: WindowKind::Normal,
        window_decorations: Some(WindowDecorations::Client),
        window_min_size: Some(size(px(760.), px(520.))),
        ..TitleBar::window_options()
    };
    let profile_for_window = profile.clone();
    let spec_for_window = spec.clone();
    let prompt_for_window = prompt_preview.clone();
    let directory_for_window = test_directory.clone();
    let working_directory_for_window = working_directory.clone();
    let completion_for_window = completion.clone();
    let startup_error_for_window = startup_error.clone();
    log::info!(
        "[agent_terminal] opening {}: profile={}",
        session_kind_label(kind),
        profile.id
    );
    match cx.open_window(options, move |window, cx| {
        let session = cx.new(|cx| match kind {
            AgentSessionKind::Connectivity => {
                let challenge = AgentConnectivityChallenge {
                    prompt: prompt_for_window.clone(),
                    expected_response: String::new(),
                };
                AgentSessionWindow::new_connectivity(
                    locale,
                    profile_for_window,
                    spec_for_window,
                    challenge,
                    directory_for_window,
                    startup_error_for_window,
                    window.window_handle().window_id().as_u64(),
                    cx,
                )
            }
            AgentSessionKind::Commit => AgentSessionWindow::new_commit(
                locale,
                profile_for_window,
                spec_for_window,
                prompt_for_window,
                working_directory_for_window,
                completion_for_window,
                startup_error_for_window,
                window.window_handle().window_id().as_u64(),
                cx,
            ),
        });
        let weak_session = session.downgrade();
        window.on_window_should_close(cx, move |_window, app| {
            let _ = weak_session.update(app, |session, cx| session.stop(cx));
            true
        });
        window.activate_window();
        session
    }) {
        Ok(handle) => {
            let started =
                handle.read(cx).is_ok_and(|session| session.started());
            workspace.agent_sessions.push((key, handle));
            if let Some((tab_id, session_id)) = begin_agent
                && started
            {
                workspace.begin_agent_commit(tab_id, session_id, cx);
            }
        }
        Err(_error) => {
            log::error!(
                "[agent_terminal] failed to open {}: profile={}",
                session_kind_label(kind),
                profile.id
            );
            if let Some(directory) = test_directory {
                if directory.cleanup().is_err() {
                    log::debug!(
                        "[agent_terminal] temporary test directory cleanup deferred"
                    );
                }
            }
        }
    }
}

/// Open or activate the visible Agent session that performs one repository
/// commit using the fixed operation prompt.
pub(super) fn open_commit(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    hint: String,
    cx: &mut Context<Workspace>,
) {
    workspace
        .agent_sessions
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    let key = commit_key(&repo_path);
    if let Some((_, handle)) =
        workspace.agent_sessions.iter().find(|(entry_key, handle)| {
            entry_key == &key
                && handle.read(cx).is_ok_and(|session| session.is_running())
        })
    {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }

    let locale = workspace.locale;
    let session_id = next_session_id();
    let fixed_prompt =
        AgentOperation::Commit.prompt(None).unwrap_or_else(|_| {
            "Commit the current repository changes.".to_string()
        });
    let profile_id = workspace.config.agent.default_profile_id();
    let (profile, prompt, spec, startup_error) = match workspace
        .config
        .agent
        .profile(&profile_id)
    {
        Some(profile) => match AgentOperation::Commit.prompt(Some(&hint)) {
            Ok(prompt) => {
                let (spec, launch_error) =
                    launch_for_profile(workspace, &profile, &prompt, cx);
                (profile, prompt, spec, launch_error)
            }
            Err(error) => {
                let (spec, _) =
                    launch_for_profile(workspace, &profile, &fixed_prompt, cx);
                (
                    profile,
                    fixed_prompt.clone(),
                    spec,
                    Some(first_line(&error.to_string()).to_string()),
                )
            }
        },
        None => {
            let profile = ResolvedAgentProfile {
                id: profile_id.clone(),
                name: profile_id.clone(),
                executable: PathBuf::new(),
                args: Vec::new(),
                prompt_mode: crate::agent::PromptMode::TrailingArgument,
                built_in: None,
            };
            (
                profile,
                fixed_prompt.clone(),
                AgentLaunchSpec {
                    executable: PathBuf::new(),
                    args: Vec::new(),
                },
                Some(i18n::text_args(
                    locale,
                    "agent-commit-invalid-profile",
                    &[("profile", &profile_id)],
                )),
            )
        }
    };
    let completion = CommitCompletion {
        workspace: cx.entity().downgrade(),
        tab_id,
        session_id,
    };
    open_session_window(
        workspace,
        key,
        AgentSessionKind::Commit,
        locale,
        profile,
        spec,
        prompt,
        None,
        PathBuf::from(repo_path),
        Some(completion),
        startup_error,
        Some((tab_id, session_id)),
        cx,
    );
}

pub(super) fn running_count(workspace: &Workspace, cx: &App) -> usize {
    workspace
        .agent_sessions
        .iter()
        .filter(|(_, handle)| {
            handle.read(cx).is_ok_and(|view| view.is_running())
        })
        .count()
}

pub(super) fn running_labels(workspace: &Workspace, cx: &App) -> Vec<String> {
    workspace
        .agent_sessions
        .iter()
        .filter_map(|(_, handle)| {
            handle
                .read(cx)
                .ok()
                .filter(|view| view.is_running())
                .map(AgentSessionWindow::label)
        })
        .collect()
}

pub(super) fn running_for_repo(
    workspace: &Workspace,
    repo_path: &str,
    cx: &App,
) -> usize {
    let key = commit_key(repo_path);
    workspace
        .agent_sessions
        .iter()
        .filter(|(entry_key, handle)| {
            entry_key == &key
                && handle.read(cx).is_ok_and(|view| view.is_running())
        })
        .count()
}

pub(super) fn running_labels_for_repo(
    workspace: &Workspace,
    repo_path: &str,
    cx: &App,
) -> Vec<String> {
    let key = commit_key(repo_path);
    workspace
        .agent_sessions
        .iter()
        .filter_map(|(entry_key, handle)| {
            (entry_key == &key)
                .then(|| handle.read(cx).ok())
                .flatten()
                .filter(|view| view.is_running())
                .map(AgentSessionWindow::label)
        })
        .collect()
}

pub(super) fn stop_all(workspace: &Workspace, cx: &mut Context<Workspace>) {
    for (_, handle) in &workspace.agent_sessions {
        let _ = handle.update(cx, |view, _, cx| view.stop(cx));
    }
}

pub(super) fn stop_for_repo(
    workspace: &mut Workspace,
    repo_path: &str,
    cx: &mut Context<Workspace>,
) -> bool {
    let key = commit_key(repo_path);
    let mut stopped = false;
    for (entry_key, handle) in &workspace.agent_sessions {
        if entry_key == &key {
            let _ = handle.update(cx, |view, _, cx| {
                if view.is_running() {
                    stopped = true;
                    view.stop(cx);
                }
            });
        }
    }
    stopped
}

pub(super) fn set_locale(
    workspace: &Workspace,
    locale: Locale,
    cx: &mut Context<Workspace>,
) {
    for (_, handle) in &workspace.agent_sessions {
        let _ = handle.update(cx, |view, _, cx| {
            view.locale = locale;
            cx.notify();
        });
    }
}

impl Workspace {
    pub(super) fn begin_agent_commit(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| tab.begin_agent_commit(session_id, cx));
        }
    }

    pub(super) fn finish_agent_commit(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        code: Option<i32>,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| {
                tab.finish_agent_commit(session_id, code, cx)
            });
        }
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}
