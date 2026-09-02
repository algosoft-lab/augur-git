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
    AgentConnectivityChallenge, AgentLaunchSpec, AgentOperation,
    AgentOperationChallenge, AgentTestDirectory, ResolvedAgentProfile,
};
use crate::core::git::agent_operation::{
    AgentCommitProbe, AgentMergeProbe, AgentRebaseProbe,
    has_other_git_operation, has_other_git_operation_except_rebase,
    probe_agent_commit, probe_agent_merge, probe_agent_rebase,
    resolve_agent_merge_target,
};
use crate::core::i18n::{self, Locale};
use crate::terminal::{
    TerminalBackend, TerminalView, normalize_working_directory,
};

use super::Workspace;
use super::agent_commit::{AgentCommitOutcome, classify_probe};
use super::agent_merge::{
    AgentMergeMode, AgentMergeOutcome, classify_merge_probe,
};
use super::agent_rebase::{
    AgentRebaseMode, AgentRebaseOutcome, classify_rebase_probe,
};
use super::tabs::TabId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentSessionKind {
    Connectivity,
    Commit,
    Merge,
    Rebase,
}

#[derive(Clone)]
struct CommitCompletion {
    workspace: WeakEntity<Workspace>,
    tab_id: TabId,
    session_id: u64,
}

#[derive(Clone)]
struct MergeCompletion {
    workspace: WeakEntity<Workspace>,
    tab_id: TabId,
    session_id: u64,
}

