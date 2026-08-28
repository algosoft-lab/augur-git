//! Commit input presentation and the bottom diff panel re-export.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};

use crate::core::i18n::{self, Locale};
use crate::git::shared;

pub use crate::git::bottom_panel::{BottomPanel, BottomPanelEvent};

/// CommitPanel to Workspace events.
#[derive(Clone, Debug)]
pub enum CommitPanelEvent {
    /// Commit the current message, optionally amending the last commit.
    Submit { message: String, amend: bool },
}

/// Commit message input panel.
pub struct CommitPanel {
    input: Entity<TextareaState>,
    /// Whether staged changes are available for a normal commit.
    has_staged: bool,
    /// Whether another repository operation is currently running.
    busy: bool,
    /// Whether the selected action amends the last commit.
    amend: bool,
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
            busy: false,
            amend: false,
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

    /// Disable commit submission while another repository operation is active.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    fn set_amend(&mut self, amend: bool, cx: &mut Context<Self>) {
        if self.amend != amend {
            self.amend = amend;
            cx.notify();
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let msg = self.input.read(cx).value().to_string();
        if msg.trim().is_empty() {
            return;
        }
        cx.emit(CommitPanelEvent::Submit {
            message: msg,
            amend: self.amend,
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
                    .text_size(px(12.))
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(self.locale, "commit-title"))),
            );

        let can_commit = !self.busy && (self.has_staged || self.amend);
        let commit_button_label = i18n::text(
            self.locale,
            if self.amend {
                "commit-amend-btn"
            } else {
                "commit-btn"
            },
        );
        let btn_commit = cx.entity();
        let commit_btn = Button::new("btn-commit")
            .label(commit_button_label)
            .primary()
            .compact()
            .flex_1()
            .disabled(!can_commit)
            .when(can_commit, |button| {
                button.on_click(move |_event, _window, cx| {
                    btn_commit.update(cx, |panel, cx| panel.submit(cx));
                })
            });

        let amend = self.amend;
        let mode_panel = cx.entity();
        let commit_action_label =
            i18n::text(self.locale, "commit-action-commit");
        let amend_action_label = i18n::text(self.locale, "commit-action-amend");
        let commit_mode_menu = Button::new("btn-commit-mode")
            .icon(IconName::ChevronDown)
            .primary()
            .xsmall()
            .h_8()
            .disabled(self.busy)
            .dropdown_menu_with_anchor(
                Anchor::BottomRight,
                move |menu, _window, _cx| {
                    let commit_panel = mode_panel.clone();
                    let amend_panel = mode_panel.clone();
                    menu.item(
                        PopupMenuItem::new(commit_action_label.clone())
                            .checked(!amend)
                            .on_click(move |_event, _window, cx| {
                                commit_panel.update(cx, |panel, cx| {
                                    panel.set_amend(false, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new(amend_action_label.clone())
                            .checked(amend)
                            .on_click(move |_event, _window, cx| {
                                amend_panel.update(cx, |panel, cx| {
                                    panel.set_amend(true, cx);
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
                            .child(commit_mode_menu),
                    ),
            )
    }
}
