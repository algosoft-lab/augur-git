use gpui::Entity;
use gpui_component::input::InputState;

/// Pending branch operation shown as an overlay dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::workspace::repo_tab) enum PendingBranchDialog {
    NewBranch,
    /// Rename a local branch; the payload is its current name.
    Rename {
        old: String,
    },
    Stash,
    /// Drop a stash after explicit confirmation; the payload is its selector.
    DropStash {
        reference: String,
    },
    Merge {
        no_ff: bool,
    },
    Rebase,
    /// Delete a local branch or a tag after confirmation.
    DeleteRef {
        name: String,
        is_tag: bool,
    },
    /// Rename a remote branch on its remote after confirmation; `old` is
    /// the current branch name without the remote prefix.
    RenameRemote {
        remote: String,
        old: String,
    },
    /// Delete a remote branch on its remote after confirmation.
    DeleteRemote {
        remote: String,
        branch: String,
    },
}

/// Dialog state for the branch operations. Only one dialog can be open at
/// a time.
#[derive(Default)]
pub(in crate::workspace::repo_tab) struct BranchDialogs {
    pub(in crate::workspace::repo_tab) pending: Option<PendingBranchDialog>,
    pub(in crate::workspace::repo_tab) text_input: Option<Entity<InputState>>,
    /// Selected source branch for merge/rebase.
    pub(in crate::workspace::repo_tab) merge_source: Option<String>,
    /// Merge dialog `--no-ff` checkbox state.
    pub(in crate::workspace::repo_tab) no_ff: bool,
    /// Delete dialog "force delete" checkbox state (local branches only).
    pub(in crate::workspace::repo_tab) force_delete: bool,
}

impl BranchDialogs {
    /// Close any open dialog. Returns whether one was open.
    pub(in crate::workspace::repo_tab) fn close(&mut self) -> bool {
        if self.pending.take().is_some() {
            self.text_input = None;
            self.merge_source = None;
            self.no_ff = false;
            self.force_delete = false;
            true
        } else {
            false
        }
    }
}
