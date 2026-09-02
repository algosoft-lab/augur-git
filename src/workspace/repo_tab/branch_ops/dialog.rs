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

use super::super::RepoTab;
use super::{PendingBranchDialog, args};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

impl RepoTab {
    /// Open one of the Branch menu dialogs as an overlay (no-op while busy).
    pub(in crate::workspace::repo_tab) fn open_branch_dialog(
        &mut self,
        pending: PendingBranchDialog,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() || self.dialogs.pending.is_some() {
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
            PendingBranchDialog::NewBranch => !self.has_unresolved_conflicts,
            PendingBranchDialog::Rename { old } => !old.is_empty(),
            PendingBranchDialog::Stash => self.local_change_count > 0,
            PendingBranchDialog::DropStash { reference } => {
                !reference.is_empty() && self.stash_count > 0
            }
            PendingBranchDialog::Merge { .. } | PendingBranchDialog::Rebase => {
                !self.has_unresolved_conflicts
                    && !self.local_branches.is_empty()
            }
            PendingBranchDialog::DeleteRef { name, is_tag } => {
                // The current branch can never be deleted.
                *is_tag || *name != self.branch
            }
            PendingBranchDialog::RenameRemote { remote, old } => {
                !remote.is_empty() && !old.is_empty()
            }
            PendingBranchDialog::DeleteRemote { remote, branch } => {
                !remote.is_empty() && !branch.is_empty()
            }
        }
    }

    /// Render the pending branch dialog, if any. Called from
    /// RepoTab::render because lazily creating the text input requires a
    /// Window handle.
    pub(in crate::workspace::repo_tab) fn render_branch_dialog(
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
                | PendingBranchDialog::Rename { .. }
                | PendingBranchDialog::RenameRemote { .. }
                | PendingBranchDialog::Stash
        ) && tab.dialogs.text_input.is_none()
        {
            let prefill = match &pending {
                PendingBranchDialog::Rename { old } => old.clone(),
                PendingBranchDialog::RenameRemote { old, .. } => old.clone(),
                _ => String::new(),
            };
            let state =
                cx.new(|cx| InputState::new(window, cx).default_value(prefill));
            state.update(cx, |input, cx| input.focus(window, cx));
            tab.dialogs.text_input = Some(state);
        }

