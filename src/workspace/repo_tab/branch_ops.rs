//! Branch operations launched from the toolbar Branch menu:
//! create/rename a branch, stash/stash pop, merge, and rebase.
//!
//! Dialog state lives on `RepoTab` (`dialogs`). Text inputs are created
//! lazily during render because `InputState` needs a `Window` handle, which
//! the toolbar event subscription does not provide. Every command runs
//! through `GitView::run` on the background Git worker and reports through
//! the shared `CommandDone` event.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};

use super::RepoTab;
use crate::core::i18n::{self, Locale};
use crate::git::shared;
use crate::git::toolbar::BranchMenuContext;

/// Pending branch operation shown as an overlay dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PendingBranchDialog {
    NewBranch,
    Rename,
    Stash,
    Merge { no_ff: bool },
    Rebase,
}

/// Dialog state for the toolbar branch operations. Only one dialog can be
/// open at a time.
#[derive(Default)]
pub(super) struct BranchDialogs {
    pub(super) pending: Option<PendingBranchDialog>,
    text_input: Option<Entity<InputState>>,
    /// Selected source branch for merge/rebase.
    merge_source: Option<String>,
    /// Merge dialog `--no-ff` checkbox state.
    no_ff: bool,
}

impl BranchDialogs {
    /// Close any open dialog. Returns whether one was open.
    pub(super) fn close(&mut self) -> bool {
        if self.pending.take().is_some() {
            self.text_input = None;
            self.merge_source = None;
            self.no_ff = false;
            true
        } else {
            false
        }
    }
}

/// Branch-name validation failure reasons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NameError {
    Empty,
    Invalid,
    Exists,
}

