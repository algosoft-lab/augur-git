//! Confirmation overlays used by a repository tab.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use crate::core::git::{WorkingTreeAction, WorkingTreeScope};
use crate::core::i18n;
use crate::git::shared;

use super::{PendingConfirmation, RepoTab};

impl RepoTab {
    pub(super) fn request_discard(
        &mut self,
        scope: WorkingTreeScope,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy {
            return;
        }
        let files = scope.files();
        if files.is_empty() || files.iter().any(|file| file.is_conflicted()) {
            return;
        }
        let tracked_count =
            files.iter().filter(|file| !file.is_untracked()).count();
        let untracked_count =
            files.iter().filter(|file| file.is_untracked()).count();
        if tracked_count == 0 && untracked_count == 0 {
            return;
        }
        self.confirmation = Some(PendingConfirmation::Discard {
            scope,
            tracked_count,
            untracked_count,
        });
        cx.notify();
    }

    pub(super) fn start_force_push(&mut self, cx: &mut Context<Self>) {
        if self.operation_busy {
            return;
        }
        self.confirmation = None;
        self.git_view.update(cx, |view, _| {
            view.run("push --force", vec!["push".into(), "--force".into()]);
        });
        self.set_operation_busy(true, cx);
        cx.notify();
    }

    /// Offer to publish the current branch when it has no upstream yet.
    ///
    /// Returns whether the confirmation dialog was opened. The dialog is
    /// skipped for detached or unborn HEAD states and when no remote exists,
    /// so a plain push can surface Git's own error in those cases.
    pub(super) fn request_push_upstream(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.branch.is_empty() || self.branch.starts_with("HEAD") {
            return false;
        }
        if self.upstream.is_some() {
            return false;
        }
        let Some(remote) = default_push_remote(&self.remotes) else {
            return false;
        };
        log::info!(
            "[git_push] branch '{}' has no upstream; offering to publish to '{remote}'",
            self.branch
        );
        self.confirmation = Some(PendingConfirmation::PushSetUpstream {
            branch: self.branch.clone(),
            remote,
        });
        cx.notify();
        true
    }

    pub(super) fn start_push_upstream(&mut self, cx: &mut Context<Self>) {
        let Some(PendingConfirmation::PushSetUpstream { branch, remote }) =
            self.confirmation.take()
        else {
            return;
        };
        if self.operation_busy {
            return;
        }
        log::info!(
            "[git_push] publishing branch '{branch}' to '{remote}' with --set-upstream"
        );
        self.git_view.update(cx, |view, _| {
            view.run(
                "push --set-upstream",
                vec!["push".into(), "--set-upstream".into(), remote, branch],
            );
        });
        self.set_operation_busy(true, cx);
        cx.notify();
    }

    pub(super) fn confirm_discard(&mut self, cx: &mut Context<Self>) {
        let Some(PendingConfirmation::Discard { scope, .. }) =
            self.confirmation.take()
        else {
            return;
        };
        self.start_working_tree_operation(
            WorkingTreeAction::Discard,
            scope,
            cx,
        );
        cx.notify();
    }

    pub(super) fn cancel_confirmation(&mut self, cx: &mut Context<Self>) {
        if self.confirmation.take().is_some() {
            cx.notify();
        }
    }

    /// Close whichever overlay is currently open, preferring the branch
    /// operations dialog over the destructive-action confirmations.
    pub(super) fn cancel_topmost(&mut self, cx: &mut Context<Self>) {
        if self.dialogs.close() {
            cx.notify();
            return;
        }
        self.cancel_confirmation(cx);
    }

