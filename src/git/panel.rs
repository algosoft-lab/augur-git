//! Commit input presentation and the bottom diff panel re-export.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonRounded, ButtonVariants},
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};

use crate::core::i18n::{self, Locale};
use crate::git::shared;

pub use crate::git::bottom_panel::{BottomPanel, BottomPanelEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitAction {
    Commit,
    Amend,
    CommitByAgent,
}

/// CommitPanel to Workspace events.
#[derive(Clone, Debug)]
pub enum CommitPanelEvent {
    /// Commit the current message or delegate the operation to an Agent.
    Submit {
        message: String,
        action: CommitAction,
    },
}

/// Commit message input panel.
pub struct CommitPanel {
    input: Entity<TextareaState>,
    /// Whether staged changes are available for a normal commit.
    has_staged: bool,
    /// Whether any staged, unstaged, or untracked changes are available.
    has_changes: bool,
    /// Whether another repository operation is currently running.
    busy: bool,
    /// The selected commit action.
    action: CommitAction,
    /// UI locale, synchronized by Workspace.
    locale: Locale,
}

impl EventEmitter<CommitPanelEvent> for CommitPanel {}

impl CommitPanel {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        locale: Locale,
    ) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 5)
                .submit_on_enter(true)
                .placeholder(i18n::text(locale, "commit-placeholder"))
        });

        // Enter submits; Shift+Enter inserts a newline.
        let input_entity = input.clone();
        cx.subscribe(&input_entity, |panel, _e, event, cx| {
            if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                panel.submit(cx);
            }
        })
        .detach();

        Self {
            input,
            has_staged: false,
            has_changes: false,
            busy: false,
            action: CommitAction::Commit,
            locale,
        }
    }

    /// Synchronize the locale and update the input placeholder.
    pub fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        let placeholder = i18n::text(locale, "commit-placeholder");
        self.input.update(cx, |input, cx| {
            let base = input.base_state().clone();
            base.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx);
            });
        });
        cx.notify();
    }

    pub fn set_has_staged(&mut self, has_staged: bool, cx: &mut Context<Self>) {
        if self.has_staged != has_staged {
            self.has_staged = has_staged;
            cx.notify();
        }
    }

    pub fn set_has_changes(
        &mut self,
        has_changes: bool,
        cx: &mut Context<Self>,
    ) {
        if self.has_changes != has_changes {
            self.has_changes = has_changes;
            cx.notify();
        }
    }

    /// Disable commit submission while another repository operation is active.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    fn set_action(&mut self, action: CommitAction, cx: &mut Context<Self>) {
        if self.action != action {
            self.action = action;
            cx.notify();
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let msg = self.input.read(cx).value().to_string();
        if self.action != CommitAction::CommitByAgent && msg.trim().is_empty() {
            return;
        }
        cx.emit(CommitPanelEvent::Submit {
            message: msg,
            action: self.action,
        });
    }
}

impl Render for CommitPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();

        // The commit editor is intentionally always visible at the top of the
        // right panel so staging and committing remain one continuous flow.
        let header = h_flex()
            .id("commit-header")
            .w_full()
            .h(px(30.))
            .flex_shrink_0()
            .px_3()
            .items_center()
            .gap_2()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(self.locale, "commit-title"))),
            );

        let can_commit = !self.busy
            && can_submit(self.action, self.has_staged, self.has_changes);
        let commit_button_label = match self.action {
            CommitAction::Commit => i18n::text(self.locale, "commit-btn"),
            CommitAction::Amend => i18n::text(self.locale, "commit-amend-btn"),
            CommitAction::CommitByAgent => {
                i18n::text(self.locale, "commit-ai-btn")
            }
        };
        let btn_commit = cx.entity();
        let commit_btn = Button::new("btn-commit")
            .label(commit_button_label)
            .primary()
            .compact()
            .rounded(ButtonRounded::None)
            .flex_1()
            .disabled(!can_commit)
            .when(can_commit, |button| {
                button.on_click(move |_event, _window, cx| {
                    btn_commit.update(cx, |panel, cx| panel.submit(cx));
                })
            });

        let action = self.action;
        let mode_panel = cx.entity();
        let commit_action_label =
            i18n::text(self.locale, "commit-action-commit");
        let amend_action_label = i18n::text(self.locale, "commit-action-amend");
        let agent_action_label = i18n::text(self.locale, "commit-action-ai");
        let commit_mode_menu = Button::new("btn-commit-mode")
            .icon(IconName::ChevronDown)
            .primary()
            .xsmall()
            .h_8()
            .rounded(ButtonRounded::None)
            .disabled(self.busy)
            .dropdown_menu_with_anchor(
                Anchor::TopRight,
                move |menu, _window, _cx| {
                    let commit_panel = mode_panel.clone();
                    let amend_panel = mode_panel.clone();
                    let agent_panel = mode_panel.clone();
                    menu.item(
                        PopupMenuItem::new(commit_action_label.clone())
                            .checked(action == CommitAction::Commit)
                            .on_click(move |_event, _window, cx| {
                                commit_panel.update(cx, |panel, cx| {
                                    panel.set_action(CommitAction::Commit, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new(amend_action_label.clone())
                            .checked(action == CommitAction::Amend)
                            .on_click(move |_event, _window, cx| {
                                amend_panel.update(cx, |panel, cx| {
                                    panel.set_action(CommitAction::Amend, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new(agent_action_label.clone())
                            .checked(action == CommitAction::CommitByAgent)
                            .on_click(move |_event, _window, cx| {
                                agent_panel.update(cx, |panel, cx| {
                                    panel.set_action(
                                        CommitAction::CommitByAgent,
                                        cx,
                                    );
                                });
                            }),
                    )
                },
            );
        v_flex()
            .id("commit-panel")
            .w_full()
            .flex_shrink_0()
            .child(header)
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .p_3()
                    .child(Textarea::new(&self.input).w_full())
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_0()
                            .child(commit_btn)
                            // Square split-button divider between the commit
                            // action and its mode selector.
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(px(1.))
                                    .h_5()
                                    .bg(colors.border),
                            )
                            .child(commit_mode_menu),
                    ),
            )
    }
}

fn can_submit(
    action: CommitAction,
    has_staged: bool,
    has_changes: bool,
) -> bool {
    match action {
        CommitAction::Commit | CommitAction::Amend => has_staged,
        CommitAction::CommitByAgent => has_changes,
    }
}

#[cfg(test)]
mod tests {
    use super::{CommitAction, can_submit};

    #[test]
    fn normal_commit_actions_require_staged_changes() {
        assert!(!can_submit(CommitAction::Commit, false, true));
        assert!(!can_submit(CommitAction::Amend, false, true));
        assert!(can_submit(CommitAction::Commit, true, true));
        assert!(can_submit(CommitAction::Amend, true, true));
    }

    #[test]
    fn agent_commit_accepts_any_working_tree_change() {
        assert!(!can_submit(CommitAction::CommitByAgent, false, false));
        assert!(can_submit(CommitAction::CommitByAgent, false, true));
        assert!(can_submit(CommitAction::CommitByAgent, true, true));
    }
}