/// Validate a branch name against git-ref rules and the existing local
/// branches (including the current one). Pure so it can be unit tested.
fn validate_branch_name(name: &str, existing: &[String]) -> Option<NameError> {
    if name.is_empty() {
        return Some(NameError::Empty);
    }
    if name.starts_with(['-', '.', '/'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
        || name.chars().any(|c| {
            matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
                || c.is_control()
        })
    {
        return Some(NameError::Invalid);
    }
    if existing.iter().any(|branch| branch == name) {
        return Some(NameError::Exists);
    }
    None
}

impl RepoTab {
    /// Sync Branch menu entry availability to the toolbar. Called after the
    /// status and refs snapshots change.
    pub(super) fn sync_branch_menu_context(&self, cx: &mut Context<Self>) {
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

    /// Open one of the Branch menu dialogs as an overlay (no-op while busy).
    pub(super) fn open_branch_dialog(
        &mut self,
        pending: PendingBranchDialog,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy || self.dialogs.pending.is_some() {
            return;
        }
        if !self.dialog_allowed(&pending) {
            return;
        }
        self.dialogs.no_ff = match &pending {
            PendingBranchDialog::Merge { no_ff } => *no_ff,
            _ => false,
        };
        self.dialogs.merge_source = match &pending {
            PendingBranchDialog::Merge { .. } | PendingBranchDialog::Rebase => {
                self.local_branches.first().cloned()
            }
            _ => None,
        };
        self.dialogs.text_input = None;
        self.dialogs.pending = Some(pending);
        log::info!(
            "[branch_ops] dialog opened: {:?}",
            self.dialogs.pending.as_ref().unwrap()
        );
        cx.notify();
    }

    fn dialog_allowed(&self, pending: &PendingBranchDialog) -> bool {
        match pending {
            PendingBranchDialog::NewBranch => true,
            PendingBranchDialog::Rename => !self.branch.is_empty(),
            PendingBranchDialog::Stash => self.local_change_count > 0,
            PendingBranchDialog::Merge { .. } | PendingBranchDialog::Rebase => {
                !self.local_branches.is_empty()
            }
        }
    }

    /// Render the pending branch dialog, if any. Called from
    /// `RepoTab::render` because lazily creating the text input requires a
    /// `Window` handle.
    pub(super) fn render_branch_dialog(
        tab: &mut RepoTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pending = tab.dialogs.pending.clone()?;
        let colors = cx.theme().colors.clone();
        let locale = tab.locale;
        let this = cx.entity();

        if matches!(
            pending,
            PendingBranchDialog::NewBranch
                | PendingBranchDialog::Rename
                | PendingBranchDialog::Stash
        ) && tab.dialogs.text_input.is_none()
        {
            let prefill = match &pending {
                PendingBranchDialog::Rename => tab.branch.clone(),
                _ => String::new(),
            };
            let state =
                cx.new(|cx| InputState::new(window, cx).default_value(prefill));
            state.update(cx, |input, cx| input.focus(window, cx));
            tab.dialogs.text_input = Some(state);
        }

        let (confirm_enabled, error_text) = match &pending {
            PendingBranchDialog::NewBranch | PendingBranchDialog::Rename => {
                let name = input_value(tab, cx);
                let mut existing = tab.local_branches.clone();
                if !tab.branch.is_empty() {
                    existing.push(tab.branch.clone());
                }
                match validate_branch_name(&name, &existing) {
                    None => (true, None),
                    Some(NameError::Empty) => (false, None),
                    Some(NameError::Invalid) => {
                        (false, Some(i18n::text(locale, "branch-name-invalid")))
                    }
                    Some(NameError::Exists) => (
                        false,
                        Some(i18n::text_args(
                            locale,
                            "branch-name-exists",
                            &[("name", name.as_str())],
                        )),
                    ),
                }
            }
            PendingBranchDialog::Stash => (true, None),
            PendingBranchDialog::Merge { .. } | PendingBranchDialog::Rebase => {
                (tab.dialogs.merge_source.is_some(), None)
            }
        };

        let body: AnyElement = match &pending {
            PendingBranchDialog::NewBranch => {
                let base = if tab.branch.is_empty() {
                    "HEAD".to_string()
                } else {
                    tab.branch.clone()
                };
                named_input_body(
                    &colors,
                    tab.dialogs.text_input.as_ref(),
                    locale,
                    "branch-name-label",
                    "branch-new-hint",
                    &[("branch", &base)],
                    error_text,
                )
            }
            PendingBranchDialog::Rename => named_input_body(
                &colors,
                tab.dialogs.text_input.as_ref(),
                locale,
                "branch-name-label",
                "branch-rename-hint",
                &[("branch", &tab.branch)],
                error_text,
            ),
            PendingBranchDialog::Stash => {
                let count = tab.local_change_count.to_string();
                named_input_body(
                    &colors,
                    tab.dialogs.text_input.as_ref(),
                    locale,
                    "stash-message-label",
                    "stash-hint",
                    &[("count", &count)],
                    None,
                )
            }
            PendingBranchDialog::Merge { .. } | PendingBranchDialog::Rebase => {
                let mut body = v_flex().w_full().gap_2();
                if pending == PendingBranchDialog::Rebase {
                    body = body.child(
                        div().text_size(px(12.)).text_color(colors.red).child(
                            shared(i18n::text_args(
                                locale,
                                "rebase-warning",
                                &[("branch", &tab.branch)],
                            )),
                        ),
                    );
                }
                body = body.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(colors.muted_foreground)
                                .child(shared(i18n::text(
                                    locale,
                                    "merge-source-label",
                                ))),
                        )
                        .child(source_selector(tab, locale, &this)),
                );
                if matches!(pending, PendingBranchDialog::Merge { .. }) {
                    body = body.child(merge_no_ff_checkbox(locale, &this, tab));
                }
                body.into_any_element()
            }
        };

        let title_icon = match &pending {
            PendingBranchDialog::NewBranch => {
                crate::git::lucide("git-branch-plus")
            }
            PendingBranchDialog::Rename => crate::git::lucide("pencil"),
            PendingBranchDialog::Stash => crate::git::lucide("archive"),
            PendingBranchDialog::Merge { .. } => {
                crate::git::lucide("git-merge")
            }
            PendingBranchDialog::Rebase => {
                crate::git::lucide("git-commit-horizontal")
            }
        };
        let title_text = match &pending {
            PendingBranchDialog::NewBranch => {
                i18n::text(locale, "branch-new-title")
            }
            PendingBranchDialog::Rename => {
                i18n::text(locale, "branch-rename-title")
            }
            PendingBranchDialog::Stash => i18n::text(locale, "stash-title"),
            PendingBranchDialog::Merge { .. } => i18n::text_args(
                locale,
                "merge-title",
                &[("branch", &tab.branch)],
            ),
            PendingBranchDialog::Rebase => i18n::text_args(
                locale,
                "rebase-title",
                &[("branch", &tab.branch)],
            ),
        };

        let title_row = h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(px(16.))
                    .text_color(colors.muted_foreground)
                    .child(title_icon),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child(shared(title_text)),
            );

        let cancel_btn = {
            let this = this.clone();
            Button::new("branch-dialog-cancel")
                .label(i18n::text(locale, "dialog-cancel"))
                .ghost()
                .flex_1()
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| tab.cancel_topmost(cx));
                })
        };
        let confirm_btn = {
            let this = this.clone();
            let mut btn = Button::new("branch-dialog-confirm")
                .label(i18n::text(locale, "dialog-confirm"))
                .flex_1();
            btn = if pending == PendingBranchDialog::Rebase {
                btn.danger()
            } else {
                btn.primary()
            };
            btn.when(confirm_enabled, |btn| {
                btn.on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| confirm_branch_dialog(tab, cx));
                })
            })
            .when(!confirm_enabled, |btn| btn.disabled(true))
        };

        let buttons = h_flex()
            .w_full()
            .gap_2()
            .child(cancel_btn)
            .child(confirm_btn);

        Some(
            tab.overlay_card(
                cx,
                "branch-dialog-overlay",
                "branch-dialog-card",
                title_row,
                body,
                buttons,
            )
            .into_any_element(),
        )
    }

    /// Execute `git stash pop` on the worker (guarded by busy and stash
    /// availability).
    pub(super) fn start_stash_pop(&mut self, cx: &mut Context<Self>) {
        if self.operation_busy || self.stash_count == 0 {
            return;
        }
        log::info!("[branch_ops] stash pop requested");
        self.git_view.update(cx, |view, _| {
            view.run("stash pop", vec!["stash".into(), "pop".into()]);
        });
        self.set_operation_busy(true, cx);
    }
}

