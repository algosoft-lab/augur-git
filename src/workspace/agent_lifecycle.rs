//! Workspace-level lifecycle guards for Agent and extension operations.
//!
//! This module coordinates actions that can close the workspace or the whole
//! application. Active background operations are always listed behind an
//! explicit confirmation before they are terminated.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use std::time::Duration;

use crate::core::i18n;

use super::tabs::{TabId, fallback_after_close};
use super::{TabContent, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingWorkspaceClose {
    Application,
    Tab(TabId),
}

impl Workspace {
    /// Request application quit from the menu or a command. The app quit is
    /// deferred while Agent processes are still active.
    pub(super) fn request_application_quit(&mut self, cx: &mut Context<Self>) {
        if self.pending_close.is_some() {
            return;
        }
        let count = self.active_operation_count(cx);
        if count == 0 {
            cx.quit();
        } else {
            log::info!(
                "[workspace] delaying application quit for {count} active operation(s)"
            );
            self.pending_close = Some(PendingWorkspaceClose::Application);
            cx.notify();
        }
    }

    /// Handle a native window close request. Returning `false` keeps the
    /// window open while the same confirmation card used by application Quit
    /// is displayed.
    pub(super) fn request_window_close(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending_close.is_some() {
            return false;
        }
        let count = self.active_operation_count(cx);
        if count == 0 {
            true
        } else {
            log::info!(
                "[workspace] delaying window close for {count} active operation(s)"
            );
            self.pending_close = Some(PendingWorkspaceClose::Application);
            cx.notify();
            false
        }
    }

    pub(super) fn request_tab_close(
        &mut self,
        id: TabId,
        cx: &mut Context<Self>,
    ) {
        if self.pending_close.is_some() {
            return;
        }
        if !self.tabs.iter().any(|entry| entry.id == id) {
            return;
        }
        let agent_active = if let Some(path) = self
            .tabs
            .iter()
            .find(|entry| entry.id == id)
            .and_then(|entry| entry.path.clone())
        {
            super::agent_connectivity::running_for_repo(self, &path, cx) > 0
        } else {
            false
        };
        let extension_active = self
            .extension_manager
            .as_ref()
            .is_some_and(|manager| manager.active_count() > 0);
        if agent_active || extension_active {
            log::info!(
                "[workspace] delaying repository tab close for active background operation"
            );
            self.pending_close = Some(PendingWorkspaceClose::Tab(id));
            cx.notify();
            return;
        }
        self.close_tab_now(id, cx);
    }

    fn close_tab_now(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let order = self.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
        let fallback = fallback_after_close(&order, self.active_tab, id);
        let entry = self.tabs.remove(index);
        if let TabContent::Repo(tab) = &entry.content {
            tab.update(cx, |tab, cx| tab.close(cx));
        }
        let was_active = self.active_tab == Some(id);
        if was_active {
            self.active_tab = None;
        }
        self.active_tab = fallback;
        if let Some(active) = fallback {
            self.activate_tab(active, cx);
        }
        self.persist_config();
        self.refresh_tab_bar(cx);
        cx.notify();
    }

    pub(super) fn cancel_pending_close(&mut self, cx: &mut Context<Self>) {
        if self.pending_close.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn confirm_pending_close(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_close.take() else {
            return;
        };
        match pending {
            PendingWorkspaceClose::Application => {
                super::agent_connectivity::stop_all(self, cx);
                if let Some(manager) = &self.extension_manager {
                    let cancelled = manager.cancel_all();
                    if cancelled > 0 {
                        log::info!(
                            "[extension_runtime] cancelled {cancelled} active extension run(s) during application close"
                        );
                    }
                }
                log::info!("[agent_terminal] confirmed application close");
                // `TerminalBackend::shutdown` gives each child a short grace
                // period before closing its PTY. Keep the app alive for that
                // hand-off so the PTY event loop can deliver the shutdown
                // message instead of letting the process exit immediately.
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(220))
                        .await;
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
            PendingWorkspaceClose::Tab(id) => {
                if let Some(path) = self
                    .tabs
                    .iter()
                    .find(|entry| entry.id == id)
                    .and_then(|entry| entry.path.clone())
                {
                    super::agent_connectivity::stop_for_repo(self, &path, cx);
                }
                if let Some(manager) = &self.extension_manager {
                    let cancelled = manager.cancel_all();
                    if cancelled > 0 {
                        log::info!(
                            "[extension_runtime] cancelled {cancelled} active extension run(s) during repository tab close"
                        );
                    }
                }
                self.close_tab_now(id, cx);
            }
        }
    }

    pub(super) fn close_confirmation_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(pending) = self.pending_close else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors.clone();
        let mut test_labels = match pending {
            PendingWorkspaceClose::Application => {
                super::agent_connectivity::running_labels(self, cx)
            }
            PendingWorkspaceClose::Tab(id) => self
                .tabs
                .iter()
                .find(|entry| entry.id == id)
                .and_then(|entry| entry.path.as_deref())
                .map(|path| {
                    super::agent_connectivity::running_labels_for_repo(
                        self, path, cx,
                    )
                })
                .unwrap_or_default(),
        };
        if let Some(manager) = &self.extension_manager {
            test_labels.extend(manager.active_labels());
        }
        let count = test_labels.len();
        let count_text = count.to_string();
        let title = i18n::text(self.locale, "workspace-close-title");
        let warning = i18n::text_args(
            self.locale,
            "workspace-close-warning",
            &[("count", &count_text)],
        );
        let this = cx.entity();
        let cancel = this.clone();
        let cancel_backdrop = cancel.clone();
        let confirm = this.clone();
        v_flex()
            .id("workspace-close-overlay")
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
                cancel_backdrop.update(cx, |workspace, cx| {
                    workspace.cancel_pending_close(cx);
                });
            })
            .child(
                v_flex()
                    .id("workspace-close-card")
                    .items_start()
                    .gap_3()
                    .p_6()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .min_w(px(380.))
                    .max_w(px(520.))
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
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .text_color(colors.warning),
                            )
                            .child(
                                div()
                                    .text_color(colors.foreground)
                                    .font_weight(FontWeight::BOLD)
                                    .child(SharedString::from(title)),
                            ),
                    )
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .text_size(crate::theme::scaled_text_size(12.))
                            .child(SharedString::from(warning)),
                    )
                    .children(test_labels.into_iter().map(|label| {
                        div()
                            .w_full()
                            .text_color(colors.foreground)
                            .text_size(crate::theme::scaled_text_size(11.))
                            .child(SharedString::from(format!("• {label}")))
                    }))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                Button::new("workspace-close-cancel")
                                    .label(i18n::text(
                                        self.locale,
                                        "workspace-close-cancel",
                                    ))
                                    .ghost()
                                    .flex_1()
                                    .on_click(move |_event, _window, cx| {
                                        cancel.update(cx, |workspace, cx| {
                                            workspace.cancel_pending_close(cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new("workspace-close-confirm")
                                    .label(i18n::text(
                                        self.locale,
                                        "workspace-close-confirm",
                                    ))
                                    .danger()
                                    .flex_1()
                                    .on_click(move |_event, _window, cx| {
                                        confirm.update(cx, |workspace, cx| {
                                            workspace.confirm_pending_close(cx);
                                        });
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn active_operation_count(&self, cx: &mut Context<Self>) -> usize {
        super::agent_connectivity::running_count(self, cx)
            + self
                .extension_manager
                .as_ref()
                .map(|manager| manager.active_count())
                .unwrap_or(0)
    }
}
