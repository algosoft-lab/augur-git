use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, h_flex, v_flex};

use crate::core::config::{
    MAX_DIFF_HEIGHT, MAX_RIGHT_PANEL_WIDTH, MAX_SIDEBAR_WIDTH, MIN_DIFF_HEIGHT,
    MIN_RIGHT_PANEL_WIDTH, MIN_SIDEBAR_WIDTH,
};
use crate::core::i18n;
use crate::git::GitStatus;

use super::{
    DIFF_RESIZE_HANDLE_HEIGHT, DiffViewerResize, MIN_COMMIT_HEIGHT, RepoTab,
    RepoTabEvent, RightPanelResize, SidebarResize,
};

/// Drag payloads for the resize handles; their render output is empty and
/// only serves as the drag marker type.
impl Render for SidebarResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

impl Render for RightPanelResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

impl Render for DiffViewerResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

impl RepoTab {
    /// Bottom status bar: repository path, the last operation result
    /// message, and transient or failing connection states.
    pub(super) fn status_bar(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let state_text = match &self.status {
            GitStatus::Ready(_) => None,
            GitStatus::None => Some((
                i18n::text(self.locale, "no-repo-open"),
                colors.muted_foreground,
            )),
            GitStatus::Scanning => Some((
                i18n::text(self.locale, "status-scanning"),
                colors.warning,
            )),
            GitStatus::Error(message) => {
                Some((format!("✗ {message}"), colors.red))
            }
        };
        // While a generic Git command runs, its animated verb takes over the
        // result slot; a finished operation's message is stale anyway
        // because the worker executes commands serially.
        let (msg, msg_color) = if let Some(verb) = self.busy_verb {
            (
                Some(format!("{verb}{}", ".".repeat(self.progress_dots))),
                colors.warning,
            )
        } else {
            let color = match self.status_message_ok {
                Some(true) => colors.green,
                Some(false) => colors.red,
                None => colors.muted_foreground,
            };
            (self.status_message.clone(), color)
        };

        h_flex()
            .id("status-bar")
            .w_full()
            .h_6()
            .flex_shrink_0()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.background)
            .px_3()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(11.))
                    .text_color(colors.muted_foreground)
                    .truncate()
                    .child(SharedString::from(self.repo_path.clone())),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when_some(msg, |row, message| {
                        row.child(
                            div()
                                .text_size(crate::theme::scaled_text_size(11.))
                                .text_color(msg_color)
                                .child(SharedString::from(message)),
                        )
                    })
                    .when_some(state_text, |row, (text, color)| {
                        row.child(
                            div()
                                .text_size(crate::theme::scaled_text_size(11.))
                                .text_color(color)
                                .child(text),
                        )
                    }),
            )
    }

    pub(super) fn main_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let sidebar_width = px(self.layout.sidebar_width);
        let right_panel_width = px(self.layout.right_panel_width);
        let available_height = f32::from(window.bounds().size.height);
        let max_diff_height =
            (available_height - MIN_COMMIT_HEIGHT - DIFF_RESIZE_HANDLE_HEIGHT)
                .max(MIN_DIFF_HEIGHT)
                .min(MAX_DIFF_HEIGHT);
        let diff_height = self
            .layout
            .diff_height
            .map(|height| px(height.clamp(MIN_DIFF_HEIGHT, max_diff_height)));

        h_flex()
            .id("main-content")
            .size_full()
            .min_h_0()
            .on_drag_move::<SidebarResize>(cx.listener(
                |tab, event: &DragMoveEvent<SidebarResize>, _, cx| {
                    let new_width = f32::from(event.event.position.x)
                        .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
                    if (tab.layout.sidebar_width - new_width).abs()
                        > f32::EPSILON
                    {
                        tab.layout.sidebar_width = new_width;
                        cx.emit(RepoTabEvent::LayoutChanged(
                            tab.layout.clone(),
                        ));
                        cx.notify();
                    }
                },
            ))
            .on_drag_move::<RightPanelResize>(cx.listener(
                |tab, event: &DragMoveEvent<RightPanelResize>, window, cx| {
                    let width = window.bounds().size.width;
                    let new_width = (f32::from(width)
                        - f32::from(event.event.position.x))
                    .clamp(MIN_RIGHT_PANEL_WIDTH, MAX_RIGHT_PANEL_WIDTH);
                    if (tab.layout.right_panel_width - new_width).abs()
                        > f32::EPSILON
                    {
                        tab.layout.right_panel_width = new_width;
                        cx.emit(RepoTabEvent::LayoutChanged(
                            tab.layout.clone(),
                        ));
                        cx.notify();
                    }
                },
            ))
            .on_drag_move::<DiffViewerResize>(cx.listener(
                |tab, event: &DragMoveEvent<DiffViewerResize>, _, cx| {
                    let main_content_height =
                        f32::from(event.bounds.size.height);
                    let position = f32::from(
                        event.event.position.y - event.bounds.origin.y,
                    );
                    let max_diff_height = (main_content_height
                        - MIN_COMMIT_HEIGHT
                        - DIFF_RESIZE_HANDLE_HEIGHT)
                        .clamp(MIN_DIFF_HEIGHT, MAX_DIFF_HEIGHT);
                    let diff_height = Some(
                        (main_content_height - position)
                            .clamp(MIN_DIFF_HEIGHT, max_diff_height),
                    );
                    if tab.layout.diff_height != diff_height {
                        tab.layout.diff_height = diff_height;
                        cx.emit(RepoTabEvent::LayoutChanged(
                            tab.layout.clone(),
                        ));
                        cx.notify();
                    }
                },
            ))
            .child(
                div()
                    .relative()
                    .w(sidebar_width)
                    .h_full()
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(colors.border)
                    .child(self.sidebar.clone())
                    .child(
                        div()
                            .id("sidebar-resize-handle")
                            .absolute()
                            .top_0()
                            .right(px(-3.))
                            .h_full()
                            .w(px(5.))
                            .cursor_col_resize()
                            .hover(|element| element.bg(colors.drag_border))
                            .on_drag(SidebarResize, |value, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| value.clone())
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(div().flex_1().min_h_0().child(self.graph.clone()))
                    .child(
                        div()
                            .id("diff-resize-handle")
                            .w_full()
                            .h(px(3.))
                            .flex_shrink_0()
                            .border_t_1()
                            .border_color(colors.border)
                            .cursor_row_resize()
                            .hover(|element| element.bg(colors.drag_border))
                            .on_drag(DiffViewerResize, |value, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| value.clone())
                            }),
                    )
                    .child({
                        let bottom =
                            v_flex().min_h_0().child(self.bottom.clone());
                        match diff_height {
                            Some(height) => bottom.h(height).flex_shrink_0(),
                            None => bottom.flex_1(),
                        }
                    }),
            )
            .child(
                div()
                    .relative()
                    .w(right_panel_width)
                    .h_full()
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(colors.border)
                    .flex()
                    .flex_col()
                    .child(self.commit.clone())
                    .child(
                        div()
                            .w_full()
                            .h(px(1.))
                            .flex_shrink_0()
                            .bg(colors.border),
                    )
                    .child(div().flex_1().min_h_0().child(self.changes.clone()))
                    .child(
                        div()
                            .id("right-panel-resize-handle")
                            .absolute()
                            .top_0()
                            .left(px(-3.))
                            .h_full()
                            .w(px(5.))
                            .cursor_col_resize()
                            .hover(|element| element.bg(colors.drag_border))
                            .on_drag(RightPanelResize, |value, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| value.clone())
                            }),
                    ),
            )
    }
}