        let (confirm_enabled, error_text) = match &pending {
            PendingBranchDialog::NewBranch
            | PendingBranchDialog::Rename { .. } => {
                let name = input_value(tab, cx);
                let allow = match &pending {
                    PendingBranchDialog::Rename { old } => Some(old.as_str()),
                    _ => None,
                };
                let mut existing = tab.local_branches.clone();
                if !tab.branch.is_empty() {
                    existing.push(tab.branch.clone());
                }
                match args::validate_branch_name(&name, &existing, allow) {
                    None => (true, None),
                    Some(args::NameError::Empty) => (false, None),
                    Some(args::NameError::Invalid) => {
                        (false, Some(i18n::text(locale, "branch-name-invalid")))
                    }
                    Some(args::NameError::Exists) => (
                        false,
                        Some(i18n::text_args(
                            locale,
                            "branch-name-exists",
                            &[("name", name.as_str())],
                        )),
                    ),
                }
            }
            PendingBranchDialog::RenameRemote { .. } => {
                // The new name is a remote branch name: only git-ref syntax
                // applies here; an existing remote branch is rejected by the
                // remote itself when the push runs.
                let name = input_value(tab, cx);
                match args::validate_branch_name(&name, &[], None) {
                    None => (true, None),
                    Some(args::NameError::Empty) => (false, None),
                    Some(_) => {
                        (false, Some(i18n::text(locale, "branch-name-invalid")))
                    }
                }
            }
            PendingBranchDialog::Stash => (true, None),
            PendingBranchDialog::DropStash { .. } => (true, None),
            PendingBranchDialog::Merge { .. } | PendingBranchDialog::Rebase => {
                (tab.dialogs.merge_source.is_some(), None)
            }
            PendingBranchDialog::DeleteRef { .. } => (true, None),
            PendingBranchDialog::DeleteRemote { .. } => (true, None),
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
            PendingBranchDialog::Rename { old } => named_input_body(
                &colors,
                tab.dialogs.text_input.as_ref(),
                locale,
                "branch-name-label",
                "branch-rename-hint",
                &[("branch", old)],
                error_text,
            ),
            PendingBranchDialog::RenameRemote { remote, old } => {
                named_input_body(
                    &colors,
                    tab.dialogs.text_input.as_ref(),
                    locale,
                    "branch-name-label",
                    "rename-remote-branch-hint",
                    &[("remote", remote), ("branch", old)],
                    error_text,
                )
            }
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
                        div()
                            .text_size(crate::theme::scaled_text_size(12.))
                            .text_color(colors.red)
                            .child(shared(i18n::text_args(
                                locale,
                                "rebase-warning",
                                &[("branch", &tab.branch)],
                            ))),
                    );
                }
                body = body.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(crate::theme::scaled_text_size(12.))
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
            PendingBranchDialog::DeleteRef { name, is_tag } => {
                let warning_key = if *is_tag {
                    "delete-tag-warning"
                } else {
                    "delete-branch-warning"
                };
                let mut body = v_flex().w_full().gap_2().child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text_args(
                            locale,
                            warning_key,
                            &[("name", name)],
                        ))),
                );
                if !is_tag {
                    body =
                        body.child(delete_force_checkbox(locale, &this, tab));
                }
                body.into_any_element()
            }
            PendingBranchDialog::DeleteRemote { remote, branch } => v_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text_args(
                            locale,
                            "delete-remote-branch-warning",
                            &[("remote", remote), ("branch", branch)],
                        ))),
                )
                .into_any_element(),
            PendingBranchDialog::DropStash { reference } => v_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text_args(
                            locale,
                            "stash-drop-warning",
                            &[("reference", reference)],
                        ))),
                )
                .into_any_element(),
        };

        let title_icon = match &pending {
            PendingBranchDialog::NewBranch => {
                crate::git::lucide("git-branch-plus")
            }
            PendingBranchDialog::Rename { .. }
            | PendingBranchDialog::RenameRemote { .. } => {
                crate::git::lucide("pencil")
            }
            PendingBranchDialog::Stash => crate::git::lucide("archive"),
            PendingBranchDialog::Merge { .. } => {
                crate::git::lucide("git-merge")
            }
            PendingBranchDialog::Rebase => {
                crate::git::lucide("git-commit-horizontal")
            }
            PendingBranchDialog::DeleteRef { .. }
            | PendingBranchDialog::DeleteRemote { .. }
            | PendingBranchDialog::DropStash { .. } => {
                crate::git::lucide("trash-2")
            }
        };
        let title_text = match &pending {
            PendingBranchDialog::NewBranch => {
                i18n::text(locale, "branch-new-title")
            }
            PendingBranchDialog::Rename { .. } => {
                i18n::text(locale, "branch-rename-title")
            }
            PendingBranchDialog::RenameRemote { .. } => {
                i18n::text(locale, "rename-remote-branch-title")
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
            PendingBranchDialog::DeleteRef { is_tag, .. } => i18n::text(
                locale,
                if *is_tag {
                    "delete-tag-title"
                } else {
                    "delete-branch-title"
                },
            ),
            PendingBranchDialog::DeleteRemote { .. } => {
                i18n::text(locale, "delete-remote-branch-title")
            }
            PendingBranchDialog::DropStash { .. } => {
                i18n::text(locale, "stash-drop-title")
            }
        };

        let title_row = h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(16.))
                    .text_color(colors.muted_foreground)
                    .child(title_icon),
            )
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(14.))
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
            btn = if matches!(
                pending,
                PendingBranchDialog::Rebase
                    | PendingBranchDialog::DeleteRef { .. }
                    | PendingBranchDialog::DeleteRemote { .. }
                    | PendingBranchDialog::DropStash { .. }
            ) {
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
            .text_size(crate::theme::scaled_text_size(12.))
            .text_color(colors.muted_foreground)
            .child(shared(i18n::text(locale, label_key))),
    );
    if let Some(input) = input {
        body = body.child(div().w_full().child(Input::new(input)));
    }
    body = body.child(
        div()
            .text_size(crate::theme::scaled_text_size(12.))
            .text_color(colors.muted_foreground)
            .child(shared(i18n::text_args(locale, hint_key, hint_args))),
    );
    if let Some(error) = error_text {
        body = body.child(
            div()
                .text_size(crate::theme::scaled_text_size(12.))
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

/// No-fast-forward checkbox for the merge dialog.
fn merge_no_ff_checkbox(
    locale: Locale,
    this: &Entity<RepoTab>,
    tab: &RepoTab,
) -> Checkbox {
    dialog_checkbox(
        "merge-no-ff",
        i18n::text(locale, "merge-no-ff-label"),
        tab.dialogs.no_ff,
        this,
        |tab, checked| tab.dialogs.no_ff = checked,
    )
}

/// Force-delete checkbox for the branch delete dialog.
fn delete_force_checkbox(
    locale: Locale,
    this: &Entity<RepoTab>,
    tab: &RepoTab,
) -> Checkbox {
    dialog_checkbox(
        "delete-force",
        i18n::text(locale, "delete-force-label"),
        tab.dialogs.force_delete,
        this,
        |tab, checked| tab.dialogs.force_delete = checked,
    )
}

/// Shared checkbox wiring for branch operation dialogs.
fn dialog_checkbox(
    id: &'static str,
    label: String,
    checked: bool,
    this: &Entity<RepoTab>,
    set: impl Fn(&mut RepoTab, bool) + Copy + 'static,
) -> Checkbox {
    let entity = this.clone();
    Checkbox::new(id).label(label).checked(checked).on_click(
        move |checked: &bool, _window, cx| {
            entity.update(cx, |tab, cx| {
                set(tab, *checked);
                cx.notify();
            });
        },
    )
}

/// Re-validate at confirm time and close the dialog before dispatching the
/// command to the Git worker.
fn confirm_branch_dialog(tab: &mut RepoTab, cx: &mut Context<RepoTab>) {
    let Some(pending) = tab.dialogs.pending.clone() else {
        return;
    };
    if let PendingBranchDialog::Merge { no_ff } = pending {
        let Some(source) = tab.dialogs.merge_source.clone() else {
            return;
        };
        tab.dialogs.close();
        tab.start_merge_command(source, no_ff, cx);
        cx.notify();
        return;
    }
    let (label, args) = match pending {
        PendingBranchDialog::NewBranch => {
            let Some(name) = validated_name(tab, cx, None) else {
                return;
            };
            ("switch", vec!["switch".into(), "-c".into(), name])
        }
        PendingBranchDialog::Rename { old } => {
            let Some(name) = validated_name(tab, cx, Some(&old)) else {
                return;
            };
            args::rename_args(&old, &name)
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
        PendingBranchDialog::Merge { .. } => {
            unreachable!("merge handled above")
        }
        PendingBranchDialog::Rebase => {
            let Some(source) = tab.dialogs.merge_source.clone() else {
                return;
            };
            ("rebase", vec!["rebase".into(), source])
        }
        PendingBranchDialog::DeleteRef { name, is_tag } => {
            args::delete_args(&name, tab.dialogs.force_delete, is_tag)
        }
        PendingBranchDialog::RenameRemote { remote, old } => {
            // Only git-ref syntax is checked locally; an existing remote
            // branch is rejected by the remote when the push runs.
            let name = input_value(tab, cx);
            let Some(name) = validated_name_in(&name, &[], None) else {
                return;
            };
            args::rename_remote_args(&remote, &old, &name)
        }
        PendingBranchDialog::DeleteRemote { remote, branch } => {
            args::delete_remote_args(&remote, &branch)
        }
        PendingBranchDialog::DropStash { reference } => {
            ("stash drop", args::stash_drop_args(&reference))
        }
    };
    tab.dialogs.close();
    log::info!("[branch_ops] command queued: {label}, args={args:?}");
    tab.git_view.update(cx, |view, _| view.run(label, args));
    tab.set_operation_busy(true, cx);
    cx.notify();
}

/// Confirm-time validation for name dialogs. The allow value exempts the old
/// name when renaming.
fn validated_name(
    tab: &RepoTab,
    cx: &Context<RepoTab>,
    allow: Option<&str>,
) -> Option<String> {
    let name = input_value(tab, cx);
    let mut existing = tab.local_branches.clone();
    if !tab.branch.is_empty() {
        existing.push(tab.branch.clone());
    }
    validated_name_in(&name, &existing, allow)
}

/// Core confirm-time validation against an explicit branch list.
fn validated_name_in(
    name: &str,
    existing: &[String],
    allow: Option<&str>,
) -> Option<String> {
    match args::validate_branch_name(name, existing, allow) {
        None => Some(name.to_string()),
        Some(error) => {
            log::warn!("[branch_ops] rejected branch name: {error:?}");
            None
        }
    }
}
