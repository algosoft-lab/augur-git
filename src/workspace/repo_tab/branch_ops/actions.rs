use gpui::Context;
use std::path::PathBuf;

use super::super::RepoTab;
use super::args::{merge_args, stash_pop_args};
use crate::core::git::agent_operation::probe_merge_state;
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
            can_integrate: !self.local_branches.is_empty(),
            can_stash: self.local_change_count > 0,
            stash_count: self.stash_count,
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
        if self.is_busy() || self.stash_count == 0 {
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
        if self.is_busy() {
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
        if self.is_busy() {
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
        if self.is_busy() || self.branch.is_empty() || name == self.branch {
            return;
        }
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
        if success {
            self.set_operation_busy(false, cx);
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
            Ok(probe) if probe.merge_head.is_some() => {
                self.confirmation =
                    Some(super::super::PendingConfirmation::MergeConflict {
                        source,
                        detail,
                        merge_head: probe.merge_head.unwrap_or_default(),
                    });
            }
            Ok(_) => {
                self.confirmation =
                    Some(super::super::PendingConfirmation::MergeError {
                        label,
                        detail,
                    });
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
}