    pub(super) fn confirmation_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.confirmation.as_ref() {
            Some(PendingConfirmation::ForcePush) => {
                self.force_push_confirm_overlay(cx).into_any_element()
            }
            Some(PendingConfirmation::PushSetUpstream { .. }) => {
                self.push_upstream_confirm_overlay(cx)
            }
            Some(PendingConfirmation::Discard { .. }) => {
                self.discard_confirm_overlay(cx)
            }
            None => div().into_any_element(),
        }
    }

    fn push_upstream_confirm_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors.clone();
        let locale = self.locale;
        let this = cx.entity();
        let Some(PendingConfirmation::PushSetUpstream { branch, remote }) =
            self.confirmation.as_ref()
        else {
            return div().into_any_element();
        };

        let title_row = h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(16.))
                    .text_color(colors.blue)
                    .child(Icon::new(IconName::ArrowUp)),
            )
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(locale, "push-upstream-title"))),
            );

        let cancel_btn = {
            let this = this.clone();
            Button::new("push-upstream-cancel")
                .label(i18n::text(locale, "push-upstream-cancel"))
                .ghost()
                .flex_1()
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| tab.cancel_confirmation(cx));
                })
        };
        let confirm_btn = {
            let this = this.clone();
            Button::new("push-upstream-confirm")
                .label(i18n::text(locale, "push-upstream-confirm"))
                .primary()
                .flex_1()
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| tab.start_push_upstream(cx));
                })
        };

        self.overlay_card(
            cx,
            "push-upstream-overlay",
            "push-upstream-card",
            title_row,
            div()
                .text_size(crate::theme::scaled_text_size(12.))
                .text_color(colors.muted_foreground)
                .child(shared(i18n::text_args(
                    locale,
                    "push-upstream-warning",
                    &[("branch", branch), ("remote", remote)],
                ))),
            h_flex()
                .w_full()
                .gap_2()
                .child(cancel_btn)
                .child(confirm_btn),
        )
        .into_any_element()
    }

    fn force_push_confirm_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let locale = self.locale;
        let this = cx.entity();

        let title_row = h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(16.))
                    .text_color(colors.red)
                    .child(Icon::new(IconName::TriangleAlert)),
            )
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(locale, "push-force-title"))),
            );

        let cancel_btn = {
            let this = this.clone();
            Button::new("force-push-cancel")
                .label(i18n::text(locale, "push-force-cancel"))
                .ghost()
                .flex_1()
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| tab.cancel_confirmation(cx));
                })
        };
        let confirm_btn = {
            let this = this.clone();
            Button::new("force-push-confirm")
                .label(i18n::text(locale, "push-force-confirm"))
                .danger()
                .flex_1()
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| tab.start_force_push(cx));
                })
        };

        self.overlay_card(
            cx,
            "force-push-overlay",
            "force-push-card",
            title_row,
            div()
                .text_size(crate::theme::scaled_text_size(12.))
                .text_color(colors.muted_foreground)
                .child(shared(i18n::text_args(
                    locale,
                    "push-force-warning",
                    &[("branch", &self.branch)],
                ))),
            h_flex()
                .w_full()
                .gap_2()
                .child(cancel_btn)
                .child(confirm_btn),
        )
    }

    fn discard_confirm_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors.clone();
        let locale = self.locale;
        let Some(PendingConfirmation::Discard {
            scope,
            tracked_count,
            untracked_count,
        }) = self.confirmation.as_ref()
        else {
            return div().into_any_element();
        };
        let this = cx.entity();
        let warning = match scope {
            WorkingTreeScope::File(file) => {
                let key = if file.is_untracked() {
                    "discard-untracked-file-warning"
                } else {
                    "discard-file-warning"
                };
                i18n::text_args(locale, key, &[("path", &file.path)])
            }
            WorkingTreeScope::All(_) => {
                let tracked = tracked_count.to_string();
                let untracked = untracked_count.to_string();
                i18n::text_args(
                    locale,
                    "discard-all-warning",
                    &[("tracked", &tracked), ("untracked", &untracked)],
                )
            }
        };
        let title_row = h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(16.))
                    .text_color(colors.red)
                    .child(Icon::new(IconName::TriangleAlert)),
            )
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(locale, "discard-title"))),
            );
        let cancel_btn = {
            let this = this.clone();
            Button::new("discard-cancel")
                .label(i18n::text(locale, "discard-cancel"))
                .ghost()
                .flex_1()
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| tab.cancel_confirmation(cx));
                })
        };
        let confirm_btn = {
            let this = this.clone();
            Button::new("discard-confirm")
                .label(i18n::text(locale, "discard-confirm"))
                .danger()
                .flex_1()
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |tab, cx| tab.confirm_discard(cx));
                })
        };
        self.overlay_card(
            cx,
            "discard-overlay",
            "discard-card",
            title_row,
            div()
                .text_size(crate::theme::scaled_text_size(12.))
                .text_color(colors.muted_foreground)
                .child(shared(warning)),
            h_flex()
                .w_full()
                .gap_2()
                .child(cancel_btn)
                .child(confirm_btn),
        )
        .into_any_element()
    }

    /// Shared card for confirmation and branch-operation overlays. Clicking
    /// the dimmed backdrop closes the topmost overlay.
    pub(super) fn overlay_card<T, W, F>(
        &self,
        cx: &Context<Self>,
        overlay_id: &'static str,
        card_id: &'static str,
        title: T,
        warning: W,
        buttons: F,
    ) -> impl IntoElement
    where
        T: IntoElement,
        W: IntoElement,
        F: IntoElement,
    {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        v_flex()
            .id(overlay_id)
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .bg(colors.background.opacity(0.9))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                this.update(cx, |tab, cx| tab.cancel_topmost(cx));
            })
            .child(
                v_flex()
                    .id(card_id)
                    .items_start()
                    .gap_3()
                    .p_6()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .min_w(px(380.))
                    .max_w(px(500.))
                    .when(cx.theme().shadow, |element| element.shadow_md())
                    .on_mouse_down(
                        MouseButton::Left,
                        |_event: &MouseDownEvent,
                         window: &mut Window,
                         cx: &mut App| {
                            window.prevent_default();
                            cx.stop_propagation();
                        },
                    )
                    .child(title)
                    .child(warning)
                    .child(buttons),
            )
    }
}