#[derive(Clone)]
struct RebaseCompletion {
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
    MergeDetected {
        oid: String,
    },
    MergeCompleted {
        outcome: AgentMergeOutcome,
    },
    RebaseDetected {
        oid: String,
    },
    RebaseCompleted {
        outcome: AgentRebaseOutcome,
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
    merge_completion: Option<MergeCompletion>,
    rebase_completion: Option<RebaseCompletion>,
    session_id: Option<u64>,
    backend: Option<Arc<TerminalBackend>>,
    terminal: Option<Entity<TerminalView>>,
    state: ConnectivityState,
    response_received: bool,
    stop_requested: bool,
    commit_challenge: Option<AgentOperationChallenge>,
    commit_baseline: Option<AgentCommitProbe>,
    commit_head_observed: Option<String>,
    commit_head_observed_at: Option<Instant>,
    commit_completed: bool,
    merge_challenge: Option<AgentOperationChallenge>,
    merge_mode: Option<AgentMergeMode>,
    merge_baseline: Option<AgentMergeProbe>,
    merge_head_observed: Option<String>,
    merge_head_observed_at: Option<Instant>,
    merge_completed: bool,
    rebase_challenge: Option<AgentOperationChallenge>,
    rebase_mode: Option<AgentRebaseMode>,
    rebase_baseline: Option<AgentRebaseProbe>,
    rebase_head_observed: Option<String>,
    rebase_head_observed_at: Option<Instant>,
    rebase_completed: bool,
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
        challenge: AgentOperationChallenge,
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
            None,
            startup_error,
            None,
            Some(challenge),
            true,
            window_id,
            cx,
        )
    }

    fn new_merge(
        locale: Locale,
        profile: ResolvedAgentProfile,
        spec: AgentLaunchSpec,
        prompt: String,
        working_directory: PathBuf,
        completion: MergeCompletion,
        challenge: AgentOperationChallenge,
        mode: AgentMergeMode,
        baseline: AgentMergeProbe,
        startup_error: Option<String>,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut session = Self::new_inner(
            AgentSessionKind::Merge,
            locale,
            profile,
            spec,
            prompt,
            None,
            working_directory,
            None,
            Some(completion),
            startup_error,
            None,
            None,
            true,
            window_id,
            cx,
        );
        session.session_id = session
            .merge_completion
            .as_ref()
            .map(|completion| completion.session_id);
        session.merge_challenge = Some(challenge);
        session.merge_mode = Some(mode);
        session.merge_baseline = Some(baseline);
        if session.backend.is_none()
            && !session.stop_requested
            && !matches!(session.state, ConnectivityState::Failed(_))
        {
            session.start_terminal(None, cx);
        }
        session
    }

    fn new_rebase(
        locale: Locale,
        profile: ResolvedAgentProfile,
        spec: AgentLaunchSpec,
        prompt: String,
        working_directory: PathBuf,
        completion: RebaseCompletion,
        challenge: AgentOperationChallenge,
        mode: AgentRebaseMode,
        baseline: AgentRebaseProbe,
        startup_error: Option<String>,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut session = Self::new_inner(
            AgentSessionKind::Rebase,
            locale,
            profile,
            spec,
            prompt,
            None,
            working_directory,
            None,
            None,
            startup_error,
            None,
            None,
            true,
            window_id,
            cx,
        );
        session.session_id = Some(completion.session_id);
        session.rebase_completion = Some(completion);
        session.rebase_challenge = Some(challenge);
        session.rebase_mode = Some(mode);
        session.rebase_baseline = Some(baseline);
        if session.backend.is_none()
            && !session.stop_requested
            && !matches!(session.state, ConnectivityState::Failed(_))
        {
            session.start_terminal(None, cx);
        }
        session
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
        merge_completion: Option<MergeCompletion>,
        startup_error: Option<String>,
        challenge: Option<AgentConnectivityChallenge>,
        commit_challenge: Option<AgentOperationChallenge>,
        defer_start: bool,
        window_id: u64,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut state = ConnectivityState::Starting;
        let working_directory = normalize_working_directory(&working_directory);
        let has_startup_error = startup_error.is_some();
        let session_id = commit_completion
            .as_ref()
            .map(|completion| completion.session_id)
            .or_else(|| {
                merge_completion
                    .as_ref()
                    .map(|completion| completion.session_id)
            });

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
            merge_completion,
            rebase_completion: None,
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
            merge_challenge: None,
            merge_mode: None,
            merge_baseline: None,
            merge_head_observed: None,
            merge_head_observed_at: None,
            merge_completed: false,
            rebase_challenge: None,
            rebase_mode: None,
            rebase_baseline: None,
            rebase_head_observed: None,
            rebase_head_observed_at: None,
            rebase_completed: false,
            window_id,
            _monitor_task: None,
        };

        if !has_startup_error {
            if defer_start && kind == AgentSessionKind::Commit {
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
            } else if !defer_start {
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
                } else if self.kind == AgentSessionKind::Merge {
                    self.finish_merge(AgentMergeOutcome::Failed, cx);
                } else if self.kind == AgentSessionKind::Rebase {
                    self.finish_rebase(AgentRebaseOutcome::Failed, cx);
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
            .map(|challenge| challenge.expected_marker.clone())
            .or_else(|| {
                self.merge_challenge
                    .as_ref()
                    .map(|challenge| challenge.expected_marker.clone())
            })
            .or_else(|| {
                self.rebase_challenge
                    .as_ref()
                    .map(|challenge| challenge.expected_marker.clone())
            });
        let profile_id_for_monitor = self.profile.id.clone();
        let kind_for_monitor = self.kind;
        let repo_path = self.working_directory.clone();
        let merge_target_for_monitor = self
            .merge_mode
            .as_ref()
            .map(|mode| mode.target_oid().to_string());
        let rebase_target_for_monitor = self
            .rebase_mode
            .as_ref()
            .and_then(|mode| mode.upstream_oid().map(ToOwned::to_owned));
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
                let should_probe = matches!(
                    kind_for_monitor,
                    AgentSessionKind::Commit
                        | AgentSessionKind::Merge
                        | AgentSessionKind::Rebase
                ) && (marker_seen
                    || completion.is_some()
                    || last_probe_at.elapsed() >= Duration::from_millis(500));
                let commit_probe = if kind_for_monitor
                    == AgentSessionKind::Commit
                    && should_probe
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
                let merge_probe = if kind_for_monitor == AgentSessionKind::Merge
                    && should_probe
                {
                    last_probe_at = Instant::now();
                    let path = repo_path.clone();
                    let target =
                        merge_target_for_monitor.clone().unwrap_or_default();
                    Some(
                            cx.background_executor()
                                .spawn(async move {
                                    probe_agent_merge(&path, &target)
                                })
                                .await,
                        )
                } else {
                    None
                };
                let rebase_probe = if kind_for_monitor
                    == AgentSessionKind::Rebase
                    && should_probe
                {
                    last_probe_at = Instant::now();
                    let path = repo_path.clone();
                    let target = rebase_target_for_monitor.clone();
                    Some(
                        cx.background_executor()
                            .spawn(async move {
                                probe_agent_rebase(&path, target.as_deref())
                            })
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
                    } else if kind_for_monitor == AgentSessionKind::Commit {
                        window.handle_commit_tick(
                            commit_probe,
                            marker_seen,
                            completion.clone(),
                            cx,
                        );
                    } else {
                        if kind_for_monitor == AgentSessionKind::Merge {
                            window.handle_merge_tick(
                                merge_probe,
                                marker_seen,
                                completion.clone(),
                                cx,
                            );
                        } else {
                            window.handle_rebase_tick(
                                rebase_probe,
                                marker_seen,
                                completion.clone(),
                                cx,
                            );
                        }
                    }
                    should_break = finished
                        || window.commit_completed
                        || window.merge_completed;
                    should_break = should_break || window.rebase_completed;
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

        if matches!(self.state, ConnectivityState::Starting) {
            self.state = ConnectivityState::WaitingForResponse;
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

    fn handle_merge_tick(
        &mut self,
        probe: Option<Result<AgentMergeProbe, String>>,
        marker_seen: bool,
        completion: Option<Result<Option<i32>, String>>,
        cx: &mut Context<Self>,
    ) {
        if self.merge_completed {
            return;
        }
        let mut current_probe = None;
        if let Some(result) = probe {
            match result {
                Ok(probe) => {
                    if let Some(baseline) = self.merge_baseline.as_ref()
                        && baseline.head != probe.head
                        && self.merge_head_observed.is_none()
                    {
                        if let Some(oid) = probe.head.clone() {
                            self.merge_head_observed = Some(oid.clone());
                            self.merge_head_observed_at = Some(Instant::now());
                            self.state = ConnectivityState::MergeDetected {
                                oid: oid.clone(),
                            };
                            log::info!(
                                "[agent_terminal] merge HEAD changed: profile={}",
                                self.profile.id
                            );
                            if let Some(completion) =
                                self.merge_completion.as_ref()
                            {
                                let tab_id = completion.tab_id;
                                let session_id = completion.session_id;
                                let _ = completion.workspace.update(
                                    cx,
                                    move |workspace, cx| {
                                        workspace.observe_agent_merge(
                                            tab_id, session_id, oid, cx,
                                        );
                                    },
                                );
                            }
                        }
                    }
                    current_probe = Some(probe);
                }
                Err(_) => {
                    log::debug!(
                        "[agent_terminal] merge probe unavailable: profile={}",
                        self.profile.id
                    );
                }
            }
        }

        if marker_seen {
            let outcome = current_probe
                .as_ref()
                .and_then(|probe| self.classify_merge_probe(probe))
                .unwrap_or(AgentMergeOutcome::Failed);
            self.finish_merge(outcome, cx);
            return;
        }

        if let Some(result) = completion {
            let code = result.as_ref().ok().copied().flatten();
            log::info!(
                "[agent_terminal] merge PTY exited: profile={}, code={code:?}",
                self.profile.id
            );
            let outcome = current_probe
                .as_ref()
                .and_then(|probe| self.classify_merge_probe(probe))
                .unwrap_or_else(|| match result {
                    Ok(code) => AgentMergeOutcome::ExitedUnverified { code },
                    Err(_) => {
                        AgentMergeOutcome::ExitedUnverified { code: None }
                    }
                });
            self.finish_merge(outcome, cx);
            return;
        }

        if matches!(self.state, ConnectivityState::Starting) {
            self.state = ConnectivityState::WaitingForResponse;
        }

        if let (Some(observed_at), Some(backend)) =
            (self.merge_head_observed_at, self.backend.as_ref())
            && observed_at.elapsed() >= Duration::from_secs(30)
            && backend.last_activity().elapsed() >= Duration::from_secs(3)
        {
            if let Some(oid) = self.merge_head_observed.clone()
                && current_probe.as_ref().is_some_and(|probe| {
                    probe.target_is_ancestor_of_head
                        && probe.merge_head.is_none()
                        && !probe.has_conflicts
                        && !probe.has_changes
                })
            {
                log::warn!(
                    "[agent_terminal] merge marker timeout; using verified HEAD: profile={}",
                    self.profile.id
                );
                self.finish_merge(AgentMergeOutcome::Merged { oid }, cx);
            }
        }
    }

    fn classify_merge_probe(
        &self,
        probe: &AgentMergeProbe,
    ) -> Option<AgentMergeOutcome> {
        self.merge_mode.as_ref().and_then(|mode| {
            classify_merge_probe(
                mode,
                self.merge_baseline
                    .as_ref()
                    .and_then(|baseline| baseline.head.as_deref()),
                probe,
            )
        })
    }

    fn handle_rebase_tick(
        &mut self,
        probe: Option<Result<AgentRebaseProbe, String>>,
        marker_seen: bool,
        completion: Option<Result<Option<i32>, String>>,
        cx: &mut Context<Self>,
    ) {
        if self.rebase_completed {
            return;
        }
        let mut current_probe = None;
        if let Some(result) = probe {
            match result {
                Ok(probe) => {
                    if let Some(baseline) = self.rebase_baseline.as_ref()
                        && baseline.head != probe.head
                        && self.rebase_head_observed.is_none()
                    {
                        if let Some(oid) = probe.head.clone() {
                            self.rebase_head_observed = Some(oid.clone());
                            self.rebase_head_observed_at = Some(Instant::now());
                            self.state = ConnectivityState::RebaseDetected {
                                oid: oid.clone(),
                            };
                            log::info!(
                                "[agent_terminal] rebase HEAD changed: profile={}",
                                self.profile.id
                            );
                            if let Some(completion) =
                                self.rebase_completion.as_ref()
                            {
                                let tab_id = completion.tab_id;
                                let session_id = completion.session_id;
                                let _ = completion.workspace.update(
                                    cx,
                                    move |workspace, cx| {
                                        workspace.observe_agent_rebase(
                                            tab_id, session_id, oid, cx,
                                        );
                                    },
                                );
                            }
                        }
                    }
                    current_probe = Some(probe);
                }
                Err(_) => {
                    log::debug!(
                        "[agent_terminal] rebase probe unavailable: profile={}",
                        self.profile.id
                    );
                }
            }
        }

        if marker_seen {
            let outcome = current_probe
                .as_ref()
                .and_then(|probe| self.classify_rebase_probe(probe))
                .unwrap_or(AgentRebaseOutcome::Failed);
            self.finish_rebase(outcome, cx);
            return;
        }

        if let Some(result) = completion {
            let code = result.as_ref().ok().copied().flatten();
            log::info!(
                "[agent_terminal] rebase PTY exited: profile={}, code={code:?}",
                self.profile.id
            );
            let outcome = current_probe
                .as_ref()
                .and_then(|probe| self.classify_rebase_probe(probe))
                .unwrap_or_else(|| match result {
                    Ok(code) => AgentRebaseOutcome::ExitedUnverified { code },
                    Err(_) => {
                        AgentRebaseOutcome::ExitedUnverified { code: None }
                    }
                });
            self.finish_rebase(outcome, cx);
            return;
        }

        if matches!(self.state, ConnectivityState::Starting) {
            self.state = ConnectivityState::WaitingForResponse;
        }

        if let (Some(observed_at), Some(backend)) =
            (self.rebase_head_observed_at, self.backend.as_ref())
            && observed_at.elapsed() >= Duration::from_secs(30)
            && backend.last_activity().elapsed() >= Duration::from_secs(3)
        {
            if let Some(oid) = self.rebase_head_observed.clone()
                && current_probe.as_ref().is_some_and(|probe| {
                    !probe.rebase_in_progress
                        && !probe.has_conflicts
                        && !probe.has_changes
                        && (self
                            .rebase_mode
                            .as_ref()
                            .and_then(|mode| mode.upstream_oid())
                            .is_none()
                            || probe.target_is_ancestor_of_head)
                })
            {
                log::warn!(
                    "[agent_terminal] rebase marker timeout; using verified HEAD: profile={}",
                    self.profile.id
                );
                self.finish_rebase(AgentRebaseOutcome::Rebased { oid }, cx);
            }
        }
    }

    fn classify_rebase_probe(
        &self,
        probe: &AgentRebaseProbe,
    ) -> Option<AgentRebaseOutcome> {
        self.rebase_mode.as_ref().and_then(|mode| {
            classify_rebase_probe(
                mode,
                self.rebase_baseline
                    .as_ref()
                    .and_then(|baseline| baseline.head.as_deref()),
                probe,
            )
        })
    }

    fn finish_merge(
        &mut self,
        outcome: AgentMergeOutcome,
        cx: &mut Context<Self>,
    ) {
        if self.merge_completed {
            return;
        }
        self.merge_completed = true;
        self.stop_requested = true;
        let success = matches!(
            &outcome,
            AgentMergeOutcome::Merged { .. }
                | AgentMergeOutcome::AlreadyUpToDate
        );
        log::info!(
            "[agent_terminal] merge operation completed: profile={}, outcome={}",
            self.profile.id,
            merge_outcome_label(&outcome)
        );
        if !matches!(self.state, ConnectivityState::Failed(_)) {
            self.state = ConnectivityState::MergeCompleted {
                outcome: outcome.clone(),
            };
        }
        if let Some(backend) = &self.backend {
            backend.shutdown();
        }
        let Some(completion) = self.merge_completion.take() else {
            cx.notify();
            return;
        };
        let tab_id = completion.tab_id;
        let session_id = completion.session_id;
        let workspace = completion.workspace.clone();
        let _ = workspace.update(cx, move |workspace, cx| {
            // The process can finish before `open_window` returns its handle
            // to Workspace. Registering first keeps the completion callback
            // effective in that fast-exit case while remaining idempotent
            // when the normal post-open registration already happened.
            workspace.begin_agent_merge(tab_id, session_id, cx);
            workspace.finish_agent_merge(
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

    fn finish_rebase(
        &mut self,
        outcome: AgentRebaseOutcome,
        cx: &mut Context<Self>,
    ) {
        if self.rebase_completed {
            return;
        }
        self.rebase_completed = true;
        self.stop_requested = true;
        let success = matches!(
            &outcome,
            AgentRebaseOutcome::Rebased { .. }
                | AgentRebaseOutcome::AlreadyUpToDate
        );
        log::info!(
            "[agent_terminal] rebase operation completed: profile={}, outcome={}",
            self.profile.id,
            rebase_outcome_label(&outcome)
        );
        if !matches!(self.state, ConnectivityState::Failed(_)) {
            self.state = ConnectivityState::RebaseCompleted {
                outcome: outcome.clone(),
            };
        }
        if let Some(backend) = &self.backend {
            backend.shutdown();
        }
        let Some(completion) = self.rebase_completion.take() else {
            cx.notify();
            return;
        };
        let tab_id = completion.tab_id;
        let session_id = completion.session_id;
        let workspace = completion.workspace.clone();
        let _ = workspace.update(cx, move |workspace, cx| {
            workspace.begin_agent_rebase(tab_id, session_id, cx);
            workspace.finish_agent_rebase(
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
            // See the merge completion path: a short-lived CLI may finish
            // before the session handle is registered by `open_window`.
            workspace.begin_agent_commit(tab_id, session_id, cx);
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
                | ConnectivityState::MergeDetected { .. }
                | ConnectivityState::RebaseDetected { .. }
        ) && !self.stop_requested
    }

    pub(super) fn label(&self) -> String {
        match self.kind {
            AgentSessionKind::Connectivity => {
                format!("{} connectivity test", self.profile.name)
            }
            AgentSessionKind::Commit => format!("{} commit", self.profile.name),
            AgentSessionKind::Merge => format!("{} merge", self.profile.name),
            AgentSessionKind::Rebase => {
                format!("{} rebase", self.profile.name)
            }
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
        if self.kind == AgentSessionKind::Merge {
            self.finish_merge(AgentMergeOutcome::Cancelled, cx);
            log::info!(
                "[agent_terminal] merge termination requested: profile={}",
                self.profile.id,
            );
            cx.notify();
            return;
        }
        if self.kind == AgentSessionKind::Rebase {
            self.finish_rebase(AgentRebaseOutcome::Cancelled, cx);
            log::info!(
                "[agent_terminal] rebase termination requested: profile={}",
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
                ConnectivityState::MergeDetected { .. }
                | ConnectivityState::MergeCompleted { .. } => {
                    i18n::text(self.locale, "agent-commit-status-running")
                }
                ConnectivityState::RebaseDetected { .. }
                | ConnectivityState::RebaseCompleted { .. } => {
                    i18n::text(self.locale, "agent-commit-status-running")
                }
            };
        }
        if self.kind == AgentSessionKind::Merge {
            return match &self.state {
                ConnectivityState::Starting => {
                    i18n::text(self.locale, "agent-merge-status-starting")
                }
                ConnectivityState::WaitingForResponse
                | ConnectivityState::ResponseReceived => {
                    i18n::text(self.locale, "agent-merge-status-running")
                }
                ConnectivityState::MergeDetected { oid } => i18n::text_args(
                    self.locale,
                    "agent-merge-status-detected",
                    &[("oid", oid)],
                ),
                ConnectivityState::MergeCompleted { outcome } => {
                    match outcome {
                        AgentMergeOutcome::Merged { oid } => i18n::text_args(
                            self.locale,
                            "agent-merge-status-merged",
                            &[("oid", oid)],
                        ),
                        AgentMergeOutcome::AlreadyUpToDate => i18n::text(
                            self.locale,
                            "agent-merge-status-up-to-date",
                        ),
                        AgentMergeOutcome::Conflict => i18n::text(
                            self.locale,
                            "agent-merge-status-conflict",
                        ),
                        AgentMergeOutcome::Failed => i18n::text(
                            self.locale,
                            "agent-merge-status-failed-generic",
                        ),
                        AgentMergeOutcome::Cancelled => i18n::text(
                            self.locale,
                            "agent-merge-status-cancelled",
                        ),
                        AgentMergeOutcome::ExitedUnverified { code } => {
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
                                "agent-merge-status-unverified",
                                &[("code", &suffix)],
                            )
                        }
                    }
                }
                ConnectivityState::Exited { code, .. } => {
                    let suffix = code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| {
                            i18n::text(self.locale, "agent-test-exit-unknown")
                        });
                    i18n::text_args(
                        self.locale,
                        "agent-merge-status-exited",
                        &[("code", &suffix)],
                    )
                }
                ConnectivityState::Failed(summary) => i18n::text_args(
                    self.locale,
                    "agent-merge-status-failed",
                    &[("error", summary)],
                ),
                ConnectivityState::CommitDetected { .. }
                | ConnectivityState::CommitCompleted { .. } => {
                    i18n::text(self.locale, "agent-merge-status-running")
                }
                ConnectivityState::RebaseDetected { .. }
                | ConnectivityState::RebaseCompleted { .. } => {
                    i18n::text(self.locale, "agent-merge-status-running")
                }
            };
        }
        if self.kind == AgentSessionKind::Rebase {
            return match &self.state {
                ConnectivityState::Starting => {
                    i18n::text(self.locale, "agent-rebase-status-starting")
                }
                ConnectivityState::WaitingForResponse
                | ConnectivityState::ResponseReceived => {
                    i18n::text(self.locale, "agent-rebase-status-running")
                }
                ConnectivityState::RebaseDetected { oid } => i18n::text_args(
                    self.locale,
                    "agent-rebase-status-detected",
                    &[("oid", oid)],
                ),
                ConnectivityState::RebaseCompleted { outcome } => match outcome
                {
                    AgentRebaseOutcome::Rebased { oid } => i18n::text_args(
                        self.locale,
                        "agent-rebase-status-rebased",
                        &[("oid", oid)],
                    ),
                    AgentRebaseOutcome::AlreadyUpToDate => i18n::text(
                        self.locale,
                        "agent-rebase-status-up-to-date",
                    ),
                    AgentRebaseOutcome::Conflict => {
                        i18n::text(self.locale, "agent-rebase-status-conflict")
                    }
                    AgentRebaseOutcome::Failed => i18n::text(
                        self.locale,
                        "agent-rebase-status-failed-generic",
                    ),
                    AgentRebaseOutcome::Cancelled => {
                        i18n::text(self.locale, "agent-rebase-status-cancelled")
                    }
                    AgentRebaseOutcome::ExitedUnverified { code } => {
                        i18n::text_args(
                            self.locale,
                            "agent-rebase-status-unverified",
                            &[("code", &format_exit_code(*code))],
                        )
                    }
                },
                ConnectivityState::Exited { code, .. } => i18n::text_args(
                    self.locale,
                    "agent-rebase-status-exited",
                    &[("code", &format_exit_code(*code))],
                ),
                ConnectivityState::Failed(summary) => i18n::text_args(
                    self.locale,
                    "agent-rebase-status-failed",
                    &[("error", summary)],
                ),
                ConnectivityState::CommitDetected { .. }
                | ConnectivityState::CommitCompleted { .. }
                | ConnectivityState::MergeDetected { .. }
                | ConnectivityState::MergeCompleted { .. } => {
                    i18n::text(self.locale, "agent-rebase-status-running")
                }
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
            ConnectivityState::MergeDetected { .. }
            | ConnectivityState::MergeCompleted { .. } => {
                i18n::text(self.locale, "agent-test-status-exited")
            }
            ConnectivityState::RebaseDetected { .. }
            | ConnectivityState::RebaseCompleted { .. } => {
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
        let is_merge = self.kind == AgentSessionKind::Merge;
        let is_rebase = self.kind == AgentSessionKind::Rebase;
        let is_git_operation = is_commit || is_merge || is_rebase;
        let can_close = is_git_operation && !can_stop;
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
            ConnectivityState::MergeCompleted {
                outcome:
                    AgentMergeOutcome::Conflict
                    | AgentMergeOutcome::Failed
                    | AgentMergeOutcome::Cancelled
                    | AgentMergeOutcome::ExitedUnverified { .. },
            } => Some(i18n::text(self.locale, "agent-merge-failed")),
            ConnectivityState::RebaseCompleted {
                outcome:
                    AgentRebaseOutcome::Conflict
                    | AgentRebaseOutcome::Failed
                    | AgentRebaseOutcome::Cancelled
                    | AgentRebaseOutcome::ExitedUnverified { .. },
            } => Some(i18n::text(self.locale, "agent-rebase-failed")),
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
                                    if is_git_operation {
                                        i18n::text(
                                            self.locale,
                                            if is_merge {
                                                "agent-merge-window-title"
                                            } else if is_rebase {
                                                "agent-rebase-window-title"
                                            } else {
                                                "agent-commit-window-title"
                                            },
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
                                    ) || matches!(
                                        &self.state,
                                        ConnectivityState::MergeCompleted {
                                            outcome: AgentMergeOutcome::Conflict
                                                | AgentMergeOutcome::Failed
                                                | AgentMergeOutcome::Cancelled
                                                | AgentMergeOutcome::ExitedUnverified { .. },
                                        }
                                    ) || matches!(
                                        &self.state,
                                        ConnectivityState::RebaseCompleted {
                                            outcome: AgentRebaseOutcome::Conflict
                                                | AgentRebaseOutcome::Failed
                                                | AgentRebaseOutcome::Cancelled
                                                | AgentRebaseOutcome::ExitedUnverified { .. },
                                        }
                                    ) {
                                        colors.red
                                    } else if self.response_received
                                        || matches!(
                                            &self.state,
                                            ConnectivityState::CommitCompleted {
                                                outcome: AgentCommitOutcome::Committed { .. },
                                            }
                                        ) || matches!(
                                            &self.state,
                                        ConnectivityState::MergeCompleted {
                                            outcome: AgentMergeOutcome::Merged { .. }
                                                | AgentMergeOutcome::AlreadyUpToDate,
                                        }
                                    ) || matches!(
                                        &self.state,
                                        ConnectivityState::RebaseCompleted {
                                            outcome: AgentRebaseOutcome::Rebased { .. }
                                                | AgentRebaseOutcome::AlreadyUpToDate,
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
                                    } else if is_merge {
                                        i18n::text(
                                            self.locale,
                                            "agent-merge-stop",
                                        )
                                    } else if is_rebase {
                                        i18n::text(
                                            self.locale,
                                            "agent-rebase-stop",
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
                                if is_merge {
                                    "agent-merge-note"
                                } else if is_rebase {
                                    "agent-rebase-note"
                                } else if is_commit {
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
        AgentSessionKind::Merge => "merge session",
        AgentSessionKind::Rebase => "rebase session",
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

fn merge_outcome_label(outcome: &AgentMergeOutcome) -> &'static str {
    match outcome {
        AgentMergeOutcome::Merged { .. } => "merged",
        AgentMergeOutcome::AlreadyUpToDate => "already-up-to-date",
        AgentMergeOutcome::Conflict => "conflict",
        AgentMergeOutcome::Failed => "failed",
        AgentMergeOutcome::Cancelled => "cancelled",
        AgentMergeOutcome::ExitedUnverified { .. } => "exited-unverified",
    }
}

fn rebase_outcome_label(outcome: &AgentRebaseOutcome) -> &'static str {
    match outcome {
        AgentRebaseOutcome::Rebased { .. } => "rebased",
        AgentRebaseOutcome::AlreadyUpToDate => "already-up-to-date",
        AgentRebaseOutcome::Conflict => "conflict",
        AgentRebaseOutcome::Failed => "failed",
        AgentRebaseOutcome::Cancelled => "cancelled",
        AgentRebaseOutcome::ExitedUnverified { .. } => "exited-unverified",
    }
}

fn connectivity_key(profile_id: &str) -> String {
    format!("connectivity:{profile_id}")
}

fn commit_key(repo_path: &str) -> String {
    format!("git-agent:{repo_path}")
}

fn merge_key(repo_path: &str) -> String {
    commit_key(repo_path)
}

fn rebase_key(repo_path: &str) -> String {
    commit_key(repo_path)
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
    merge_completion: Option<MergeCompletion>,
    commit_challenge: Option<AgentOperationChallenge>,
    merge_challenge: Option<AgentOperationChallenge>,
    merge_mode: Option<AgentMergeMode>,
    merge_baseline: Option<AgentMergeProbe>,
    startup_error: Option<String>,
    begin_agent: Option<(TabId, u64)>,
    begin_merge_agent: Option<(TabId, u64)>,
    rebase_completion: Option<RebaseCompletion>,
    rebase_challenge: Option<AgentOperationChallenge>,
    rebase_mode: Option<AgentRebaseMode>,
    rebase_baseline: Option<AgentRebaseProbe>,
    begin_rebase_agent: Option<(TabId, u64)>,
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
    let merge_completion_for_window = merge_completion.clone();
    let commit_challenge_for_window = commit_challenge.clone();
    let merge_challenge_for_window = merge_challenge.clone();
    let merge_mode_for_window = merge_mode.clone();
    let merge_baseline_for_window = merge_baseline.clone();
    let rebase_completion_for_window = rebase_completion.clone();
    let rebase_challenge_for_window = rebase_challenge.clone();
    let rebase_mode_for_window = rebase_mode.clone();
    let rebase_baseline_for_window = rebase_baseline.clone();
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
            AgentSessionKind::Merge => AgentSessionWindow::new_merge(
                locale,
                profile_for_window,
                spec_for_window,
                prompt_for_window,
                working_directory_for_window,
                merge_completion_for_window
                    .expect("merge completion is required"),
                merge_challenge_for_window
                    .expect("merge challenge is required"),
                merge_mode_for_window.expect("merge mode is required"),
                merge_baseline_for_window.expect("merge baseline is required"),
                startup_error_for_window,
                window.window_handle().window_id().as_u64(),
                cx,
            ),
            AgentSessionKind::Rebase => AgentSessionWindow::new_rebase(
                locale,
                profile_for_window,
                spec_for_window,
                prompt_for_window,
                working_directory_for_window,
                rebase_completion_for_window
                    .expect("rebase completion is required"),
                rebase_challenge_for_window
                    .expect("rebase challenge is required"),
                rebase_mode_for_window.expect("rebase mode is required"),
                rebase_baseline_for_window
                    .expect("rebase baseline is required"),
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
                let should_begin = workspace
                    .agent_sessions
                    .iter()
                    .find(|(_, candidate)| {
                        candidate.read(cx).is_ok_and(|session| {
                            session.session_id() == Some(session_id)
                        })
                    })
                    .and_then(|(_, candidate)| {
                        candidate
                            .read(cx)
                            .ok()
                            .map(|session| session.is_running())
                    })
                    .unwrap_or(false);
                if should_begin {
                    workspace.begin_agent_commit(tab_id, session_id, cx);
                }
            }
            if let Some((tab_id, session_id)) = begin_merge_agent
                && startup_error.is_none()
            {
                let should_begin = workspace
                    .agent_sessions
                    .iter()
                    .find(|(_, candidate)| {
                        candidate.read(cx).is_ok_and(|session| {
                            session.session_id() == Some(session_id)
                        })
                    })
                    .and_then(|(_, candidate)| {
                        candidate
                            .read(cx)
                            .ok()
                            .map(|session| session.is_running())
                    })
                    .unwrap_or(false);
                if should_begin {
                    workspace.begin_agent_merge(tab_id, session_id, cx);
                }
            }
            if let Some((tab_id, session_id)) = begin_rebase_agent
                && startup_error.is_none()
            {
                let should_begin = workspace
                    .agent_sessions
                    .iter()
                    .find(|(_, candidate)| {
                        candidate.read(cx).is_ok_and(|session| {
                            session.session_id() == Some(session_id)
                        })
                    })
                    .and_then(|(_, candidate)| {
                        candidate
                            .read(cx)
                            .ok()
                            .map(|session| session.is_running())
                    })
                    .unwrap_or(false);
                if should_begin {
                    workspace.begin_agent_rebase(tab_id, session_id, cx);
                }
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
/// With opt-in presets the agent list can legitimately be empty. Instead of
/// opening a session window that immediately fails, send the user straight
/// to the Agents settings section. Returns false when nothing is configured.
fn ensure_agent_enabled(
    workspace: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> bool {
    let profile_id = workspace.config.agent.default_profile_id();
    if workspace.config.agent.profile(&profile_id).is_some() {
        return true;
    }
    log::info!(
        "[agent_terminal] no agent profile enabled; opening Agents settings"
    );
    workspace.show_settings = true;
    workspace.settings_panel.update(cx, |panel, cx| {
        panel.reveal_agents(cx);
    });
    cx.notify();
    false
}

pub(super) fn open_commit(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    hint: String,
    cx: &mut Context<Workspace>,
) {
    if !ensure_agent_enabled(workspace, cx) {
        return;
    }
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
    let challenge = AgentOperationChallenge::new();
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
        None,
        Some(challenge),
        None,
        None,
        None,
        startup_error,
        Some((tab_id, session_id)),
        None,
        None,
        None,
        None,
        None,
        None,
        cx,
    );
}

struct PreparedMerge {
    mode: AgentMergeMode,
    baseline: AgentMergeProbe,
}

struct PreparedRebase {
    mode: AgentRebaseMode,
    baseline: AgentRebaseProbe,
}

fn prepare_rebase(
    repo_path: &std::path::Path,
    source: &str,
) -> Result<PreparedRebase, String> {
    let upstream_oid = resolve_agent_merge_target(repo_path, source)?;
    let probe = probe_agent_rebase(repo_path, Some(&upstream_oid))?;
    if has_other_git_operation(repo_path)?
        || has_other_git_operation_except_rebase(repo_path)?
    {
        return Err(
            "another Git operation is already in progress; finish or abort it first"
                .to_string(),
        );
    }
    if probe.rebase_in_progress {
        return Err(
            "a rebase is already in progress; finish or abort it first"
                .to_string(),
        );
    }
    if probe.has_conflicts {
        return Err("the repository has unresolved conflicts".to_string());
    }
    if probe.has_changes {
        return Err(
            "the working tree must be clean before Rebase by AI".to_string()
        );
    }
    Ok(PreparedRebase {
        mode: AgentRebaseMode::Start { upstream_oid },
        baseline: probe,
    })
}

fn prepare_rebase_resolution(
    repo_path: &std::path::Path,
    rebase_head: Option<&str>,
    upstream_oid: Option<&str>,
    baseline_head: Option<&str>,
) -> Result<PreparedRebase, String> {
    let probe = probe_agent_rebase(repo_path, upstream_oid)?;
    if has_other_git_operation_except_rebase(repo_path)? {
        return Err(
            "another Git operation is already in progress; finish or abort it first"
                .to_string(),
        );
    }
    if !probe.rebase_in_progress {
        return Err("the rebase is no longer in progress".to_string());
    }
    if let Some(expected) = rebase_head
        && probe.rebase_head.as_deref() != Some(expected)
    {
        return Err(
            "the rebase state changed while the dialog was open".to_string()
        );
    }
    if probe.head.as_deref() != baseline_head {
        return Err(
            "the repository HEAD changed while the rebase dialog was open"
                .to_string(),
        );
    }
    Ok(PreparedRebase {
        mode: AgentRebaseMode::Resolve {
            upstream_oid: upstream_oid.map(str::to_owned),
            rebase_head_oid: probe.rebase_head.clone(),
        },
        baseline: probe,
    })
}

fn prepare_merge(
    repo_path: &std::path::Path,
    source: &str,
) -> Result<PreparedMerge, String> {
    let target_oid = resolve_agent_merge_target(repo_path, source)?;
    let probe = probe_agent_merge(repo_path, &target_oid)?;
    let has_other_operation = has_other_git_operation(repo_path)?;
    log::debug!(
        "[agent_terminal] merge preflight probe: head_present={}, merge_head_present={}, changes={}, conflicts={}, other_operation={}",
        probe.head.is_some(),
        probe.merge_head.is_some(),
        probe.has_changes,
        probe.has_conflicts,
        has_other_operation,
    );
    if has_other_operation {
        return Err(
            "another Git operation is already in progress; finish or abort it first"
                .to_string(),
        );
    }
    if let Some(merge_head) = probe.merge_head.clone() {
        if merge_head != target_oid {
            return Err(
                "a different merge is already in progress; finish or abort it first"
                    .to_string(),
            );
        }
        return Ok(PreparedMerge {
            mode: AgentMergeMode::Resolve {
                merge_head_oid: merge_head,
            },
            baseline: probe,
        });
    }
    if probe.has_conflicts {
        return Err(
            "the repository has conflicts without a matching MERGE_HEAD"
                .to_string(),
        );
    }
    if probe.has_changes {
        return Err(
            "the working tree must be clean before Merge by AI".to_string()
        );
    }
    Ok(PreparedMerge {
        mode: AgentMergeMode::Start { target_oid },
        baseline: probe,
    })
}

fn prepare_merge_resolution(
    repo_path: &std::path::Path,
    merge_head: &str,
    baseline_head: Option<&str>,
) -> Result<PreparedMerge, String> {
    let probe = probe_agent_merge(repo_path, merge_head)?;
    let has_other_operation = has_other_git_operation(repo_path)?;
    log::debug!(
        "[agent_terminal] merge resolution preflight probe: head_present={}, merge_head_present={}, changes={}, conflicts={}, other_operation={}",
        probe.head.is_some(),
        probe.merge_head.is_some(),
        probe.has_changes,
        probe.has_conflicts,
        has_other_operation,
    );
    if has_other_operation {
        return Err(
            "another Git operation is already in progress; finish or abort it first"
                .to_string(),
        );
    }
    if probe.merge_head.as_deref() != Some(merge_head) {
        return Err("the merge is no longer in progress".to_string());
    }
    if probe.head.as_deref() != baseline_head {
        return Err(
            "the repository HEAD changed while the merge dialog was open"
                .to_string(),
        );
    }
    Ok(PreparedMerge {
        mode: AgentMergeMode::Resolve {
            merge_head_oid: merge_head.to_string(),
        },
        baseline: probe,
    })
}

/// Open or activate a visible Agent session that performs a complete merge.
pub(super) fn open_merge(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    source: String,
    cx: &mut Context<Workspace>,
) {
    log::info!(
        "[agent_terminal] Merge by AI requested: tab={tab_id}, source_present=true"
    );
    if !ensure_agent_enabled(workspace, cx) {
        return;
    }
    open_merge_preflight(workspace, tab_id, repo_path, Some(source), cx);
}

/// Open or activate a visible Agent session that resolves an existing merge.
pub(super) fn open_merge_resolution(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    merge_head: String,
    baseline_head: Option<String>,
    cx: &mut Context<Workspace>,
) {
    log::info!(
        "[agent_terminal] merge conflict resolution requested: tab={tab_id}"
    );
    if !ensure_agent_enabled(workspace, cx) {
        return;
    }
    let key = merge_key(&repo_path);
    workspace
        .agent_sessions
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    if let Some((_, handle)) =
        workspace.agent_sessions.iter().find(|(entry, handle)| {
            entry == &key
                && handle.read(cx).is_ok_and(|session| session.is_running())
        })
    {
        log::debug!(
            "[agent_terminal] merge resolution activated existing session: tab={tab_id}"
        );
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    if workspace
        .agent_preflight_keys
        .iter()
        .any(|entry| entry == &key)
    {
        log::debug!(
            "[agent_terminal] merge resolution ignored: preflight already running"
        );
        return;
    }
    workspace.agent_preflight_keys.insert(key.clone());
    let entity = cx.entity();
    let path = PathBuf::from(repo_path);
    let probe_path = path.clone();
    let source = merge_head;
    cx.spawn(async move |_, cx| {
        let result = cx
            .background_executor()
            .spawn(async move {
                prepare_merge_resolution(
                    &probe_path,
                    &source,
                    baseline_head.as_deref(),
                )
            })
            .await;
        let _ = entity.update(cx, |workspace, cx| {
            workspace.agent_preflight_keys.retain(|entry| entry != &key);
            match result {
                Ok(prepared) => {
                    log::info!(
                        "[agent_terminal] merge resolution preflight passed: tab={tab_id}"
                    );
                    start_merge_session(workspace, tab_id, path, prepared, cx)
                }
                Err(error) => {
                    log::warn!(
                        "[agent_terminal] merge resolution preflight failed: {}",
                        log_summary(&error)
                    );
                    workspace.agent_merge_preflight_failed(
                        tab_id,
                        first_line(&error).to_string(),
                        cx,
                    )
                }
            }
        });
    })
    .detach();
}

fn open_merge_preflight(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    source: Option<String>,
    cx: &mut Context<Workspace>,
) {
    log::debug!(
        "[agent_terminal] merge preflight started: tab={}, source_present={}",
        tab_id,
        source.is_some()
    );
    let key = merge_key(&repo_path);
    workspace
        .agent_sessions
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    if workspace.agent_sessions.iter().any(|(entry, handle)| {
        entry == &key
            && handle.read(cx).is_ok_and(|session| session.is_running())
    }) {
        if let Some((_, handle)) =
            workspace.agent_sessions.iter().find(|(entry, handle)| {
                entry == &key
                    && handle.read(cx).is_ok_and(|session| session.is_running())
            })
        {
            log::debug!(
                "[agent_terminal] merge activated existing session: tab={tab_id}"
            );
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        }
        return;
    }
    if workspace
        .agent_preflight_keys
        .iter()
        .any(|entry| entry == &key)
    {
        log::debug!(
            "[agent_terminal] merge ignored: preflight already running"
        );
        return;
    }
    workspace.agent_preflight_keys.insert(key.clone());
    let entity = cx.entity();
    let path = PathBuf::from(repo_path);
    let probe_path = path.clone();
    cx.spawn(async move |_, cx| {
        let result = if let Some(source) = source {
            cx.background_executor()
                .spawn(async move { prepare_merge(&probe_path, &source) })
                .await
        } else {
            Err("missing merge source".to_string())
        };
        let _ = entity.update(cx, |workspace, cx| {
            workspace.agent_preflight_keys.retain(|entry| entry != &key);
            match result {
                Ok(prepared) => {
                    log::info!(
                        "[agent_terminal] merge preflight passed: tab={tab_id}"
                    );
                    start_merge_session(workspace, tab_id, path, prepared, cx)
                }
                Err(error) => {
                    log::warn!(
                        "[agent_terminal] merge preflight failed: {}",
                        log_summary(&error)
                    );
                    workspace.agent_merge_preflight_failed(
                        tab_id,
                        first_line(&error).to_string(),
                        cx,
                    )
                }
            }
        });
    })
    .detach();
}

fn start_merge_session(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: PathBuf,
    prepared: PreparedMerge,
    cx: &mut Context<Workspace>,
) {
    let key = merge_key(&repo_path.to_string_lossy());
    let tab_busy = workspace
        .tabs
        .iter()
        .find(|entry| entry.id == tab_id)
        .map(|entry| match &entry.content {
            super::TabContent::Repo(tab) => tab.read(cx).is_busy(),
            super::TabContent::Welcome => true,
        })
        .unwrap_or(true);
    if tab_busy {
        log::warn!(
            "[agent_terminal] merge preflight stopped: repository became busy before launch"
        );
        workspace.agent_merge_preflight_failed(
            tab_id,
            i18n::text(workspace.locale, "agent-merge-repository-busy"),
            cx,
        );
        return;
    }
    if workspace.agent_sessions.iter().any(|(entry, handle)| {
        entry == &key
            && handle.read(cx).is_ok_and(|session| session.is_running())
    }) {
        return;
    }
    let mode = prepared.mode;
    let operation = match &mode {
        AgentMergeMode::Start { target_oid } => AgentOperation::Merge {
            target_oid: target_oid.clone(),
            baseline_head: prepared.baseline.head.clone(),
        },
        AgentMergeMode::Resolve { merge_head_oid } => {
            AgentOperation::ResolveMerge {
                merge_head_oid: merge_head_oid.clone(),
                baseline_head: prepared.baseline.head.clone(),
            }
        }
    };
    let challenge = AgentOperationChallenge::new();
    let prompt = match operation.prompt_with_challenge(None, &challenge) {
        Ok(prompt) => prompt,
        Err(error) => {
            workspace.agent_merge_preflight_failed(
                tab_id,
                first_line(&error.to_string()).to_string(),
                cx,
            );
            return;
        }
    };
    let profile_id = workspace.config.agent.default_profile_id();
    let (profile, spec, startup_error) =
        match workspace.config.agent.profile(&profile_id) {
            Some(profile) => {
                let (spec, launch_error) =
                    launch_for_profile(workspace, &profile, &prompt, cx);
                (profile, spec, launch_error)
            }
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
                    AgentLaunchSpec {
                        executable: PathBuf::new(),
                        args: Vec::new(),
                    },
                    Some(i18n::text_args(
                        workspace.locale,
                        "agent-merge-invalid-profile",
                        &[("profile", &profile_id)],
                    )),
                )
            }
        };
    let session_id = next_session_id();
    let completion = MergeCompletion {
        workspace: cx.entity().downgrade(),
        tab_id,
        session_id,
    };
    open_session_window(
        workspace,
        key,
        AgentSessionKind::Merge,
        workspace.locale,
        profile,
        spec,
        prompt,
        None,
        repo_path,
        None,
        Some(completion),
        None,
        Some(challenge),
        Some(mode),
        Some(prepared.baseline),
        startup_error,
        None,
        Some((tab_id, session_id)),
        None,
        None,
        None,
        None,
        None,
        cx,
    );
}

/// Open or activate a visible Agent session that performs a complete rebase.
pub(super) fn open_rebase(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    source: String,
    cx: &mut Context<Workspace>,
) {
    if !ensure_agent_enabled(workspace, cx) {
        return;
    }
    open_rebase_preflight(workspace, tab_id, repo_path, source, cx);
}

/// Open or activate a visible Agent session that continues an existing
/// rebase, including one left by `git pull --rebase`.
pub(super) fn open_rebase_resolution(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    rebase_head: Option<String>,
    upstream_oid: Option<String>,
    baseline_head: Option<String>,
    cx: &mut Context<Workspace>,
) {
    if !ensure_agent_enabled(workspace, cx) {
        return;
    }
    let key = rebase_key(&repo_path);
    workspace
        .agent_sessions
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    if let Some((_, handle)) =
        workspace.agent_sessions.iter().find(|(entry, handle)| {
            entry == &key
                && handle.read(cx).is_ok_and(|session| session.is_running())
        })
    {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    if workspace
        .agent_preflight_keys
        .iter()
        .any(|entry| entry == &key)
    {
        return;
    }
    workspace.agent_preflight_keys.insert(key.clone());
    let entity = cx.entity();
    let path = PathBuf::from(repo_path);
    let probe_path = path.clone();
    cx.spawn(async move |_, cx| {
        let result = cx
            .background_executor()
            .spawn(async move {
                prepare_rebase_resolution(
                    &probe_path,
                    rebase_head.as_deref(),
                    upstream_oid.as_deref(),
                    baseline_head.as_deref(),
                )
            })
            .await;
        let _ = entity.update(cx, |workspace, cx| {
            workspace.agent_preflight_keys.retain(|entry| entry != &key);
            match result {
                Ok(prepared) => {
                    start_rebase_session(workspace, tab_id, path, prepared, cx)
                }
                Err(error) => workspace.agent_rebase_preflight_failed(
                    tab_id,
                    first_line(&error).to_string(),
                    cx,
                ),
            }
        });
    })
    .detach();
}

fn open_rebase_preflight(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: String,
    source: String,
    cx: &mut Context<Workspace>,
) {
    let key = rebase_key(&repo_path);
    workspace
        .agent_sessions
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    if let Some((_, handle)) =
        workspace.agent_sessions.iter().find(|(entry, handle)| {
            entry == &key
                && handle.read(cx).is_ok_and(|session| session.is_running())
        })
    {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    if workspace
        .agent_preflight_keys
        .iter()
        .any(|entry| entry == &key)
    {
        return;
    }
    workspace.agent_preflight_keys.insert(key.clone());
    let entity = cx.entity();
    let path = PathBuf::from(repo_path);
    let probe_path = path.clone();
    cx.spawn(async move |_, cx| {
        let result = cx
            .background_executor()
            .spawn(async move { prepare_rebase(&probe_path, &source) })
            .await;
        let _ = entity.update(cx, |workspace, cx| {
            workspace.agent_preflight_keys.retain(|entry| entry != &key);
            match result {
                Ok(prepared) => {
                    start_rebase_session(workspace, tab_id, path, prepared, cx)
                }
                Err(error) => workspace.agent_rebase_preflight_failed(
                    tab_id,
                    first_line(&error).to_string(),
                    cx,
                ),
            }
        });
    })
    .detach();
}

fn start_rebase_session(
    workspace: &mut Workspace,
    tab_id: TabId,
    repo_path: PathBuf,
    prepared: PreparedRebase,
    cx: &mut Context<Workspace>,
) {
    let key = rebase_key(&repo_path.to_string_lossy());
    let tab_busy = workspace
        .tabs
        .iter()
        .find(|entry| entry.id == tab_id)
        .map(|entry| match &entry.content {
            super::TabContent::Repo(tab) => tab.read(cx).is_busy(),
            super::TabContent::Welcome => true,
        })
        .unwrap_or(true);
    if tab_busy {
        log::info!(
            "[agent_terminal] rebase preflight completed after the repository became busy"
        );
        return;
    }
    if workspace.agent_sessions.iter().any(|(entry, handle)| {
        entry == &key
            && handle.read(cx).is_ok_and(|session| session.is_running())
    }) {
        return;
    }
    let mode = prepared.mode;
    let operation = match &mode {
        AgentRebaseMode::Start { upstream_oid } => AgentOperation::Rebase {
            upstream_oid: upstream_oid.clone(),
            baseline_head: prepared.baseline.head.clone(),
        },
        AgentRebaseMode::Resolve {
            rebase_head_oid,
            upstream_oid,
        } => AgentOperation::ResolveRebase {
            rebase_head_oid: rebase_head_oid.clone(),
            upstream_oid: upstream_oid.clone(),
            baseline_head: prepared.baseline.head.clone(),
        },
    };
    let challenge = AgentOperationChallenge::new();
    let prompt = match operation.prompt_with_challenge(None, &challenge) {
        Ok(prompt) => prompt,
        Err(error) => {
            workspace.agent_rebase_preflight_failed(
                tab_id,
                first_line(&error.to_string()).to_string(),
                cx,
            );
            return;
        }
    };
    let profile_id = workspace.config.agent.default_profile_id();
    let (profile, spec, startup_error) =
        match workspace.config.agent.profile(&profile_id) {
            Some(profile) => {
                let (spec, launch_error) =
                    launch_for_profile(workspace, &profile, &prompt, cx);
                (profile, spec, launch_error)
            }
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
                    AgentLaunchSpec {
                        executable: PathBuf::new(),
                        args: Vec::new(),
                    },
                    Some(i18n::text_args(
                        workspace.locale,
                        "agent-rebase-invalid-profile",
                        &[("profile", &profile_id)],
                    )),
                )
            }
        };
    let session_id = next_session_id();
    let completion = RebaseCompletion {
        workspace: cx.entity().downgrade(),
        tab_id,
        session_id,
    };
    open_session_window(
        workspace,
        key,
        AgentSessionKind::Rebase,
        workspace.locale,
        profile,
        spec,
        prompt,
        None,
        repo_path,
        None,
        None,
        None,
        None,
        None,
        None,
        startup_error,
        None,
        None,
        Some(completion),
        Some(challenge),
        Some(mode),
        Some(prepared.baseline),
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
            let _ = handle.update(cx, |view, window, cx| {
                if view.is_running() {
                    stopped = true;
                    view.stop(cx);
                    window.remove_window();
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

    pub(super) fn begin_agent_merge(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| tab.begin_agent_merge(session_id, cx));
        }
    }

    pub(super) fn observe_agent_merge(
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
                tab.observe_agent_merge(session_id, oid, cx)
            });
        }
    }

    pub(super) fn finish_agent_merge(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        outcome: AgentMergeOutcome,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| {
                tab.finish_agent_merge(session_id, outcome, cx)
            });
        }
    }

    pub(super) fn agent_merge_preflight_failed(
        &mut self,
        tab_id: TabId,
        summary: String,
        cx: &mut Context<Self>,
    ) {
        log::warn!(
            "[agent_terminal] merge preflight failure surfaced: tab={}, {}",
            tab_id,
            log_summary(&summary)
        );
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| {
                tab.agent_merge_preflight_failed(summary, cx)
            });
        }
    }

    pub(super) fn begin_agent_rebase(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| tab.begin_agent_rebase(session_id, cx));
        }
    }

    pub(super) fn observe_agent_rebase(
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
                tab.observe_agent_rebase(session_id, oid, cx)
            });
        }
    }

    pub(super) fn finish_agent_rebase(
        &mut self,
        tab_id: TabId,
        session_id: u64,
        outcome: AgentRebaseOutcome,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| {
                tab.finish_agent_rebase(session_id, outcome, cx)
            });
        }
    }

    pub(super) fn agent_rebase_preflight_failed(
        &mut self,
        tab_id: TabId,
        summary: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab_id)
            && let super::TabContent::Repo(tab) = &entry.content
        {
            tab.update(cx, |tab, cx| {
                tab.agent_rebase_preflight_failed(summary, cx)
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

/// Keep diagnostic log entries bounded and single-line without recording the
/// prompt, terminal output, or repository path.
fn log_summary(value: &str) -> String {
    first_line(value).chars().take(240).collect()
}

fn format_exit_code(code: Option<i32>) -> String {
    code.map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
