//! Branch operations launched from the toolbar Branch menu and the sidebar
//! context menus.
//!
//! The implementation is split by responsibility:
//! - state stores the pending-dialog state;
//! - args contains pure Git argument builders and branch-name validation;
//! - dialog owns branch-operation dialog rendering and confirmation;
//! - actions owns direct operation dispatch and toolbar synchronization.

mod actions;
mod args;
mod dialog;
mod state;

use super::RepoTab;
use gpui::Context;

use crate::core::i18n;
use crate::git::sidebar::SidebarEvent;

pub(super) use state::{BranchDialogs, PendingBranchDialog};

/// Route sidebar context-menu events to the branch operation handlers.
pub(super) fn handle_sidebar_event(
    tab: &mut RepoTab,
    event: &SidebarEvent,
    cx: &mut Context<RepoTab>,
) {
    match event {
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
        SidebarEvent::PopStash(stash_ref) => {
            tab.start_stash_pop(Some(stash_ref.clone()), cx);
        }
        SidebarEvent::DropStash(stash_ref) => {
            tab.open_branch_dialog(
                PendingBranchDialog::DropStash {
                    reference: stash_ref.clone(),
                },
                cx,
            );
        }
        SidebarEvent::RenameBranch(name) => {
            tab.open_branch_dialog(
                PendingBranchDialog::Rename { old: name.clone() },
                cx,
            );
        }
        SidebarEvent::DeleteBranch(name) => {
            tab.open_branch_dialog(
                PendingBranchDialog::DeleteRef {
                    name: name.clone(),
                    is_tag: false,
                },
                cx,
            );
        }
        SidebarEvent::DeleteTag(name) => {
            tab.open_branch_dialog(
                PendingBranchDialog::DeleteRef {
                    name: name.clone(),
                    is_tag: true,
                },
                cx,
            );
        }
        SidebarEvent::MergeIntoCurrent { name, no_ff } => {
            tab.merge_into_current(name.clone(), *no_ff, cx);
        }
        SidebarEvent::MergeByAgent(name) => {
            tab.start_agent_merge(name.clone(), cx);
        }
        SidebarEvent::RenameRemoteBranch { remote, branch } => {
            tab.open_branch_dialog(
                PendingBranchDialog::RenameRemote {
                    remote: remote.clone(),
                    old: branch.clone(),
                },
                cx,
            );
        }
        SidebarEvent::DeleteRemoteBranch { remote, branch } => {
            tab.open_branch_dialog(
                PendingBranchDialog::DeleteRemote {
                    remote: remote.clone(),
                    branch: branch.clone(),
                },
                cx,
            );
        }
    }
}
