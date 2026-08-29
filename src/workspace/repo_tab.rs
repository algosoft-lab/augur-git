use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, h_flex, v_flex};

use crate::core::config::LayoutSettings;
use crate::core::git::{
    CheckoutTarget, WorkingTreeAction, WorkingTreeDiffKind, WorkingTreeScope,
    WorkingTreeScopeKind,
};
use crate::core::i18n::{self, Locale};
use crate::git::changes_panel::{ChangesPanel, ChangesPanelEvent};
use crate::git::diff_view::DiffLayoutMode;
use crate::git::graph::{GraphEvent, GraphView};
use crate::git::panel::{
    BottomPanel, BottomPanelEvent, CommitPanel, CommitPanelEvent,
};
use crate::git::sidebar::{Sidebar, SidebarEvent};
use crate::git::toolbar::{Toolbar, ToolbarEvent};
use crate::git::{GitStatus, GitUiEvent, GitView};

use super::tabs::{TabId, TabState, TabSummary};

mod dialogs;
mod layout;

#[derive(Clone, Debug)]
pub enum RepoTabEvent {
    Opened { id: TabId, path: String },
    SummaryChanged(TabSummary),
    RequestSettings,
    LayoutChanged(LayoutSettings),
}

enum PendingConfirmation {
    ForcePush,
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

impl Render for SidebarResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

impl Render for RightPanelResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

impl Render for DiffViewerResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

pub(super) const MIN_COMMIT_HEIGHT: f32 = 120.0;
pub(super) const DIFF_RESIZE_HANDLE_HEIGHT: f32 = 3.0;

pub struct RepoTab {
    id: TabId,
    repo_path: String,
    opened: bool,
    branch: String,
    git_view: Entity<GitView>,
    sidebar: Entity<Sidebar>,
    graph: Entity<GraphView>,
    toolbar: Entity<Toolbar>,
    commit: Entity<CommitPanel>,
    changes: Entity<ChangesPanel>,
    bottom: Entity<BottomPanel>,
    status: GitStatus,
    status_message: Option<String>,
    status_message_ok: Option<bool>,
    working_diff_request_id: u64,
    working_tree_operation_id: u64,
    operation_busy: bool,
    layout: LayoutSettings,
    confirmation: Option<PendingConfirmation>,
    locale: Locale,
}

impl EventEmitter<RepoTabEvent> for RepoTab {}

impl RepoTab {
    pub fn new(
        id: TabId,
        repo_path: String,
        locale: Locale,
        diff_layout: DiffLayoutMode,
        mut layout: LayoutSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        layout.normalize();
        let git_view = cx.new(|cx| GitView::new(locale, cx));
        let sidebar = cx.new(|cx| Sidebar::new(window, cx, locale));
        let graph = cx.new(|cx| GraphView::new(id, locale, cx));
        let toolbar = cx.new(|_cx| Toolbar::new(locale));
        let commit = cx.new(|cx| CommitPanel::new(window, cx, locale));
        let changes = cx.new(|_cx| ChangesPanel::new(locale));
        let bottom = cx.new(|_cx| {
            BottomPanel::new(locale, diff_layout, layout.file_list_ratio)
        });

        cx.subscribe(&sidebar, |tab, _event, event, cx| match event {
            SidebarEvent::BranchSelected(name) => {
                tab.status_message = Some(i18n::text_args(
                    tab.locale,
                    "branch-selected",
                    &[("name", name)],
                ));
                cx.notify();
            }
            SidebarEvent::CheckoutRef(target) => {
                tab.start_checkout(target.clone(), cx);
            }
            SidebarEvent::CopyRef(value) => {
                tab.copy_ref(value, cx);
            }
        })
        .detach();

        cx.subscribe(&toolbar, |tab, _event, event, cx| match event {
            ToolbarEvent::Fetch => {
                if tab.operation_busy {
                    return;
                }
                tab.git_view.update(cx, |view, _| {
                    view.run(
                        "fetch --all",
                        vec!["fetch".into(), "--all".into()],
                    );
                });
                tab.set_operation_busy(true, cx);
            }
            ToolbarEvent::PullMerge => {
                if tab.operation_busy {
                    return;
                }
                tab.git_view.update(cx, |view, _| {
                    view.run("pull", vec!["pull".into()]);
                });
                tab.set_operation_busy(true, cx);
            }
            ToolbarEvent::PullRebase => {
                if tab.operation_busy {
                    return;
                }
                tab.git_view.update(cx, |view, _| {
                    view.run(
                        "pull --rebase",
                        vec!["pull".into(), "--rebase".into()],
                    );
                });
                tab.set_operation_busy(true, cx);
            }
            ToolbarEvent::Push => {
                if tab.operation_busy {
                    return;
                }
                tab.git_view.update(cx, |view, _| {
                    view.run("push", vec!["push".into()]);
                });
                tab.set_operation_busy(true, cx);
            }
            ToolbarEvent::PushForce => {
                // Never run directly: open the confirmation dialog first.
                if !tab.operation_busy {
                    tab.confirmation = Some(PendingConfirmation::ForcePush);
                    cx.notify();
                }
            }
            ToolbarEvent::Branch => {
                tab.sidebar.update(cx, |sidebar, cx| {
                    sidebar.flash_branches(cx);
                });
            }
            ToolbarEvent::Refresh => {
                tab.refresh_repository(cx);
            }
            ToolbarEvent::Settings => {
                cx.emit(RepoTabEvent::RequestSettings);
            }
        })
        .detach();

        cx.subscribe(&graph, |tab, _event, event, cx| match event {
            GraphEvent::CommitSelected {
                oid,
                short,
                subject,
                ..
            } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_commit(oid, short, subject, cx);
                });
                tab.git_view.update(cx, |view, _| {
                    view.commit_files(oid.clone());
                    view.commit_message(oid.clone());
                });
            }
            GraphEvent::CommitMessageRequested(oid) => {
                tab.git_view.update(cx, |view, _| {
                    view.commit_message(oid.clone());
                });
            }
            GraphEvent::CheckoutRef(target) => {
                tab.start_checkout(target.clone(), cx);
            }
            GraphEvent::CopyRef(value) => {
                tab.copy_ref(value, cx);
            }
            GraphEvent::CopyCommitMessage(oid) => {
                tab.start_copy_commit_message(oid.clone(), cx);
            }
        })
        .detach();

        cx.subscribe(&commit, |tab, _event, event, cx| match event {
            CommitPanelEvent::Submit { message, amend } => {
                if tab.operation_busy {
                    return;
                }
                log::info!("[commit_panel] submit requested: amend={amend}");
                tab.git_view.update(cx, |view, _| {
                    view.commit(message.clone(), *amend);
                });
                tab.set_operation_busy(true, cx);
            }
        })
        .detach();

        cx.subscribe(&changes, |tab, _event, event, cx| match event {
            ChangesPanelEvent::FileSelected { staged, file } => {
                tab.working_diff_request_id =
                    tab.working_diff_request_id.wrapping_add(1).max(1);
                let request_id = tab.working_diff_request_id;
                let kind = if *staged {
                    WorkingTreeDiffKind::Staged
                } else {
                    WorkingTreeDiffKind::Unstaged
                };
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_working_tree_file(
                        request_id,
                        *staged,
                        file.clone(),
                        cx,
                    );
                });
                tab.git_view.update(cx, |view, _| {
                    view.working_tree_file_diff(request_id, kind, file.clone());
                });
            }
            ChangesPanelEvent::OperationRequested { action, scope } => {
                if *action == WorkingTreeAction::Discard {
                    tab.request_discard(scope.clone(), cx);
                } else {
                    tab.start_working_tree_operation(
                        *action,
                        scope.clone(),
                        cx,
                    );
                }
            }
            ChangesPanelEvent::RefreshRequested => {
                if !tab.operation_busy {
                    tab.refresh_repository(cx);
                }
            }
        })
        .detach();

        cx.subscribe(&bottom, |tab, _event, event, cx| match event {
            BottomPanelEvent::ShowFileDiff {
                oid,
                merge_parent,
                file,
            } => {
                tab.git_view.update(cx, |view, _| {
                    view.file_diff(
                        oid.clone(),
                        merge_parent.clone(),
                        file.clone(),
                    );
                });
            }
            BottomPanelEvent::ShowAllFileDiffs {
                oid,
                merge_parent,
                files,
            } => {
                tab.git_view.update(cx, |view, _| {
                    view.file_diffs(
                        oid.clone(),
                        merge_parent.clone(),
                        files.clone(),
                    );
                });
            }
            BottomPanelEvent::LayoutChanged { file_list_ratio } => {
                tab.layout.file_list_ratio = *file_list_ratio;
                cx.emit(RepoTabEvent::LayoutChanged(tab.layout.clone()));
            }
        })
        .detach();

        cx.subscribe(&git_view, |tab, _event, event, cx| match event {
            GitUiEvent::StatusChanged {
                branch,
                ahead,
                behind,
                files,
                branches,
            } => {
                let branch_name = branch.clone();
                let has_staged =
                    files.iter().any(|file| file.has_staged_changes());
                let staged_count =
                    files.iter().filter(|file| file.has_staged_changes()).count();
                let unstaged_count = files
                    .iter()
                    .filter(|file| {
                        file.is_conflicted() || file.has_worktree_changes()
                    })
                    .count();
                let ahead_text = ahead.to_string();
                let behind_text = behind.to_string();
                let staged_text = staged_count.to_string();
                let unstaged_text = unstaged_count.to_string();

                tab.branch = branch_name;
                tab.status = GitStatus::Ready(i18n::text_args(
                    tab.locale,
                    "status-summary",
                    &[
                        ("branch", branch),
                        ("ahead", &ahead_text),
                        ("behind", &behind_text),
                        ("staged", &staged_text),
                        ("unstaged", &unstaged_text),
                    ],
                ));
                tab.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_status(branch.clone(), branches.clone(), cx);
                });
                tab.changes.update(cx, |changes, cx| {
                    changes.set_files(files.clone(), cx);
                });
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.sync_working_tree_files(files, cx);
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_ahead_behind(*ahead, *behind, cx);
                });
                tab.commit.update(cx, |commit, cx| {
                    commit.set_has_staged(has_staged, cx);
                });
                tab.emit_summary(cx);
                cx.notify();
            }
            GitUiEvent::LogChanged { rows } => {
                tab.graph.update(cx, |graph, cx| {
                    graph.set_rows(rows.clone(), cx);
                });
            }
            GitUiEvent::RefsChanged(refs) => {
                tab.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_refs(refs.clone(), cx);
                });
            }
            GitUiEvent::CommitFilesChanged {
                oid,
                files,
                merge_parent,
            } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_files(
                        oid,
                        merge_parent.clone(),
                        files.clone(),
                        cx,
                    );
                });
            }
            GitUiEvent::CommitMessageChanged { oid, message } => {
                tab.graph.update(cx, |graph, cx| {
                    graph.set_commit_message(&oid, message.clone(), cx);
                });
            }
            GitUiEvent::FileDiffChanged {
                oid,
                file,
                patch,
                old_source,
                new_source,
            } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_diff(
                        oid,
                        file,
                        patch.clone(),
                        old_source.clone(),
                        new_source.clone(),
                        cx,
                    );
                });
            }
            GitUiEvent::WorkingTreeFileDiffChanged {
                request_id,
                kind,
                file,
                patch,
                old_source,
                new_source,
            } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_working_tree_diff(
                        *request_id,
                        *kind,
                        file,
                        patch.clone(),
                        old_source.clone(),
                        new_source.clone(),
                        cx,
                    );
                });
            }
            GitUiEvent::WorkingTreeFileDiffError {
                request_id,
                kind,
                file,
                detail,
            } => {
                log::warn!(
                    "[workspace] working-tree diff unavailable: request_id={}, kind={kind:?}",
                    request_id
                );
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_working_tree_error(
                        *request_id,
                        *kind,
                        file,
                        detail.clone(),
                        cx,
                    );
                });
            }
            GitUiEvent::WorkingTreeOperationFinished {
                request_id,
                action,
                scope,
                success,
                detail,
            } => {
                if *request_id != tab.working_tree_operation_id {
                    log::debug!(
                        "[git_worktree] ignoring stale operation result: request_id={request_id}"
                    );
                    return;
                }
                tab.set_operation_busy(false, cx);
                let label_key = operation_result_key(*action, *scope, *success);
                tab.status_message = Some(if *success {
                    i18n::text(tab.locale, label_key)
                } else {
                    i18n::text_args(
                        tab.locale,
                        "changes-operation-failed",
                        &[("error", first_line(detail))],
                    )
                });
                tab.status_message_ok = Some(*success);
                cx.notify();
            }
            GitUiEvent::CommandDone {
                label,
                success,
                message,
            } => {
                if label == "checkout" {
                    log::info!(
                        "[git_checkout] result received: success={success}"
                    );
                }
                let copy_commit_message = label == "copy-commit-message";
                tab.set_operation_busy(false, cx);
                if copy_commit_message {
                    if *success {
                        tab.finish_copy_commit_message(message, cx);
                    } else {
                        tab.status_message = Some(i18n::text_args(
                            tab.locale,
                            "context-copy-commit-message-failed",
                            &[("error", first_line(message))],
                        ));
                        tab.status_message_ok = Some(false);
                        cx.notify();
                    }
                } else {
                    let refresh_after = matches!(
                        label.as_str(),
                        "commit"
                            | "checkout"
                            | "fetch --all"
                            | "pull"
                            | "pull --rebase"
                            | "push"
                            | "push --force"
                    );
                    tab.status_message = Some(if *success {
                        i18n::text_args(
                            tab.locale,
                            "command-success",
                            &[("label", label)],
                        )
                    } else {
                        i18n::text_args(
                            tab.locale,
                            "command-failed",
                            &[("label", label), ("error", first_line(message))],
                        )
                    });
                    tab.status_message_ok = Some(*success);
                    if *success && refresh_after {
                        tab.refresh_repository(cx);
                    }
                    cx.notify();
                }
            }
            GitUiEvent::RepoOpened(path) => {
                cx.emit(RepoTabEvent::Opened {
                    id: tab.id,
                    path: path.clone(),
                });
                tab.emit_summary(cx);
            }
            GitUiEvent::Error(message) => {
                tab.set_operation_busy(false, cx);
                tab.status = GitStatus::Error(message.clone());
                tab.emit_summary(cx);
                cx.notify();
            }
            GitUiEvent::StatusError(message) => {
                tab.status = GitStatus::Error(message.clone());
                tab.emit_summary(cx);
                cx.notify();
            }
        })
        .detach();

        Self {
            id,
            repo_path,
            opened: false,
            branch: String::new(),
            git_view,
            sidebar,
            graph,
            toolbar,
            commit,
            changes,
            bottom,
            status: GitStatus::None,
            status_message: None,
            status_message_ok: None,
            working_diff_request_id: 0,
            working_tree_operation_id: 0,
            operation_busy: false,
            layout,
            confirmation: None,
            locale,
        }
    }

    /// Refresh this tab's data after the window regained activation.
    /// Returns whether a refresh was actually requested.
    pub(super) fn refresh_on_focus(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.opened || self.operation_busy {
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

    fn set_operation_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.operation_busy == busy {
            return;
        }
        self.operation_busy = busy;
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

    fn start_working_tree_operation(
        &mut self,
        action: WorkingTreeAction,
        scope: WorkingTreeScope,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy {
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
        if self.operation_busy {
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
        if self.operation_busy {
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
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        self.open(cx);
        self.git_view
            .update(cx, |view, cx| view.set_active(true, cx));
    }

    /// Stop consuming events while retaining the repository state and worker.
    pub fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.git_view
            .update(cx, |view, cx| view.set_active(false, cx));
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.git_view.update(cx, |view, _| view.close_repo());
        self.opened = false;
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
            graph.set_locale(locale, cx);
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

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let (text, color) = match &self.status {
            GitStatus::None => (
                i18n::text(self.locale, "no-repo-open"),
                colors.muted_foreground,
            ),
            GitStatus::Scanning => {
                (i18n::text(self.locale, "status-scanning"), colors.warning)
            }
            GitStatus::Ready(label) => (format!("● {label}"), colors.green),
            GitStatus::Error(message) => (format!("✗ {message}"), colors.red),
        };
        let msg = self.status_message.clone();
        let msg_color = match self.status_message_ok {
            Some(true) => colors.green,
            Some(false) => colors.red,
            None => colors.muted_foreground,
        };

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
            .justify_between()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .truncate()
                    .child(SharedString::from(self.repo_path.clone())),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when_some(msg, |row, message| {
                        row.child(
                            div()
                                .text_size(px(11.))
                                .text_color(msg_color)
                                .child(SharedString::from(message)),
                        )
                    })
                    .child(
                        div().text_size(px(11.)).text_color(color).child(text),
                    ),
            )
    }
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
            .child(self.main_content(window, cx))
            .child(self.status_bar(cx))
            .when(self.confirmation.is_some(), |element| {
                element.child(self.confirmation_overlay(cx))
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

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

fn operation_result_key(
    action: WorkingTreeAction,
    scope: WorkingTreeScopeKind,
    success: bool,
) -> &'static str {
    match (action, scope, success) {
        (WorkingTreeAction::Stage, WorkingTreeScopeKind::File, true) => {
            "changes-stage-success"
        }
        (WorkingTreeAction::Stage, WorkingTreeScopeKind::All, true) => {
            "changes-stage-all-success"
        }
        (WorkingTreeAction::Unstage, WorkingTreeScopeKind::File, true) => {
            "changes-unstage-success"
        }
        (WorkingTreeAction::Unstage, WorkingTreeScopeKind::All, true) => {
            "changes-unstage-all-success"
        }
        (WorkingTreeAction::Discard, WorkingTreeScopeKind::File, true) => {
            "changes-discard-success"
        }
        (WorkingTreeAction::Discard, WorkingTreeScopeKind::All, true) => {
            "changes-discard-all-success"
        }
        (_, _, false) => "changes-operation-failed",
    }
}
