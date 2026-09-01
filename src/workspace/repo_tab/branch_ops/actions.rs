use gpui::Context;

use super::super::RepoTab;
use super::args::{merge_args, stash_pop_args};
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
        if self.operation_busy || self.stash_count == 0 {
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
        if self.operation_busy {
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
        log::info!(
            "[branch_ops] command queued: {label}, args={args:?} (source={name})"
        );
        self.git_view.update(cx, |view, _| view.run(label, args));
        self.set_operation_busy(true, cx);
    }
}