/// Read the current value of the dialog text input, if any.
fn input_value(tab: &RepoTab, cx: &Context<RepoTab>) -> String {
    tab.dialogs
        .text_input
        .as_ref()
        .map_or(String::new(), |input| input.read(cx).value().to_string())
}

/// Labeled text input plus a hint line and an optional inline error.
fn named_input_body(
    colors: &gpui_component::theme::ThemeColor,
    input: Option<&Entity<InputState>>,
    locale: Locale,
    label_key: &'static str,
    hint_key: &'static str,
    hint_args: &[(&str, &str)],
    error_text: Option<String>,
) -> AnyElement {
    let mut body = v_flex().w_full().gap_2().child(
        div()
            .text_size(px(12.))
            .text_color(colors.muted_foreground)
            .child(shared(i18n::text(locale, label_key))),
    );
    if let Some(input) = input {
        body = body.child(div().w_full().child(Input::new(input)));
    }
    body = body.child(
        div()
            .text_size(px(12.))
            .text_color(colors.muted_foreground)
            .child(shared(i18n::text_args(locale, hint_key, hint_args))),
    );
    if let Some(error) = error_text {
        body = body.child(
            div()
                .text_size(px(12.))
                .text_color(colors.red)
                .child(shared(error)),
        );
    }
    body.into_any_element()
}

/// Source branch dropdown used by the merge and rebase dialogs.
fn source_selector(
    tab: &RepoTab,
    locale: Locale,
    this: &Entity<RepoTab>,
) -> impl IntoElement {
    let selected = tab.dialogs.merge_source.clone();
    let label = selected
        .clone()
        .unwrap_or_else(|| i18n::text(locale, "merge-source-label"));
    let branches = tab.local_branches.clone();
    let menu_entity = this.clone();

    Button::new("branch-dialog-source")
        .ghost()
        .small()
        .label(label)
        .icon(IconName::ChevronDown)
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
            let menu_entity = menu_entity.clone();
            let selected = selected.clone();
            branches.iter().fold(menu, |menu, name| {
                let name = name.clone();
                let item_entity = menu_entity.clone();
                menu.item(
                    PopupMenuItem::new(name.clone())
                        .checked(selected.as_deref() == Some(name.as_str()))
                        .on_click(move |_event, _window, cx| {
                            item_entity.update(cx, |tab, cx| {
                                tab.dialogs.merge_source = Some(name.clone());
                                cx.notify();
                            });
                        }),
                )
            })
        })
}

