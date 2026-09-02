use gpui::prelude::*;
use gpui::*;
use gpui_component::v_flex;

use crate::core::config::{GraphHistoryPreference, LayoutSettings};
use crate::core::git::{
    CheckoutTarget, LogScope, WorkingTreeAction, WorkingTreeScope,
};
use crate::core::i18n::{self, Locale};
use crate::git::changes_panel::ChangesPanel;
use crate::git::diff_view::DiffLayoutMode;
use crate::git::graph::GraphView;
use crate::git::panel::{BottomPanel, CommitPanel};
use crate::git::sidebar::Sidebar;
use crate::git::toolbar::Toolbar;
use crate::git::{GitStatus, GitView};

use super::agent_commit::AgentCommitOutcome;
use super::tabs::{TabId, TabState, TabSummary};

mod branch_compare;
mod branch_ops;
mod dialogs;
mod layout;
mod subscriptions;

#[derive(Clone, Debug)]
pub enum RepoTabEvent {
    Opened {
        id: TabId,
        path: String,
    },
    SummaryChanged(TabSummary),
    RequestSettings,
    LayoutChanged(LayoutSettings),
    AgentCommitRequested {
        id: TabId,
        repo_path: String,
        hint: String,
    },
}

enum PendingConfirmation {
    ForcePush,
    PushSetUpstream {
        branch: String,
        remote: String,
    },
    Discard {
        scope: WorkingTreeScope,
        tracked_count: usize,
        untracked_count: usize,
    },
}

#[derive(Clone, Debug)]
pub struct SidebarResize;
#[derive(Clone, Debug)]
pub struct RightPanelResize;
#[derive(Clone, Debug)]
pub struct DiffViewerResize;

pub(super) const MIN_COMMIT_HEIGHT: f32 = 120.0;
pub(super) const DIFF_RESIZE_HANDLE_HEIGHT: f32 = 3.0;

pub struct RepoTab {
    id: TabId,
    repo_path: String,
    opened: bool,
    branch: String,
    /// Tracked upstream of the current branch from the latest status.
    upstream: Option<String>,
    /// Configured remote names from the latest refs snapshot.
    remotes: Vec<String>,
    /// Persisted commit-graph history scope chosen in settings.
    graph_history: GraphHistoryPreference,
    /// Log scope last sent to the Git worker (None until first status).
    log_scope: Option<LogScope>,
    /// Non-head local branch names (merge/rebase source candidates).
    local_branches: Vec<String>,
    /// Stash record count from the latest refs snapshot.
    stash_count: usize,
    /// Files with stashable (tracked) changes in the latest status snapshot.
    local_change_count: usize,
    git_view: Entity<GitView>,
    sidebar: Entity<Sidebar>,
    graph: Entity<GraphView>,
    toolbar: Entity<Toolbar>,
    commit: Entity<CommitPanel>,
    changes: Entity<ChangesPanel>,
    bottom: Entity<BottomPanel>,
    compare: Entity<crate::git::branch_compare::BranchCompareView>,
    compare_window: Option<AnyWindowHandle>,
    compare_window_closed: Option<Subscription>,
    status: GitStatus,
    status_message: Option<String>,
    status_message_ok: Option<bool>,
    working_diff_request_id: u64,
    working_tree_operation_id: u64,
    operation_busy: bool,
    agent_commit_session_id: Option<u64>,
    agent_commit_observed_head: Option<String>,
    pending_agent_refresh: bool,
    layout: LayoutSettings,
    confirmation: Option<PendingConfirmation>,
    dialogs: branch_ops::BranchDialogs,
    locale: Locale,
}

impl EventEmitter<RepoTabEvent> for RepoTab {}

