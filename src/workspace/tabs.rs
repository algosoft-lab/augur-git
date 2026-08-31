use gpui::prelude::*;
use gpui::*;
use gpui_component::scroll::ScrollableElement;
use gpui_component::theme::ThemeColor;
use gpui_component::{ActiveTheme, Icon, IconName, h_flex};

use crate::git::shared;

pub type TabId = u64;

pub fn fallback_after_close(
    order: &[TabId],
    active: Option<TabId>,
    closing: TabId,
) -> Option<TabId> {
    if active != Some(closing) {
        return active;
    }
    let Some(index) = order.iter().position(|id| *id == closing) else {
        return active;
    };
    order
        .get(index + 1)
        .copied()
        .or_else(|| index.checked_sub(1).and_then(|i| order.get(i).copied()))
}

pub fn should_refresh_after_switch(
    changed: bool,
    target_was_opened: bool,
) -> bool {
    changed && target_was_opened
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabState {
    Loading,
    Ready,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSummary {
    pub id: TabId,
    pub title: String,
    pub branch: Option<String>,
    pub state: TabState,
}

#[derive(Clone, Debug)]
pub enum RepoTabBarEvent {
    NewTab,
    Select(TabId),
    Close(TabId),
}

pub struct RepoTabBar {
    tabs: Vec<TabSummary>,
    active: Option<TabId>,
}

impl EventEmitter<RepoTabBarEvent> for RepoTabBar {}

impl RepoTabBar {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
        }
    }

    pub fn set_tabs(
        &mut self,
        tabs: Vec<TabSummary>,
        active: Option<TabId>,
        cx: &mut Context<Self>,
    ) {
        if self.tabs == tabs && self.active == active {
            return;
        }
        self.tabs = tabs;
        self.active = active;
        cx.notify();
    }

    fn state_color(&self, state: TabState, colors: &ThemeColor) -> Hsla {
        match state {
            TabState::Loading => colors.warning,
            TabState::Ready => colors.green,
            TabState::Error => colors.red,
        }
    }

    fn tab(
        &self,
        summary: &TabSummary,
        colors: &ThemeColor,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = summary.id;
        let active = self.active == Some(id);
        let select_this = cx.entity();
        let close_this = cx.entity();
        let state_color = self.state_color(summary.state, colors);

        let close = div()
            .id(SharedString::from(format!("repo-tab-close-{id}")))
            .size(px(14.))
            .rounded_sm()
            .flex()
            .items_center()
            .justify_center()
            .text_color(colors.muted_foreground)
            .hover(|el| el.bg(colors.list_hover))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(move |_event, _window, cx| {
                close_this.update(cx, |_bar, cx| {
                    cx.emit(RepoTabBarEvent::Close(id));
                });
            })
            .child(Icon::new(IconName::Close));

        h_flex()
            .id(SharedString::from(format!("repo-tab-{id}")))
            .h(px(24.))
            .max_w(px(220.))
            .px_2()
            .gap_1()
            .items_center()
            .rounded_md()
            .cursor(CursorStyle::PointingHand)
            .when(active, |el| el.bg(colors.input))
            .hover(|el| if active { el } else { el.bg(colors.list_hover) })
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(move |_event, _window, cx| {
                select_this.update(cx, |_bar, cx| {
                    cx.emit(RepoTabBarEvent::Select(id));
                });
            })
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(state_color)
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(if active {
                        colors.foreground
                    } else {
                        colors.muted_foreground
                    })
                    .child(shared(summary.title.clone())),
            )
            .child(close)
            .into_any_element()
    }
}

impl Default for RepoTabBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for RepoTabBar {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let tabs = self
            .tabs
            .iter()
            .map(|summary| self.tab(summary, &colors, cx))
            .collect::<Vec<_>>();
        let this = cx.entity();
        let new_tab = div()
            .id("repo-tab-new")
            .size(px(24.))
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::PointingHand)
            .text_size(px(17.))
            .text_color(colors.muted_foreground)
            .hover(|el| el.bg(colors.list_hover))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(move |_event, _window, cx| {
                log::info!("[workspace_tabs] new-tab button clicked");
                this.update(cx, |_bar, cx| {
                    cx.emit(RepoTabBarEvent::NewTab);
                });
            })
            .child(shared("+"));

        h_flex()
            .id("repo-tab-bar")
            .h_full()
            .flex_1()
            .min_w_0()
            .items_center()
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scrollbar()
                    .child(
                        h_flex()
                            .h_full()
                            .flex_none()
                            .items_center()
                            .gap_1()
                            .children(tabs)
                            .child(new_tab),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{fallback_after_close, should_refresh_after_switch};

    #[test]
    fn active_close_prefers_the_tab_to_the_right() {
        assert_eq!(fallback_after_close(&[1, 2, 3], Some(2), 2), Some(3));
    }

    #[test]
    fn active_close_falls_back_to_the_left_at_the_end() {
        assert_eq!(fallback_after_close(&[1, 2, 3], Some(3), 3), Some(2));
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_tab() {
        assert_eq!(fallback_after_close(&[1, 2, 3], Some(1), 3), Some(1));
    }

    #[test]
    fn switching_to_an_opened_tab_requests_a_refresh() {
        assert!(should_refresh_after_switch(true, true));
    }

    #[test]
    fn first_activation_uses_the_initial_load() {
        assert!(!should_refresh_after_switch(true, false));
    }

    #[test]
    fn activating_the_current_tab_does_not_refresh() {
        assert!(!should_refresh_after_switch(false, true));
    }
}