/// Pick the remote used to publish a branch without an upstream.
///
/// Prefers `origin` when configured and otherwise falls back to the first
/// configured remote, mirroring what most Git clients assume.
fn default_push_remote(remotes: &[String]) -> Option<String> {
    if remotes.iter().any(|remote| remote == "origin") {
        Some("origin".to_string())
    } else {
        remotes.first().cloned()
    }
}

/// Whether a failed `git push` stderr reports that the current branch has no
/// upstream, which means a `--set-upstream` push can publish it.
pub(super) fn push_error_missing_upstream(message: &str) -> bool {
    message.contains("has no upstream branch")
}

#[cfg(test)]
mod tests {
    use super::{default_push_remote, push_error_missing_upstream};

    #[test]
    fn default_push_remote_prefers_origin_then_first_entry() {
        let multi = vec![
            "upstream".to_string(),
            "origin".to_string(),
            "fork".to_string(),
        ];
        assert_eq!(default_push_remote(&multi).as_deref(), Some("origin"));

        let no_origin = vec!["upstream".to_string(), "fork".to_string()];
        assert_eq!(
            default_push_remote(&no_origin).as_deref(),
            Some("upstream")
        );

        let empty: Vec<String> = Vec::new();
        assert_eq!(default_push_remote(&empty), None);
    }

    #[test]
    fn push_error_missing_upstream_matches_git_message_only() {
        let stderr = concat!(
            "fatal: The current branch feature/x has no upstream branch.\n",
            "To push the current branch and set the remote as upstream, use\n",
        );
        assert!(push_error_missing_upstream(stderr));
        assert!(!push_error_missing_upstream(
            "fatal: unable to access 'https://example.com': connection refused"
        ));
        assert!(!push_error_missing_upstream(""));
    }
}