/// `--no-ff` checkbox for the merge dialog.
fn merge_no_ff_checkbox(
    locale: Locale,
    this: &Entity<RepoTab>,
    tab: &RepoTab,
) -> Checkbox {
    let checked = tab.dialogs.no_ff;
    let entity = this.clone();
    Checkbox::new("merge-no-ff")
        .label(i18n::text(locale, "merge-no-ff-label"))
        .checked(checked)
        .on_click(move |checked: &bool, _window, cx| {
            entity.update(cx, |tab, cx| {
                tab.dialogs.no_ff = *checked;
                cx.notify();
            });
        })
}

/// Re-validate at confirm time and close the dialog before dispatching the
/// command to the Git worker.
fn confirm_branch_dialog(tab: &mut RepoTab, cx: &mut Context<RepoTab>) {
    let Some(pending) = tab.dialogs.pending.clone() else {
        return;
    };
    let (label, args) = match pending {
        PendingBranchDialog::NewBranch => {
            let Some(name) = validated_name(tab, cx) else {
                return;
            };
            ("switch", vec!["switch".into(), "-c".into(), name])
        }
        PendingBranchDialog::Rename => {
            let Some(name) = validated_name(tab, cx) else {
                return;
            };
            ("branch -m", vec!["branch".into(), "-m".into(), name])
        }
        PendingBranchDialog::Stash => {
            let message = input_value(tab, cx);
            let mut args = vec!["stash".into(), "push".into()];
            if !message.is_empty() {
                args.push("-m".into());
                args.push(message);
            }
            ("stash", args)
        }
        PendingBranchDialog::Merge { no_ff } => {
            let Some(source) = tab.dialogs.merge_source.clone() else {
                return;
            };
            let label = if no_ff { "merge --no-ff" } else { "merge" };
            let mut args = vec!["merge".into(), source];
            if no_ff {
                args.push("--no-ff".into());
            }
            (label, args)
        }
        PendingBranchDialog::Rebase => {
            let Some(source) = tab.dialogs.merge_source.clone() else {
                return;
            };
            ("rebase", vec!["rebase".into(), source])
        }
    };
    tab.dialogs.close();
    log::info!("[branch_ops] command queued: {label}");
    tab.git_view.update(cx, |view, _| view.run(label, args));
    tab.set_operation_busy(true, cx);
    cx.notify();
}

/// Confirm-time validation for name dialogs.
fn validated_name(tab: &RepoTab, cx: &Context<RepoTab>) -> Option<String> {
    let name = input_value(tab, cx);
    let mut existing = tab.local_branches.clone();
    if !tab.branch.is_empty() {
        existing.push(tab.branch.clone());
    }
    match validate_branch_name(&name, &existing) {
        None => Some(name),
        Some(error) => {
            log::warn!("[branch_ops] rejected branch name: {error:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NameError, validate_branch_name};

    fn existing(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn accepts_common_branch_names() {
        let refs = existing(&["main", "feature/one"]);
        assert_eq!(validate_branch_name("dev", &refs), None);
        assert_eq!(validate_branch_name("feature/two", &refs), None);
        assert_eq!(validate_branch_name("v1.2.3", &refs), None);
        assert_eq!(validate_branch_name("fix_bug-1", &refs), None);
        assert_eq!(validate_branch_name("topic+.patch", &refs), None);
    }

    #[test]
    fn rejects_empty_names() {
        assert_eq!(validate_branch_name("", &[]), Some(NameError::Empty));
    }

    #[test]
    fn rejects_invalid_ref_syntax() {
        for name in [
            "-dev", ".hidden", "a..b", "a b", "a~b", "a^b", "a:b", "a?b",
            "a*b", "a[b", "a\\b", "a@{b", "a.lock", "a/", "a.", "/a", "a//b",
        ] {
            assert_eq!(
                validate_branch_name(name, &[]),
                Some(NameError::Invalid),
                "expected {name:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_existing_branches() {
        let refs = existing(&["main", "feature/one"]);
        assert_eq!(
            validate_branch_name("main", &refs),
            Some(NameError::Exists)
        );
        assert_eq!(
            validate_branch_name("feature/one", &refs),
            Some(NameError::Exists)
        );
    }
}
