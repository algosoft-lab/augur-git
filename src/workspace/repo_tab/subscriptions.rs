//! Event wiring between a repository tab's panels, the Git worker, and the
//! shared workspace state. Each `wire_*` function owns the `cx.subscribe`
//! channel for one panel; `RepoTab::new` only calls [`wire`].

use gpui::{Context, Entity, Window};

use crate::core::git::{
    WorkingTreeAction, WorkingTreeDiffKind, WorkingTreeScopeKind,
};
use crate::core::i18n;
use crate::git::changes_panel::{ChangesPanel, ChangesPanelEvent};
use crate::git::graph::{GraphEvent, GraphView};
use crate::git::panel::{
    BottomPanel, BottomPanelEvent, CommitAction, CommitPanel, CommitPanelEvent,
};
use crate::git::sidebar::Sidebar;
use crate::git::toolbar::{Toolbar, ToolbarEvent};
use crate::git::{GitStatus, GitUiEvent, GitView};

use super::dialogs::push_error_missing_upstream;
use super::{PendingConfirmation, RepoTab, RepoTabEvent};
use super::{branch_compare, branch_ops};

/// Attach every panel subscription of a freshly created `RepoTab`.
pub(super) fn wire(
    git_view: &Entity<GitView>,
    sidebar: &Entity<Sidebar>,
    toolbar: &Entity<Toolbar>,
    graph: &Entity<GraphView>,
    commit: &Entity<CommitPanel>,
    changes: &Entity<ChangesPanel>,
    bottom: &Entity<BottomPanel>,
    window: &mut Window,
    cx: &mut Context<RepoTab>,
) {
    wire_sidebar(sidebar, cx);
    wire_toolbar(toolbar, window, cx);
    wire_graph(graph, cx);
    wire_commit(commit, cx);
    wire_changes(changes, cx);
    wire_bottom(bottom, cx);
    wire_git_view(git_view, cx);
}

fn wire_sidebar(sidebar: &Entity<Sidebar>, cx: &mut Context<RepoTab>) {
    cx.subscribe(sidebar, |tab, _event, event, cx| {
        branch_ops::handle_sidebar_event(tab, event, cx);
    })
    .detach();
}

fn wire_toolbar(
    toolbar: &Entity<Toolbar>,
    window: &mut Window,
    cx: &mut Context<RepoTab>,
) {
    cx.subscribe_in(toolbar, window, |tab, _event, event, _window, cx| {
        match event {
            ToolbarEvent::Fetch => {
                if tab.is_busy() {
                    return;
                }
                tab.git_view.update(cx, |view, _| {
                    view.run(
                        "fetch --all --prune",
                        vec!["fetch".into(), "--all".into(), "--prune".into()],
                    );
                });
                tab.set_operation_busy(true, cx);
            }
            ToolbarEvent::PullMerge => {
                if tab.is_busy() || tab.has_unresolved_conflicts {
                    return;
                }
                tab.git_view.update(cx, |view, _| {
                    view.run("pull", vec!["pull".into()]);
                });
                tab.set_operation_busy(true, cx);
            }
            ToolbarEvent::PullRebase => {
                if tab.is_busy() || tab.has_unresolved_conflicts {
                    return;
                }
                tab.start_pull_rebase(cx);
            }
            ToolbarEvent::Push => {
                if tab.is_busy() {
                    return;
                }
                // Publish a branch that has no upstream through a confirmed
                // `--set-upstream` push instead of letting plain push fail.
                if tab.request_push_upstream(cx) {
                    return;
                }
                tab.git_view.update(cx, |view, _| {
                    view.run("push", vec!["push".into()]);
                });
                tab.set_operation_busy(true, cx);
            }
            ToolbarEvent::PushForce => {
                // Never run directly: open the confirmation dialog first.
                if !tab.is_busy() {
                    tab.confirmation = Some(PendingConfirmation::ForcePush);
                    cx.notify();
                }
            }
            ToolbarEvent::BranchNew => {
                tab.open_branch_dialog(
                    branch_ops::PendingBranchDialog::NewBranch,
                    cx,
                );
            }
            ToolbarEvent::BranchRename => tab.open_branch_dialog(
                branch_ops::PendingBranchDialog::Rename {
                    old: tab.branch.clone(),
                },
                cx,
            ),
            ToolbarEvent::Stash => {
                tab.open_branch_dialog(
                    branch_ops::PendingBranchDialog::Stash,
                    cx,
                );
            }
            ToolbarEvent::StashPop => {
                tab.start_stash_pop(None, cx);
            }
            ToolbarEvent::Merge { no_ff } => {
                tab.open_branch_dialog(
                    branch_ops::PendingBranchDialog::Merge { no_ff: *no_ff },
                    cx,
                );
            }
            ToolbarEvent::Rebase => {
                tab.open_branch_dialog(
                    branch_ops::PendingBranchDialog::Rebase,
                    cx,
                );
            }
            ToolbarEvent::Compare => branch_compare::open(tab, cx),
            ToolbarEvent::Refresh => {
                if !tab.is_busy() {
                    tab.refresh_repository(cx);
                }
            }
            ToolbarEvent::Settings => {
                cx.emit(RepoTabEvent::RequestSettings);
            }
        }
    })
    .detach();
}

