//! Visible connectivity diagnostics for configured external Agent CLIs.
//!
//! A connectivity test is deliberately a normal interactive PTY window. The
//! user can see and use the provider's login, approval, and follow-up prompts;
//! Augur Git only observes the bounded terminal grid for the challenge reply.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, TitleBar, h_flex, v_flex,
};

use crate::agent::{
    AgentConnectivityChallenge, AgentLaunchSpec, AgentTestDirectory,
    ResolvedAgentProfile,
};
use crate::core::i18n::{self, Locale};
use crate::terminal::{TerminalBackend, TerminalView};

use super::Workspace;

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

/// Root view for one standalone, visible Agent connectivity test window.
pub(super) struct AgentConnectivityWindow {
    locale: Locale,
    profile: ResolvedAgentProfile,
    spec: AgentLaunchSpec,
    challenge: AgentConnectivityChallenge,
    working_directory: Option<AgentTestDirectory>,
    backend: Option<Arc<TerminalBackend>>,
    terminal: Option<Entity<TerminalView>>,
    state: ConnectivityState,
    response_received: bool,
    stop_requested: bool,
    monitor_task: Option<Task<()>>,
}

impl AgentConnectivityWindow {
    fn new(
        locale: Locale,
        profile: ResolvedAgentProfile,
        spec: AgentLaunchSpec,
        challenge: AgentConnectivityChallenge,
        working_directory: Option<AgentTestDirectory>,
        startup_error: Option<String>,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut backend = None;
        let mut terminal = None;
        let mut state = ConnectivityState::Starting;
        let mut monitor_task = None;

        if let Some(error) = startup_error {
            state = ConnectivityState::Failed(error);
        } else if let Some(directory) = working_directory.as_ref() {
            match TerminalBackend::spawn(
                &spec,
                Some(directory.clone()),
                directory.path(),
                window_id,
            ) {
                Ok(value) => {
                    let value = Arc::new(value);
                    let terminal_entity =
                        cx.new(|cx| TerminalView::new(value.clone(), cx));
                    let window_entity = cx.entity();
                    let terminal_for_monitor = terminal_entity.clone();
                    let backend_for_monitor = value.clone();
                    let expected_response = challenge.expected_response.clone();
                    let profile_id_for_monitor = profile.id.clone();
                    monitor_task = Some(cx.spawn(async move |_, cx| {
                        loop {
                            cx.background_executor()
                                .timer(Duration::from_millis(100))
                                .await;
                            let completion = terminal_for_monitor
                                .read_with(cx, |terminal, _| {
                                    terminal.completion()
                                });
                            let response_received = backend_for_monitor
                                .contains_text(&expected_response);
                            let finished = completion.is_some();
                            window_entity.update(cx, |window, cx| {
                                if window.stop_requested {
                                    return;
                                }
                                if response_received && !window.response_received {
                                    window.response_received = true;
                                    log::info!(
                                        "[agent_terminal] connectivity test response received: profile={}",
                                        profile_id_for_monitor
                                    );
                                }
                                if let Some(result) = completion {
                                    window.state = match result {
                                        Ok(code) => {
                                            log::info!(
                                                "[agent_terminal] connectivity test exited: profile={}, code={code:?}",
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
                                                "[agent_terminal] connectivity test failed: profile={}",
                                                profile_id_for_monitor
                                            );
                                            ConnectivityState::Failed(summary)
                                        }
                                    };
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
                            if finished {
                                break;
                            }
                        }
                    }));
                    backend = Some(value);
                    terminal = Some(terminal_entity);
                    log::info!(
                        "[agent_terminal] connectivity test started: profile={}",
                        profile.id
                    );
                }
                Err(error) => {
                    state = ConnectivityState::Failed(
                        first_line(&error.to_string()).to_string(),
                    );
                    if directory.cleanup().is_err() {
                        log::debug!(
                            "[agent_terminal] temporary test directory cleanup deferred"
                        );
                    }
                }
            }
        } else {
            state = ConnectivityState::Failed(i18n::text(
                locale,
                "agent-test-temp-directory-unavailable",
            ));
        }

        Self {
            locale,
            profile,
            spec,
            challenge,
            working_directory,
            backend,
            terminal,
            state,
            response_received: false,
            stop_requested: false,
            monitor_task,
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
        format!("{} connectivity test", self.profile.name)
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
        self.monitor_task = None;
        log::info!(
            "[agent_terminal] connectivity test termination requested: profile={}",
            self.profile.id
        );
        cx.notify();
    }

    fn state_label(&self) -> String {
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

impl Drop for AgentConnectivityWindow {
    fn drop(&mut self) {
        if let Some(backend) = &self.backend {
            backend.shutdown();
        }
        if let Some(directory) = &self.working_directory {
            if directory.cleanup().is_err() {
                log::debug!(
                    "[agent_terminal] temporary test directory cleanup deferred"
                );
            }
        }
    }
}

impl Render for AgentConnectivityWindow {
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
        let cwd = self
            .working_directory
            .as_ref()
            .map(|directory| directory.path().display().to_string())
            .unwrap_or_else(|| {
                i18n::text(self.locale, "agent-test-temp-directory-unavailable")
            });
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
            } => Some(i18n::text(self.locale, "agent-test-no-response")),
            _ => None,
        };

        v_flex()
            .id("agent-connectivity-window")
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
                                .child(i18n::text(
                                    self.locale,
                                    "agent-test-window-title",
                                )),
                        ),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .p_4()
                    .child(self.metadata_row(
                        i18n::text(self.locale, "agent-test-profile"),
                        self.profile.name.clone(),
                        &colors,
                    ))
                    .child(self.metadata_row(
                        i18n::text(self.locale, "agent-test-executable"),
                        self.spec.executable.display().to_string(),
                        &colors,
                    ))
                    .child(self.metadata_row(
                        i18n::text(self.locale, "agent-test-arguments"),
                        argv.join("\n"),
                        &colors,
                    ))
                    .child(self.metadata_row(
                        i18n::text(self.locale, "agent-test-working-directory"),
                        cwd,
                        &colors,
                    ))
                    .child(self.metadata_row(
                        i18n::text(self.locale, "agent-test-prompt"),
                        self.challenge.prompt.clone(),
                        &colors,
                    ))
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
                                        "agent-test-status-label",
                                    )),
                            )
                            .child(
                                div()
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
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-test-stop",
                                    ))
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
                            .text_color(colors.muted_foreground)
                            .text_size(crate::theme::scaled_text_size(11.))
                            .child(i18n::text(
                                self.locale,
                                "agent-test-temp-note",
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
                            .min_h_0()
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
    workspace
        .agent_connectivity_windows
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    if let Some((_, handle)) = workspace
        .agent_connectivity_windows
        .iter()
        .find(|(id, _)| id == &profile_id)
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
    let mut spec = profile.launch_spec_for_prompt(&challenge.prompt);
    let startup_error = match crate::agent::resolve_executable(&spec.executable)
    {
        Ok(executable) => {
            spec.executable = executable;
            None
        }
        Err(error) => Some(first_line(&error.to_string()).to_string()),
    };
    let (working_directory, startup_error) = match AgentTestDirectory::create()
    {
        Ok(directory) => (Some(directory), startup_error),
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
            AgentConnectivityWindow::new(
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
        Ok(handle) => workspace
            .agent_connectivity_windows
            .push((profile_id, handle)),
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

pub(super) fn running_count(workspace: &Workspace, cx: &App) -> usize {
    workspace
        .agent_connectivity_windows
        .iter()
        .filter(|(_, handle)| {
            handle.read(cx).is_ok_and(|view| view.is_running())
        })
        .count()
}

pub(super) fn running_labels(workspace: &Workspace, cx: &App) -> Vec<String> {
    workspace
        .agent_connectivity_windows
        .iter()
        .filter_map(|(_, handle)| {
            handle
                .read(cx)
                .ok()
                .filter(|view| view.is_running())
                .map(AgentConnectivityWindow::label)
        })
        .collect()
}

pub(super) fn stop_all(workspace: &Workspace, cx: &mut Context<Workspace>) {
    for (_, handle) in &workspace.agent_connectivity_windows {
        let _ = handle.update(cx, |view, _, cx| view.stop(cx));
    }
}

pub(super) fn set_locale(
    workspace: &Workspace,
    locale: Locale,
    cx: &mut Context<Workspace>,
) {
    for (_, handle) in &workspace.agent_connectivity_windows {
        let _ = handle.update(cx, |view, _, cx| {
            view.locale = locale;
            cx.notify();
        });
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}