impl RepoTab {
    pub fn new(
        id: TabId,
        repo_path: String,
        locale: Locale,
        diff_layout: DiffLayoutMode,
        graph_history: GraphHistoryPreference,
        mut layout: LayoutSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        layout.normalize();
        let git_view = cx.new(|cx| GitView::new(locale, cx));
        let sidebar = cx.new(|cx| Sidebar::new(window, cx, locale));
        let graph = cx.new(|cx| GraphView::new(id, locale, window, cx));
        let toolbar = cx.new(|_cx| Toolbar::new(locale));
        let commit = cx.new(|cx| CommitPanel::new(window, cx, locale));
        let changes = cx.new(|_cx| ChangesPanel::new(locale));
        let bottom = cx.new(|_cx| {
            BottomPanel::new(locale, diff_layout, layout.file_list_ratio)
        });
        let compare = branch_compare::new_view(window, cx, locale, diff_layout);
        branch_compare::subscribe(&compare, window, cx);

        subscriptions::wire(
            &git_view, &sidebar, &toolbar, &graph, &commit, &changes, &bottom,
            window, cx,
        );

        Self {
            id,
            repo_path,
            opened: false,
            branch: String::new(),
            upstream: None,
            remotes: Vec::new(),
            graph_history,
            log_scope: None,
            local_branches: Vec::new(),
            stash_count: 0,
            local_change_count: 0,
            git_view,
            sidebar,
            graph,
            toolbar,
            commit,
            changes,
            bottom,
            compare,
            compare_window: None,
            compare_window_closed: None,
            status: GitStatus::None,
            status_message: None,
            status_message_ok: None,
            working_diff_request_id: 0,
            working_tree_operation_id: 0,
            operation_busy: false,
            agent_commit_session_id: None,
            agent_commit_observed_head: None,
            pending_agent_refresh: false,
            layout,
            confirmation: None,
            dialogs: branch_ops::BranchDialogs::default(),
            locale,
        }
    }

    /// Refresh this tab after the window regained activation.
    /// Returns whether a refresh was actually requested.
    pub(super) fn refresh_on_focus(&mut self, cx: &mut Context<Self>) -> bool {
        self.refresh_if_ready(cx)
    }

    /// Refresh this tab after it becomes active through a tab switch.
    /// Returns whether a refresh was actually requested.
    pub(super) fn refresh_on_tab_switch(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        self.refresh_if_ready(cx)
    }

    fn refresh_if_ready(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.opened || self.is_busy() {
            return false;
        }
        self.refresh_repository(cx);
        true
    }

    fn refresh_repository(&mut self, cx: &mut Context<Self>) {
        let refresh_working_diff = self.bottom.read(cx).has_working_tree_diff();
        self.changes.update(cx, |changes, _cx| {
            changes.set_refresh_selected(refresh_working_diff);
        });
        self.git_view.update(cx, |view, _| view.refresh());
    }

    pub(super) fn is_busy(&self) -> bool {
        self.operation_busy || self.agent_commit_session_id.is_some()
    }