fn wire_graph(graph: &Entity<GraphView>, cx: &mut Context<RepoTab>) {
    cx.subscribe(graph, |tab, _event, event, cx| match event {
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
        GraphEvent::SelectionCleared => {
            tab.bottom.update(cx, |bottom, cx| {
                bottom.clear_commit(cx);
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
        GraphEvent::MoreLogPageRequested => {
            log::debug!("[git_view] more log page requested");
            tab.git_view.update(cx, |view, _| {
                view.request_more_log_page();
            });
        }
    })
    .detach();
}

fn wire_commit(commit: &Entity<CommitPanel>, cx: &mut Context<RepoTab>) {
    cx.subscribe(commit, |tab, _event, event, cx| match event {
        CommitPanelEvent::Submit { message, action } => {
            if tab.is_busy() {
                return;
            }
            match action {
                CommitAction::Commit => {
                    log::info!(
                        "[commit_panel] submit requested: action=commit"
                    );
                    tab.git_view.update(cx, |view, _| {
                        view.commit(message.clone(), false);
                    });
                    tab.set_operation_busy(true, cx);
                }
                CommitAction::Amend => {
                    log::info!("[commit_panel] submit requested: action=amend");
                    tab.git_view.update(cx, |view, _| {
                        view.commit(message.clone(), true);
                    });
                    tab.set_operation_busy(true, cx);
                }
                CommitAction::CommitByAgent => {
                    log::info!("[commit_panel] submit requested: action=agent");
                    cx.emit(RepoTabEvent::AgentCommitRequested {
                        id: tab.id,
                        repo_path: tab.repo_path.clone(),
                        hint: message.clone(),
                    });
                }
            }
        }
    })
    .detach();
}

fn wire_changes(changes: &Entity<ChangesPanel>, cx: &mut Context<RepoTab>) {
    cx.subscribe(changes, |tab, _event, event, cx| match event {
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
                tab.start_working_tree_operation(*action, scope.clone(), cx);
            }
        }
    })
    .detach();
}

fn wire_bottom(bottom: &Entity<BottomPanel>, cx: &mut Context<RepoTab>) {
    cx.subscribe(bottom, |tab, _event, event, cx| match event {
        BottomPanelEvent::ShowFileDiff {
            oid,
            merge_parent,
            file,
        } => {
            tab.git_view.update(cx, |view, _| {
                view.file_diff(oid.clone(), merge_parent.clone(), file.clone());
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
}

fn wire_git_view(git_view: &Entity<GitView>, cx: &mut Context<RepoTab>) {
    cx.subscribe(git_view, |tab, _event, event, cx| {
        if branch_compare::handle_git_event(tab, event, cx) {
            return;
        }
        match event {
            GitUiEvent::StatusChanged {
                branch,
                head,
                upstream,
                ahead,
                behind,
                files,
                branches,
            } => {
                let branch_name = branch.clone();
                let has_staged =
                    files.iter().any(|file| file.has_staged_changes());
                let staged_count = files
                    .iter()
                    .filter(|file| file.has_staged_changes())
                    .count();
                let unstaged_count = files
                    .iter()
                    .filter(|file| {
                        file.is_conflicted() || file.has_worktree_changes()
                    })
                    .count();
                let has_unresolved_conflicts =
                    files.iter().any(|file| file.is_conflicted());
                let ahead_text = ahead.to_string();
                let behind_text = behind.to_string();
                let staged_text = staged_count.to_string();
                let unstaged_text = unstaged_count.to_string();

                tab.branch = branch_name;
                tab.head = head.clone();
                tab.upstream = upstream.clone();
                tab.ahead = *ahead;
                tab.behind = *behind;
                tab.local_branches = branches
                    .iter()
                    .filter(|info| !info.is_head)
                    .map(|info| info.name.clone())
                    .collect();
                tab.local_change_count = files
                    .iter()
                    .filter(|file| {
                        file.has_staged_changes()
                            || file.has_worktree_changes()
                            || file.is_conflicted()
                    })
                    .count();
                let had_conflict_guard = tab.has_unresolved_conflicts;
                // Keep an existing guard until the asynchronous probe confirms
                // that MERGE_HEAD is gone. A resolved index can have no `U`
                // entries while Git is still waiting for the merge commit.
                tab.has_unresolved_conflicts = has_unresolved_conflicts
                    || had_conflict_guard;
                if has_unresolved_conflicts
                    || had_conflict_guard
                    || tab.merge_state_probe_request_id == 0
                {
                    tab.schedule_merge_state_probe(cx);
                }
                tab.sync_log_scope(cx);
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
                    sidebar.set_conflicts(
                        tab.has_unresolved_conflicts,
                        cx,
                    );
                });
                tab.changes.update(cx, |changes, cx| {
                    changes.set_files(files.clone(), cx);
                });
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.sync_working_tree_files(files, cx);
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_ahead_behind(*ahead, *behind, cx);
                    toolbar.set_conflicts(tab.has_unresolved_conflicts, cx);
                });
                tab.sync_branch_menu_context(cx);
                tab.commit.update(cx, |commit, cx| {
                    commit.set_has_staged(has_staged, cx);
                    commit.set_has_changes(tab.local_change_count > 0, cx);
                });
                tab.emit_summary(cx);
                cx.notify();
            }
            GitUiEvent::LogPageChanged { rows, replace, has_more } => {
                tab.graph.update(cx, |graph, cx| {
                    graph.set_log_page(rows.clone(), *replace, *has_more, cx);
                });
                // Keep the comparison selector fed with loaded commits.
                let loaded = tab.graph.read(cx).log_rows();
                tab.compare.update(cx, |view, cx| {
                    view.set_log_rows(loaded, cx);
                });
            }
            GitUiEvent::RefsChanged(refs) => {
                tab.stash_count = refs.stashes.len();
                tab.remotes = refs.remotes.clone();
                tab.sync_branch_menu_context(cx);
                tab.graph.update(cx, |graph, cx| {
                    graph.set_remote_names(refs.remotes.clone(), cx);
                });
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
                let label_key =
                    operation_result_key(*action, *scope, *success);
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
                if label == "merge"
                    || label == "merge --no-ff"
                    || label == "merge --abort"
                {
                    tab.handle_merge_result(
                        label.clone(),
                        *success,
                        message.clone(),
                        cx,
                    );
                    return;
                }
                if label == "rebase"
                    || label == "pull --rebase"
                    || label == "rebase --abort"
                {
                    tab.handle_rebase_result(
                        label.clone(),
                        *success,
                        message.clone(),
                        cx,
                    );
                    return;
                }
                if label == "checkout" {
                    log::info!(
                        "[git_checkout] result received: success={success}"
                    );
                }
                let copy_commit_message = label == "copy-commit-message";
                tab.set_operation_busy(false, cx);
                // A stale status snapshot can hide a missing upstream, so a
                // plain push may still fail that way; offer the same dialog.
                if !*success
                    && label == "push"
                    && push_error_missing_upstream(message)
                    && tab.request_push_upstream(cx)
                {
                    return;
                }
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
                            | "fetch --all --prune"
                            | "pull"
                            | "pull --rebase"
                            | "push"
                            | "push --force"
                            | "push --set-upstream"
                            | "push --rename"
                            | "push --delete"
                            | "switch"
                            | "branch -m"
                            | "branch -d"
                            | "branch -D"
                            | "tag -d"
                            | "stash"
                            | "stash pop"
                            | "stash drop"
                            | "merge"
                            | "merge --no-ff"
                            | "rebase"
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
                if tab.pending_merge_command.is_some()
                    || tab.merge_abort_pending
                {
                    let label = if tab.merge_abort_pending {
                        "merge --abort".to_string()
                    } else if tab
                        .pending_merge_command
                        .as_ref()
                        .is_some_and(|pending| pending.no_ff)
                    {
                        "merge --no-ff".to_string()
                    } else {
                        "merge".to_string()
                    };
                    tab.handle_merge_result(
                        label,
                        false,
                        message.clone(),
                        cx,
                    );
                    return;
                }
                if tab.pending_rebase_command.is_some()
                    || tab.rebase_abort_pending
                {
                    let label = if tab.rebase_abort_pending {
                        "rebase --abort".to_string()
                    } else {
                        tab.pending_rebase_command
                            .as_ref()
                            .map(|pending| pending.label.clone())
                            .unwrap_or_else(|| "rebase".to_string())
                    };
                    tab.handle_rebase_result(
                        label,
                        false,
                        message.clone(),
                        cx,
                    );
                    return;
                }
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
            _ => {}
        }
    })
    .detach();
}

/// First line of a multi-line Git message, for one-line status text.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// i18n key describing a completed working-tree operation.
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
