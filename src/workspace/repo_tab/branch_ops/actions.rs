use gpui::Context;
use std::path::PathBuf;

use super::super::RepoTab;
use super::args::{merge_args, stash_pop_args};
use crate::core::git::agent_operation::{
    has_other_git_operation_except_rebase, probe_agent_rebase,
    probe_merge_state, probe_rebase_state, resolve_agent_merge_target,
};
use crate::git::toolbar::BranchMenuContext;

impl RepoTab {
    /// Sync Branch menu entry availability to the toolbar. Called after the
    /// status and refs snapshots change.
    pub(in crate::workspace::repo_tab) fn sync_branch_menu_context(
        &self,
        cx: &mut Context<Self>,
    ) {
        let ctx = BranchMenuContext {
            can_rename: !self.branch.is_empty(),
            can_integrate: !self.has_unresolved_conflicts
                && !self.local_branches.is_empty(),
            can_stash: self.local_change_count > 0,
            stash_count: self.stash_count,
            has_conflicts: self.has_unresolved_conflicts,
        };
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_branch_context(ctx, cx);
        });
    }

    /// Execute git stash pop on the worker (guarded by busy and stash
    /// availability).
    pub(in crate::workspace::repo_tab) fn start_stash_pop(
        &mut self,
        stash_ref: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy()
            || self.has_unresolved_conflicts
            || self.stash_count == 0
        {
            return;
        }
        log::info!("[branch_ops] stash pop requested: target={stash_ref:?}");
        let args = stash_pop_args(stash_ref.as_deref());
        self.git_view.update(cx, |view, _| {
            view.run("stash pop", args);
        });
        self.set_operation_busy(true, cx);
    }

    /// Merge a local branch into the current branch without a dialog. The
    /// source branch was picked explicitly in the sidebar context menu.
    pub(in crate::workspace::repo_tab) fn merge_into_current(
        &mut self,
        name: String,
        no_ff: bool,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() || self.has_unresolved_conflicts {
            return;
        }
        if self.branch.is_empty() || name == self.branch {
            log::warn!(
                "[branch_ops] rejected merge into current: source={name}, current={}",
                self.branch
            );
            return;
        }
        self.start_merge_command(name, no_ff, cx);
    }

    /// Execute a normal Git merge while retaining enough context to classify
    /// a conflict if Git leaves `MERGE_HEAD` behind.
    pub(in crate::workspace::repo_tab) fn start_merge_command(
        &mut self,
        name: String,
        no_ff: bool,
        cx: &mut Context<RepoTab>,
    ) {
        if self.is_busy() || self.has_unresolved_conflicts {
            return;
        }
        if self.branch.is_empty() || name == self.branch {
            log::warn!(
                "[branch_ops] rejected merge into current: source={name}, current={}",
                self.branch
            );
            return;
        }
        let (label, args) = merge_args(&name, no_ff);
        self.invalidate_merge_state_probe();
        self.pending_merge_command = Some(super::super::PendingMergeCommand {
            source: name.clone(),
            no_ff,
        });
        log::info!(
            "[branch_ops] command queued: {label}, args={args:?} (source={name})"
        );
        self.git_view.update(cx, |view, _| view.run(label, args));
        self.set_operation_busy(true, cx);
    }

    /// Request a visible Agent session for a merge operation from the sidebar.
    pub(in crate::workspace::repo_tab) fn start_agent_merge(
        &mut self,
        name: String,
        cx: &mut Context<RepoTab>,
    ) {
        if self.is_busy() {
            log::warn!(
                "[agent_terminal] merge request ignored: repository is busy"
            );
            return;
        }
        if self.branch.is_empty() {
            log::warn!(
                "[agent_terminal] merge request ignored: current branch is unavailable"
            );
            return;
        }
        if name == self.branch {
            log::warn!(
                "[agent_terminal] merge request ignored: source is current branch"
            );
            return;
        }
        log::info!(
            "[agent_terminal] merge request accepted: source branch selected"
        );
        cx.emit(super::super::RepoTabEvent::AgentMergeRequested {
            id: self.id,
            repo_path: self.repo_path.clone(),
            source: name,
        });
    }

    /// Handle the result of an ordinary merge command. A failed merge is
    /// inspected asynchronously so the UI can distinguish a real conflict
    /// from validation, hook, or repository errors.
    pub(in crate::workspace::repo_tab) fn handle_merge_result(
        &mut self,
        label: String,
        success: bool,
        detail: String,
        cx: &mut Context<RepoTab>,
    ) {
        if self.merge_abort_pending {
            self.merge_abort_pending = false;
            self.set_operation_busy(false, cx);
            if success {
                self.confirmation = None;
                self.has_unresolved_conflicts = false;
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_conflicts(false, cx);
                });
                self.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_conflicts(false, cx);
                });
                self.status_message = Some(crate::core::i18n::text_args(
                    self.locale,
                    "command-success",
                    &[("label", &label)],
                ));
                self.status_message_ok = Some(true);
                self.refresh_repository(cx);
            } else {
                self.confirmation =
                    Some(super::super::PendingConfirmation::MergeError {
                        label,
                        detail,
                    });
                self.status_message_ok = Some(false);
            }
            cx.notify();
            return;
        }

        let Some(pending) = self.pending_merge_command.take() else {
            self.set_operation_busy(false, cx);
            return;
        };
        let expected_label = if pending.no_ff {
            "merge --no-ff"
        } else {
            "merge"
        };
        if label != expected_label {
            log::warn!(
                "[branch_ops] merge result label mismatch: expected={expected_label}, received={label}"
            );
        }
        if success {
            self.set_operation_busy(false, cx);
            self.has_unresolved_conflicts = false;
            self.sidebar.update(cx, |sidebar, cx| {
                sidebar.set_conflicts(false, cx);
            });
            self.toolbar.update(cx, |toolbar, cx| {
                toolbar.set_conflicts(false, cx);
            });
            self.status_message = Some(crate::core::i18n::text_args(
                self.locale,
                "command-success",
                &[("label", &label)],
            ));
            self.status_message_ok = Some(true);
            self.refresh_repository(cx);
            cx.notify();
            return;
        }

        self.merge_probe_request_id =
            self.merge_probe_request_id.wrapping_add(1).max(1);
        let request_id = self.merge_probe_request_id;
        let source = pending.source;
        let repo_path = PathBuf::from(self.repo_path.clone());
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { probe_merge_state(&repo_path) })
                .await;
            let _ = entity.update(cx, |tab, cx| {
                tab.finish_merge_probe(
                    request_id, label, source, detail, result, cx,
                );
            });
        })
        .detach();
    }

    fn finish_merge_probe(
        &mut self,
        request_id: u64,
        label: String,
        source: String,
        detail: String,
        result: Result<
            crate::core::git::agent_operation::AgentMergeProbe,
            String,
        >,
        cx: &mut Context<RepoTab>,
    ) {
        if request_id != self.merge_probe_request_id {
            return;
        }
        self.set_operation_busy(false, cx);
        match result {
            Ok(probe) => {
                let has_conflicts =
                    probe.has_conflicts || probe.merge_head.is_some();
                self.has_unresolved_conflicts = has_conflicts;
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_conflicts(has_conflicts, cx);
                });
                self.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_conflicts(has_conflicts, cx);
                });
                self.sync_branch_menu_context(cx);
                if let Some(merge_head) = probe.merge_head {
                    self.confirmation = Some(
                        super::super::PendingConfirmation::MergeConflict {
                            source,
                            detail,
                            merge_head,
                            baseline_head: probe.head,
                        },
                    );
                } else {
                    self.confirmation =
                        Some(super::super::PendingConfirmation::MergeError {
                            label,
                            detail,
                        });
                }
            }
            Err(error) => {
                self.confirmation =
                    Some(super::super::PendingConfirmation::MergeError {
                        label,
                        detail: format!("{detail}\n\n{error}"),
                    });
            }
        }
        self.refresh_repository(cx);
        cx.notify();
    }

    /// Request a visible Agent session that rebases the current branch onto a
    /// selected local branch.
    pub(in crate::workspace::repo_tab) fn start_agent_rebase(
        &mut self,
        name: String,
        cx: &mut Context<RepoTab>,
    ) {
        if self.is_busy()
            || self.has_unresolved_conflicts
            || self.branch.is_empty()
            || name == self.branch
        {
            return;
        }
        cx.emit(super::super::RepoTabEvent::AgentRebaseRequested {
            id: self.id,
            repo_path: self.repo_path.clone(),
            source: name,
        });
    }

    /// Start an ordinary rebase after capturing a read-only baseline. The
    /// command itself remains on the Git worker; the short probe runs off the
    /// UI thread so conflict recovery can verify what changed.
    pub(in crate::workspace::repo_tab) fn start_rebase_command(
        &mut self,
        source: String,
        cx: &mut Context<RepoTab>,
    ) {
        if self.is_busy()
            || self.has_unresolved_conflicts
            || self.branch.is_empty()
            || source == self.branch
        {
            return;
        }
        self.queue_rebase_command(
            Some(source),
            "rebase".to_string(),
            false,
            cx,
        );
    }

    /// Start the normal `git pull --rebase` command after capturing its HEAD
    /// baseline. Fetching and remote selection remain Git's responsibility.
    pub(in crate::workspace::repo_tab) fn start_pull_rebase(
        &mut self,
        cx: &mut Context<RepoTab>,
    ) {
        if self.is_busy() || self.has_unresolved_conflicts {
            return;
        }
        self.queue_rebase_command(None, "pull --rebase".to_string(), true, cx);
    }

    fn queue_rebase_command(
        &mut self,
        source: Option<String>,
        label: String,
        pull: bool,
        cx: &mut Context<RepoTab>,
    ) {
        self.invalidate_merge_state_probe();
        self.rebase_probe_request_id =
            self.rebase_probe_request_id.wrapping_add(1).max(1);
        let request_id = self.rebase_probe_request_id;
        self.pending_rebase_command =
            Some(super::super::PendingRebaseCommand {
                source: source.clone(),
                upstream_oid: None,
                baseline_head: None,
                label: label.clone(),
                pull,
            });
        self.set_operation_busy(true, cx);
        let repo_path = PathBuf::from(self.repo_path.clone());
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let upstream_oid = source
                        .as_deref()
                        .map(|branch| {
                            resolve_agent_merge_target(&repo_path, branch)
                        })
                        .transpose()?;
                    let probe = probe_agent_rebase(
                        &repo_path,
                        upstream_oid.as_deref(),
                    )?;
                    let has_other_operation =
                        has_other_git_operation_except_rebase(&repo_path)?;
                    Ok::<_, String>((probe, upstream_oid, has_other_operation))
                })
                .await;
            let _ = entity.update(cx, |tab, cx| {
                tab.finish_rebase_preflight(request_id, result, cx);
            });
        })
        .detach();
    }

    fn finish_rebase_preflight(
        &mut self,
        request_id: u64,
        result: Result<
            (
                crate::core::git::agent_operation::AgentRebaseProbe,
                Option<String>,
                bool,
            ),
            String,
        >,
        cx: &mut Context<RepoTab>,
    ) {
        if request_id != self.rebase_probe_request_id {
            return;
        }
        let Some(mut pending) = self.pending_rebase_command.take() else {
            self.set_operation_busy(false, cx);
            return;
        };
        let (probe, upstream_oid, has_other_operation) = match result {
            Ok(value) => value,
            Err(error) => {
                self.set_operation_busy(false, cx);
                self.status_message = Some(crate::core::i18n::text_args(
                    self.locale,
                    "rebase-preflight-failed",
                    &[("error", &first_line(&error).to_string())],
                ));
                self.status_message_ok = Some(false);
                cx.notify();
                return;
            }
        };
        if has_other_operation
            || probe.rebase_in_progress
            || probe.has_conflicts
        {
            self.set_operation_busy(false, cx);
            self.status_message = Some(crate::core::i18n::text(
                self.locale,
                "rebase-preflight-operation-in-progress",
            ));
            self.status_message_ok = Some(false);
            cx.notify();
            return;
        }
        if !pending.pull && probe.has_changes {
            self.set_operation_busy(false, cx);
            self.status_message = Some(crate::core::i18n::text(
                self.locale,
                "rebase-preflight-dirty",
            ));
            self.status_message_ok = Some(false);
            cx.notify();
            return;
        }
        pending.baseline_head = probe.head;
        pending.upstream_oid = upstream_oid;
        let args = pending
            .source
            .as_deref()
            .map(|source| vec!["rebase".into(), source.to_string()])
            .unwrap_or_else(|| vec!["pull".into(), "--rebase".into()]);
        log::info!(
            "[branch_ops] command queued: {}, source={:?}",
            pending.label,
            pending.source
        );
        self.git_view.update(cx, |view, _| {
            view.run(pending.label.clone(), args);
        });
        self.pending_rebase_command = Some(pending);
        cx.notify();
    }

    /// Handle the result of an ordinary rebase or pull --rebase command.
    /// Failures are probed asynchronously so an in-progress rebase can be
    /// handed to the selected Agent without losing its baseline.
    pub(in crate::workspace::repo_tab) fn handle_rebase_result(
        &mut self,
        label: String,
        success: bool,
        detail: String,
        cx: &mut Context<RepoTab>,
    ) {
        if self.rebase_abort_pending {
            self.rebase_abort_pending = false;
            self.set_operation_busy(false, cx);
            if success {
                self.confirmation = None;
                self.has_unresolved_conflicts = false;
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_conflicts(false, cx);
                });
                self.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_conflicts(false, cx);
                });
                self.status_message = Some(crate::core::i18n::text_args(
                    self.locale,
                    "command-success",
                    &[("label", &label)],
                ));
                self.status_message_ok = Some(true);
                self.refresh_repository(cx);
            } else {
                self.confirmation =
                    Some(super::super::PendingConfirmation::RebaseError {
                        label,
                        detail,
                    });
                self.status_message_ok = Some(false);
            }
            cx.notify();
            return;
        }

        let Some(pending) = self.pending_rebase_command.take() else {
            self.set_operation_busy(false, cx);
            return;
        };
        if success {
            self.set_operation_busy(false, cx);
            self.has_unresolved_conflicts = false;
            self.sidebar.update(cx, |sidebar, cx| {
                sidebar.set_conflicts(false, cx);
            });
            self.toolbar.update(cx, |toolbar, cx| {
                toolbar.set_conflicts(false, cx);
            });
            self.status_message = Some(crate::core::i18n::text_args(
                self.locale,
                "command-success",
                &[("label", &label)],
            ));
            self.status_message_ok = Some(true);
            self.refresh_repository(cx);
            cx.notify();
            return;
        }

        self.rebase_probe_request_id =
            self.rebase_probe_request_id.wrapping_add(1).max(1);
        let request_id = self.rebase_probe_request_id;
        let repo_path = PathBuf::from(self.repo_path.clone());
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { probe_rebase_state(&repo_path) })
                .await;
            let _ = entity.update(cx, |tab, cx| {
                tab.finish_rebase_probe(
                    request_id, label, pending, detail, result, cx,
                );
            });
        })
        .detach();
    }

    fn finish_rebase_probe(
        &mut self,
        request_id: u64,
        label: String,
        pending: super::super::PendingRebaseCommand,
        detail: String,
        result: Result<
            crate::core::git::agent_operation::AgentRebaseProbe,
            String,
        >,
        cx: &mut Context<RepoTab>,
    ) {
        if request_id != self.rebase_probe_request_id {
            return;
        }
        self.set_operation_busy(false, cx);
        match result {
            Ok(probe) => {
                let conflict = probe.rebase_in_progress
                    || probe.rebase_head.is_some()
                    || probe.has_conflicts;
                self.has_unresolved_conflicts = conflict;
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_conflicts(conflict, cx);
                });
                self.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_conflicts(conflict, cx);
                });
                self.sync_branch_menu_context(cx);
                if conflict {
                    let source = pending.source.clone();
                    let upstream_oid = pending.upstream_oid.clone();
                    let baseline_head = pending.baseline_head.clone();
                    self.confirmation = Some(
                        super::super::PendingConfirmation::RebaseConflict {
                            label,
                            source,
                            detail,
                            rebase_head: probe.rebase_head,
                            upstream_oid,
                            baseline_head,
                        },
                    );
                } else {
                    self.confirmation =
                        Some(super::super::PendingConfirmation::RebaseError {
                            label,
                            detail,
                        });
                }
            }
            Err(error) => {
                self.confirmation =
                    Some(super::super::PendingConfirmation::RebaseError {
                        label,
                        detail: format!("{detail}\n\n{error}"),
                    });
            }
        }
        self.refresh_repository(cx);
        cx.notify();
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}