    fn sync_busy_controls(&mut self, cx: &mut Context<Self>) {
        let busy = self.is_busy();
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_busy(busy, cx);
        });
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_busy(busy, cx);
        });
        self.graph.update(cx, |graph, cx| {
            graph.set_busy(busy, cx);
        });
        self.commit.update(cx, |commit, cx| {
            commit.set_busy(busy, cx);
        });
        self.changes.update(cx, |changes, cx| {
            changes.set_busy(busy, cx);
        });
    }

    fn set_operation_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.operation_busy == busy {
            return;
        }
        self.operation_busy = busy;
        self.sync_busy_controls(cx);
        if !busy && self.pending_agent_refresh && !self.is_busy() {
            self.pending_agent_refresh = false;
            self.refresh_repository(cx);
        }
    }

    pub(super) fn begin_agent_commit(
        &mut self,
        session_id: u64,
        cx: &mut Context<Self>,
    ) {
        if self.agent_commit_session_id.is_some() {
            return;
        }
        self.agent_commit_session_id = Some(session_id);
        self.agent_commit_observed_head = None;
        self.sync_busy_controls(cx);
        cx.notify();
    }

    pub(super) fn observe_agent_commit(
        &mut self,
        session_id: u64,
        oid: String,
        cx: &mut Context<Self>,
    ) {
        if self.agent_commit_session_id != Some(session_id)
            || self.agent_commit_observed_head.as_deref() == Some(&oid)
        {
            return;
        }
        self.agent_commit_observed_head = Some(oid.clone());
        self.status_message = Some(i18n::text_args(
            self.locale,
            "agent-commit-observed",
            &[("oid", &short_oid(&oid))],
        ));
        self.status_message_ok = None;
        self.refresh_repository(cx);
        cx.notify();
    }

    pub(super) fn finish_agent_commit(
        &mut self,
        session_id: u64,
        outcome: AgentCommitOutcome,
        cx: &mut Context<Self>,
    ) {
        if self.agent_commit_session_id != Some(session_id) {
            return;
        }
        self.agent_commit_session_id = None;
        self.agent_commit_observed_head = None;
        let (message, ok) = match &outcome {
            AgentCommitOutcome::Committed { oid } => (
                i18n::text_args(
                    self.locale,
                    "agent-commit-created",
                    &[("oid", &short_oid(oid))],
                ),
                true,
            ),
            AgentCommitOutcome::NoChanges => {
                (i18n::text(self.locale, "agent-commit-no-changes"), false)
            }
            AgentCommitOutcome::Conflict => {
                (i18n::text(self.locale, "agent-commit-conflict"), false)
            }
            AgentCommitOutcome::Failed => {
                (i18n::text(self.locale, "agent-commit-failed"), false)
            }
            AgentCommitOutcome::Cancelled => {
                (i18n::text(self.locale, "agent-commit-cancelled"), false)
            }
            AgentCommitOutcome::ExitedUnverified { code } => (
                i18n::text_args(
                    self.locale,
                    "agent-commit-unverified",
                    &[("code", &format_exit_code(*code))],
                ),
                false,
            ),
        };
        self.status_message = Some(message);
        self.status_message_ok = Some(ok);
        if self.operation_busy {
            self.pending_agent_refresh = true;
        } else {
            self.refresh_repository(cx);
        }
        self.sync_busy_controls(cx);
        cx.notify();
    }

    fn start_working_tree_operation(
        &mut self,
        action: WorkingTreeAction,
        scope: WorkingTreeScope,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        self.working_tree_operation_id =
            self.working_tree_operation_id.wrapping_add(1).max(1);
        let request_id = self.working_tree_operation_id;
        log::info!(
            "[git_worktree] operation requested: request_id={}, action={}, scope={:?}",
            request_id,
            action.description(),
            scope.kind()
        );
        self.set_operation_busy(true, cx);
        self.git_view.update(cx, |view, _| {
            view.working_tree_operation(request_id, action, scope);
        });
        cx.notify();
    }

    fn start_checkout(
        &mut self,
        target: CheckoutTarget,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        self.git_view.update(cx, |view, _| view.checkout(target));
        self.set_operation_busy(true, cx);
    }

    fn copy_ref(&mut self, value: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()));
        self.status_message = Some(i18n::text_args(
            self.locale,
            "context-copied",
            &[("name", value)],
        ));
        self.status_message_ok = Some(true);
        cx.notify();
    }

    fn start_copy_commit_message(
        &mut self,
        oid: String,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        self.git_view.update(cx, |view, _| {
            view.copy_commit_message(oid);
        });
        self.set_operation_busy(true, cx);
    }

    fn finish_copy_commit_message(
        &mut self,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(message.to_string()));
        self.status_message =
            Some(i18n::text(self.locale, "context-copied-commit-message"));
        self.status_message_ok = Some(true);
        cx.notify();
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        if self.opened {
            return;
        }
        self.opened = true;
        self.status = GitStatus::Scanning;
        self.emit_summary(cx);
        let path = self.repo_path.clone();
        self.git_view
            .update(cx, |view, cx| view.open_repo(&path, cx));
    }

    /// Make this repository the active UI tab and start consuming its events.
    /// Returns whether the repository was already opened before activation.
    pub fn activate(&mut self, cx: &mut Context<Self>) -> bool {
        let was_opened = self.opened;
        self.open(cx);
        self.git_view
            .update(cx, |view, cx| view.set_active(true, cx));
        was_opened
    }

    /// Stop consuming events while retaining the repository state and worker.
    pub fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.git_view
            .update(cx, |view, cx| view.set_active(false, cx));
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        branch_compare::close(self, cx);
        self.git_view.update(cx, |view, _| view.close_repo());
        self.opened = false;
        // A reopened repository starts a fresh worker on AllBranches, so the
        // scope cache must be invalidated to force a re-sync.
        self.log_scope = None;
    }

    pub fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        self.git_view.update(cx, |view, _| view.set_locale(locale));
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_locale(locale, cx);
        });
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_locale(locale, cx);
        });
        self.graph.update(cx, |graph, cx| {
            graph.set_locale(locale, window, cx);
        });
        self.commit.update(cx, |commit, cx| {
            commit.set_locale(locale, window, cx);
        });
        self.changes.update(cx, |changes, cx| {
            changes.set_locale(locale, cx);
        });
        self.bottom.update(cx, |bottom, cx| {
            bottom.set_locale(locale, cx);
        });
        branch_compare::set_locale(self, locale, cx);
        cx.notify();
    }

    /// Apply the persisted diff layout chosen in the settings overlay.
    pub fn set_diff_layout(
        &mut self,
        diff_layout: DiffLayoutMode,
        cx: &mut Context<Self>,
    ) {
        self.bottom.update(cx, |bottom, cx| {
            bottom.set_diff_layout(diff_layout, cx);
        });
        branch_compare::set_diff_layout(self, diff_layout, cx);
    }

    /// Apply the persisted commit graph history scope to this repository tab.
    pub fn set_graph_history(
        &mut self,
        preference: GraphHistoryPreference,
        cx: &mut Context<Self>,
    ) {
        self.graph_history = preference;
        self.sync_log_scope(cx);
    }

    /// Send the commit-graph log scope to the worker when it changes.
    ///
    /// The worker starts on `AllBranches` and reloads the first page on every
    /// refresh, so the initial matching state needs no explicit query.
    fn sync_log_scope(&mut self, cx: &mut Context<Self>) {
        let scope = match self.graph_history {
            GraphHistoryPreference::AllBranches => LogScope::AllBranches,
            GraphHistoryPreference::CurrentBranch => LogScope::CurrentBranch {
                upstream: self.upstream.clone(),
            },
        };
        if self.log_scope == Some(scope.clone()) {
            return;
        }
        if self.log_scope.is_none() && scope == LogScope::AllBranches {
            self.log_scope = Some(scope);
            return;
        }
        log::info!(
            "[git_view] log scope changed: {scope:?}, upstream={:?}",
            self.upstream
        );
        self.log_scope = Some(scope.clone());
        self.git_view
            .update(cx, |view, _| view.set_log_scope(scope));
    }

    pub fn set_layout(
        &mut self,
        mut layout: LayoutSettings,
        cx: &mut Context<Self>,
    ) {
        layout.normalize();
        self.layout = layout.clone();
        self.bottom.update(cx, |bottom, cx| {
            bottom.set_file_list_ratio(layout.file_list_ratio, cx);
        });
        cx.notify();
    }

    pub fn focus_branches(&mut self, cx: &mut Context<Self>) {
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.flash_branches(cx);
        });
    }

    pub fn summary(&self) -> TabSummary {
        let state = match self.status {
            GitStatus::Error(_) => TabState::Error,
            GitStatus::Ready(_) => TabState::Ready,
            GitStatus::None | GitStatus::Scanning => TabState::Loading,
        };
        TabSummary {
            id: self.id,
            title: repo_title(&self.repo_path),
            branch: (!self.branch.is_empty()).then(|| self.branch.clone()),
            state,
        }
    }

    fn emit_summary(&self, cx: &mut Context<Self>) {
        cx.emit(RepoTabEvent::SummaryChanged(self.summary()));
    }
}

fn short_oid(oid: &str) -> String {
    oid.get(..7).unwrap_or(oid).to_string()
}

impl Render for RepoTab {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .id(SharedString::from(format!("repo-content-{}", self.id)))
            .relative()
            .size_full()
            .min_h_0()
            .child(self.toolbar.clone())
            .child(branch_compare::render(self, window, cx))
            .child(self.status_bar(cx))
            .when(self.confirmation.is_some(), |element| {
                element.child(self.confirmation_overlay(cx))
            })
            .when(self.dialogs.pending.is_some(), |element| {
                element
                    .children(RepoTab::render_branch_dialog(self, window, cx))
            })
    }
}

fn repo_title(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        path.to_string()
    } else {
        crate::git::dir_name(trimmed).to_string()
    }
}

fn format_exit_code(code: Option<i32>) -> String {
    code.map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
