//! Visible sessions for configured external Agent CLIs.
//!
//! Connectivity tests and Git operations are deliberately normal interactive
//! PTY windows. The user can see and use provider login, approval, and
//! follow-up prompts while Augur Git coordinates only process lifecycle.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, TitleBar, h_flex, v_flex,
};

use crate::agent::{
    AgentCommitChallenge, AgentConnectivityChallenge, AgentLaunchSpec,
    AgentOperation, AgentTestDirectory, ResolvedAgentProfile,
};
use crate::core::git::agent_operation::{AgentCommitProbe, probe_agent_commit};
use crate::core::i18n::{self, Locale};
use crate::terminal::{
    TerminalBackend, TerminalView, normalize_working_directory,
};

use super::Workspace;
use super::agent_commit::{AgentCommitOutcome, classify_probe};
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
    CommitDetected {
        oid: String,
    },
    CommitCompleted {
        outcome: AgentCommitOutcome,
    },
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
    session_id: Option<u64>,
    backend: Option<Arc<TerminalBackend>>,
    terminal: Option<Entity<TerminalView>>,
    state: ConnectivityState,
    response_received: bool,
    stop_requested: bool,
    commit_challenge: Option<AgentCommitChallenge>,
    commit_baseline: Option<AgentCommitProbe>,
    commit_head_observed: Option<String>,
    commit_head_observed_at: Option<Instant>,
    commit_completed: bool,
    window_id: u64,
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
            None,
            false,
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
        challenge: AgentCommitChallenge,
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
            Some(challenge),
            true,
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
        commit_challenge: Option<AgentCommitChallenge>,
        defer_start: bool,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut state = ConnectivityState::Starting;
        let working_directory = normalize_working_directory(&working_directory);
        let has_startup_error = startup_error.is_some();
        let session_id = commit_completion
            .as_ref()
            .map(|completion| completion.session_id);

        if let Some(error) = startup_error {
            state = ConnectivityState::Failed(error);
        }

        let mut session = Self {
            kind,
            locale,
            profile,
            spec,
            prompt_preview,
            test_directory,
            working_directory,
            commit_completion,
            session_id,
            backend: None,
            terminal: None,
            state,
            response_received: false,
            stop_requested: false,
            commit_challenge,
            commit_baseline: None,
            commit_head_observed: None,
            commit_head_observed_at: None,
            commit_completed: false,
            window_id,
            _monitor_task: None,
        };

        if !has_startup_error {
            if defer_start {
                let entity = cx.entity();
                let repo_path = session.working_directory.clone();
                session._monitor_task = Some(cx.spawn(async move |_, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move { probe_agent_commit(&repo_path) })
                        .await;
                    let _ = entity.update(cx, |window, cx| {
                        window.start_commit_after_probe(result, cx);
                    });
                }));
            } else {
                session.start_terminal(challenge, cx);
            }
        }

        session
    }

    fn start_commit_after_probe(
        &mut self,
        result: Result<AgentCommitProbe, String>,
        cx: &mut Context<Self>,
    ) {
        if self.stop_requested || self.commit_completed {
            return;
        }
        match result {
            Ok(baseline) => {
                self.commit_baseline = Some(baseline);
                self.start_terminal(None, cx);
            }
            Err(error) => {
                self.state =
                    ConnectivityState::Failed(first_line(&error).to_string());
                self.finish_commit(AgentCommitOutcome::Failed, cx);
            }
        }
    }

    fn start_terminal(
        &mut self,
        challenge: Option<AgentConnectivityChallenge>,
        cx: &mut Context<Self>,
    ) {
        if self.backend.is_some() || self.stop_requested {
            return;
        }
        let result = TerminalBackend::spawn(
            &self.spec,
            self.test_directory.clone(),
            &self.working_directory,
            self.window_id,
        );
        let value = match result {
            Ok(value) => Arc::new(value),
            Err(error) => {
                let summary = first_line(&error.to_string()).to_string();
                self.state = ConnectivityState::Failed(summary);
                if let Some(directory) = self.test_directory.as_ref() {
                    if directory.cleanup().is_err() {
                        log::debug!(
                            "[agent_terminal] temporary test directory cleanup deferred"
                        );
                    }
                }
                if self.kind == AgentSessionKind::Commit {
                    self.finish_commit(AgentCommitOutcome::Failed, cx);
                }
                return;
            }
        };

        let terminal_entity = cx.new(|cx| TerminalView::new(value.clone(), cx));
        let window_entity = cx.entity();
        let terminal_for_monitor = terminal_entity.clone();
        let backend_for_monitor = value.clone();
        let expected_response = challenge
            .as_ref()
            .map(|challenge| challenge.expected_response.clone());
        let expected_marker = self
            .commit_challenge
            .as_ref()
            .map(|challenge| challenge.expected_marker.clone());
        let profile_id_for_monitor = self.profile.id.clone();
        let kind_for_monitor = self.kind;
        let repo_path = self.working_directory.clone();
        self.backend = Some(value.clone());
        self.terminal = Some(terminal_entity);
        self._monitor_task = Some(cx.spawn(async move |_, cx| {
            let mut last_probe_at = Instant::now() - Duration::from_secs(1);
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let completion = terminal_for_monitor
                    .read_with(cx, |terminal, _| terminal.completion());
                let response_received =
                    expected_response.as_deref().is_some_and(|expected| {
                        backend_for_monitor.contains_text(expected)
                    });
                let marker_seen =
                    expected_marker.as_deref().is_some_and(|marker| {
                        backend_for_monitor.contains_text(marker)
                    });
                let probe = if kind_for_monitor == AgentSessionKind::Commit
                    && (marker_seen
                        || completion.is_some()
                        || last_probe_at.elapsed()
                            >= Duration::from_millis(500))
                {
                    last_probe_at = Instant::now();
                    let path = repo_path.clone();
                    Some(
                        cx.background_executor()
                            .spawn(async move { probe_agent_commit(&path) })
                            .await,
                    )
                } else {
                    None
                };
                let finished = completion.is_some();
                let mut should_break = false;
                let _ = window_entity.update(cx, |window, cx| {
                    if kind_for_monitor == AgentSessionKind::Connectivity {
                        window.handle_connectivity_tick(
                            response_received,
                            completion.clone(),
                            &profile_id_for_monitor,
                            cx,
                        );
                    } else {
                        window.handle_commit_tick(
                            probe,
                            marker_seen,
                            completion.clone(),
                            cx,
                        );
                    }
                    should_break = finished || window.commit_completed;
                    cx.notify();
                });
                if should_break {
                    break;
                }
            }
        }));
        log::info!(
            "[agent_terminal] {} started: profile={}",
            session_kind_label(self.kind),
            self.profile.id
        );
    }

    fn handle_connectivity_tick(
        &mut self,
        response_received: bool,
        completion: Option<Result<Option<i32>, String>>,
        profile_id: &str,
        _cx: &mut Context<Self>,
    ) {
        if response_received && !self.response_received {
            self.response_received = true;
            log::info!(
                "[agent_terminal] connectivity test response received: profile={profile_id}"
            );
        }
        if let Some(result) = completion {
            self.state = match result {
                Ok(code) => {
                    log::info!(
                        "[agent_terminal] connectivity test exited: profile={profile_id}, code={code:?}"
                    );
                    ConnectivityState::Exited {
                        code,
                        response_received: self.response_received,
                    }
                }
                Err(summary) => {
                    log::error!(
                        "[agent_terminal] connectivity test failed: profile={profile_id}"
                    );
                    ConnectivityState::Failed(summary)
                }
            };
        } else if self.response_received {
            self.state = ConnectivityState::ResponseReceived;
        } else if matches!(self.state, ConnectivityState::Starting) {
            self.state = ConnectivityState::WaitingForResponse;
        }
    }

    fn handle_commit_tick(
        &mut self,
        probe: Option<Result<AgentCommitProbe, String>>,
        marker_seen: bool,
        completion: Option<Result<Option<i32>, String>>,
        cx: &mut Context<Self>,
    ) {
        if self.commit_completed {
            return;
        }
        let mut current_probe = None;
        if let Some(result) = probe {
            match result {
                Ok(probe) => {
                    if let Some(baseline) = self.commit_baseline.as_ref()
                        && baseline.head != probe.head
                    {
                        if self.commit_head_observed.is_none() {
                            if let Some(oid) = probe.head.clone() {
                                self.commit_head_observed = Some(oid.clone());
                                self.commit_head_observed_at =
                                    Some(Instant::now());
                                log::info!(
                                    "[agent_terminal] commit HEAD changed: profile={}",
                                    self.profile.id
                                );
                                self.state =
                                    ConnectivityState::CommitDetected {
                                        oid: oid.clone(),
                                    };
                                if let Some(completion) =
                                    self.commit_completion.as_ref()
                                {
                                    let tab_id = completion.tab_id;
                                    let session_id = completion.session_id;
                                    let _ = completion.workspace.update(
                                        cx,
                                        move |workspace, cx| {
                                            workspace.observe_agent_commit(
                                                tab_id, session_id, oid, cx,
                                            );
                                        },
                                    );
                                }
                            }
                        }
                    }
                    current_probe = Some(probe);
                }
                Err(_error) => {
                    log::debug!(
                        "[agent_terminal] commit probe unavailable: profile={}",
                        self.profile.id
                    );
                }
            }
        }

        if marker_seen {
            let outcome = current_probe
                .as_ref()
                .and_then(|probe| self.classify_commit_probe(probe))
                .unwrap_or(AgentCommitOutcome::Failed);
            self.finish_commit(outcome, cx);
            return;
        }

        if let Some(result) = completion {
            let code = result.as_ref().ok().copied().flatten();
            log::info!(
                "[agent_terminal] commit PTY exited: profile={}, code={code:?}",
                self.profile.id
            );
            let outcome = current_probe
                .as_ref()
                .and_then(|probe| self.classify_commit_probe(probe))
                .unwrap_or_else(|| match result {
                    Ok(code) => AgentCommitOutcome::ExitedUnverified { code },
                    Err(_) => {
                        AgentCommitOutcome::ExitedUnverified { code: None }
                    }
                });
            self.finish_commit(outcome, cx);
            return;
        }

        if let (Some(observed_at), Some(backend)) =
            (self.commit_head_observed_at, self.backend.as_ref())
            && observed_at.elapsed() >= Duration::from_secs(30)
            && backend.last_activity().elapsed() >= Duration::from_secs(3)
        {
            if let Some(oid) = self.commit_head_observed.clone() {
                log::warn!(
                    "[agent_terminal] commit marker timeout; using verified HEAD: profile={}",
                    self.profile.id
                );
                self.finish_commit(AgentCommitOutcome::Committed { oid }, cx);
            }
        }
    }

    fn classify_commit_probe(
        &self,
        probe: &AgentCommitProbe,
    ) -> Option<AgentCommitOutcome> {
        self.commit_baseline
            .as_ref()
            .and_then(|baseline| classify_probe(baseline, probe))
    }

    fn finish_commit(
        &mut self,
        outcome: AgentCommitOutcome,
        cx: &mut Context<Self>,
    ) {
        if self.commit_completed {
            return;
        }
        self.commit_completed = true;
        self.stop_requested = true;
        let success = matches!(&outcome, AgentCommitOutcome::Committed { .. });
        log::info!(
            "[agent_terminal] commit operation completed: profile={}, outcome={}",
            self.profile.id,
            commit_outcome_label(&outcome)
        );
        if !(matches!(&self.state, ConnectivityState::Failed(_))
            && matches!(&outcome, AgentCommitOutcome::Failed))
        {
            self.state = ConnectivityState::CommitCompleted {
                outcome: outcome.clone(),
            };
        }
        if let Some(backend) = &self.backend {
            backend.shutdown();
        }
        let Some(completion) = self.commit_completion.take() else {
            cx.notify();
            return;
        };
        let tab_id = completion.tab_id;
        let session_id = completion.session_id;
        let workspace = completion.workspace.clone();
        let _ = workspace.update(cx, move |workspace, cx| {
            workspace.finish_agent_commit(
                tab_id,
                session_id,
                outcome.clone(),
                cx,
            );
        });
        if success {
            let _ = cx
                .spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(300))
                        .await;
                    let _ = workspace.update(cx, move |workspace, cx| {
                        workspace.close_agent_session(session_id, cx);
                    });
                })
                .detach();
        }
        cx.notify();
    }

    pub(super) fn is_running(&self) -> bool {
        matches!(
            self.state,
            ConnectivityState::Starting
                | ConnectivityState::WaitingForResponse
                | ConnectivityState::ResponseReceived
                | ConnectivityState::CommitDetected { .. }
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

    pub(super) fn session_id(&self) -> Option<u64> {
        self.session_id
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.is_running() {
            return;
        }
        if self.kind == AgentSessionKind::Commit {
            self.finish_commit(AgentCommitOutcome::Cancelled, cx);
            log::info!(
                "[agent_terminal] commit termination requested: profile={}",
                self.profile.id,
            );
            cx.notify();
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
                ConnectivityState::CommitDetected { oid } => i18n::text_args(
                    self.locale,
                    "agent-commit-status-detected",
                    &[("oid", oid)],
                ),
                ConnectivityState::CommitCompleted { outcome } => match outcome
                {
                    AgentCommitOutcome::Committed { oid } => i18n::text_args(
                        self.locale,
                        "agent-commit-status-committed",
                        &[("oid", oid)],
                    ),
                    AgentCommitOutcome::NoChanges => i18n::text(
                        self.locale,
                        "agent-commit-status-no-changes",
                    ),
                    AgentCommitOutcome::Conflict => {
                        i18n::text(self.locale, "agent-commit-status-conflict")
                    }
                    AgentCommitOutcome::Failed => i18n::text(
                        self.locale,
                        "agent-commit-status-failed-generic",
                    ),
                    AgentCommitOutcome::Cancelled => {
                        i18n::text(self.locale, "agent-commit-status-cancelled")
                    }
                    AgentCommitOutcome::ExitedUnverified { code } => {
                        let suffix = code
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| {
                                i18n::text(
                                    self.locale,
                                    "agent-test-exit-unknown",
                                )
                            });
                        i18n::text_args(
                            self.locale,
                            "agent-commit-status-unverified",
                            &[("code", &suffix)],
                        )
                    }
                },
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
            ConnectivityState::CommitDetected { .. }
            | ConnectivityState::CommitCompleted { .. } => {
                i18n::text(self.locale, "agent-test-status-exited")
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
        let is_commit = self.kind == AgentSessionKind::Commit;
        let can_close = is_commit && !can_stop;
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
                                    .text_color(if matches!(
                                        self.state,
                                        ConnectivityState::Failed(_)
                                    ) || matches!(
                                        &self.state,
                                        ConnectivityState::CommitCompleted {
                                            outcome: AgentCommitOutcome::Conflict
                                                | AgentCommitOutcome::Failed
                                                | AgentCommitOutcome::Cancelled
                                                | AgentCommitOutcome::NoChanges
                                                | AgentCommitOutcome::ExitedUnverified { .. },
                                        }
                                    ) {
                                        colors.red
                                    } else if self.response_received
                                        || matches!(
                                            &self.state,
                                            ConnectivityState::CommitCompleted {
                                                outcome: AgentCommitOutcome::Committed { .. },
                                            }
                                        ) {
                                        colors.green
                                    } else {
                                        colors.foreground
                                    })
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(status)),
                            )
                            .child(
                                Button::new("agent-connectivity-stop")
                                    .label(if can_close {
                                        i18n::text(
                                            self.locale,
                                            "agent-commit-close",
                                        )
                                    } else if is_commit {
                                        i18n::text(
                                            self.locale,
                                            "agent-commit-stop",
                                        )
                                    } else {
                                        i18n::text(
                                            self.locale,
                                            "agent-test-stop",
                                        )
                                    })
                                    .danger()
                                    .small()
                                    .disabled(!can_close && !can_stop)
                                    .on_click(move |_event, window, cx| {
                                        if can_close {
                                            window.remove_window();
                                        } else {
                                            stop.update(cx, |test, cx| {
                                                test.stop(cx)
                                            });
                                        }
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

fn commit_outcome_label(outcome: &AgentCommitOutcome) -> &'static str {
    match outcome {
        AgentCommitOutcome::Committed { .. } => "committed",
        AgentCommitOutcome::NoChanges => "no-changes",
        AgentCommitOutcome::Conflict => "conflict",
        AgentCommitOutcome::Failed => "failed",
        AgentCommitOutcome::Cancelled => "cancelled",
        AgentCommitOutcome::ExitedUnverified { .. } => "exited-unverified",
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
    commit_challenge: Option<AgentCommitChallenge>,
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
    let commit_challenge_for_window = commit_challenge.clone();
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
                commit_challenge_for_window.unwrap_or_default(),
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
            workspace.agent_sessions.push((key, handle));
            if let Some((tab_id, session_id)) = begin_agent
                && startup_error.is_none()
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
    let challenge = AgentCommitChallenge::new();
    let fixed_prompt = AgentOperation::Commit
        .prompt_with_challenge(None, &challenge)
        .unwrap_or_else(|_| {
            "Commit the current repository changes.".to_string()
        });
    let profile_id = workspace.config.agent.default_profile_id();
    let (profile, prompt, spec, startup_error) = match workspace
        .config
        .agent
        .profile(&profile_id)
    {
        Some(profile) => match AgentOperation::Commit
            .prompt_with_challenge(Some(&hint), &challenge)
        {
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
        Some(challenge),
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

    pub(super) fn observe_agent_commit(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        oid: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| {
                tab.observe_agent_commit(session_id, oid, cx)
            });
        }
    }

    pub(super) fn finish_agent_commit(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        outcome: AgentCommitOutcome,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| {
                tab.finish_agent_commit(session_id, outcome, cx)
            });
        }
    }

    pub(super) fn close_agent_session(
        &mut self,
        session_id: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.agent_sessions.iter().position(|(_, handle)| {
            handle
                .read(cx)
                .is_ok_and(|session| session.session_id() == Some(session_id))
        }) else {
            return;
        };
        let (_, handle) = self.agent_sessions.remove(index);
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}
