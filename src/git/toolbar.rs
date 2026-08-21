//! M1：Toolbar 工具栏（镜像 rgitui toolbar.rs）
//!
//! 左组：Fetch/Pull/Push + ahead/behind 徽标 + 分支按钮（M2 打开分支面板）
//! 右组：刷新/设置（设置 M2 占位）
//! 动作经 ToolbarEvent 事件链下发 Workspace → GitCommand（git 子进程后台执行）

use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, h_flex};

use crate::core::i18n::{self, Locale};
use crate::git::shared;

/// Toolbar → Workspace 事件
#[derive(Clone, Debug)]
pub enum ToolbarEvent {
    Fetch,
    Pull,
    Push,
    Branch,
    Refresh,
    Settings,
}

pub struct Toolbar {
    ahead: usize,
    behind: usize,
    /// 是否有远程（无远程时 fetch/pull/push 禁用）
    has_remote: bool,
    /// 操作进行中（按钮禁用/转圈占位）
    busy: bool,
    /// 界面语言（Workspace 切换语言时同步）
    locale: Locale,
}

impl EventEmitter<ToolbarEvent> for Toolbar {}

impl Toolbar {
    pub fn new(locale: Locale) -> Self {
        Self {
            ahead: 0,
            behind: 0,
            has_remote: true,
            busy: false,
            locale,
        }
    }

    /// 切换语言（Workspace::set_language 同步）
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    pub fn set_ahead_behind(&mut self, ahead: usize, behind: usize, cx: &mut Context<Self>) {
        if self.ahead == ahead && self.behind == behind {
            return;
        }
        self.ahead = ahead;
        self.behind = behind;
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    /// 工具按钮（文本按钮，带 hover）；label_key 为 i18n 键
    fn tool_button(
        &self,
        id: &'static str,
        label_key: &str,
        colors: &gpui_component::theme::ThemeColor,
        enabled: bool,
        event: ToolbarEvent,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let this = cx.entity();
        div()
            .id(id)
            .px_2()
            .py_0p5()
            .rounded_md()
            .bg(colors.input)
            .opacity(if enabled { 1.0 } else { 0.45 })
            .hover(|this| this.bg(colors.list_hover))
            .text_size(px(12.))
            .text_color(colors.foreground)
            .child(shared(i18n::text(self.locale, label_key)))
            .when(enabled, |el| {
                el.on_click(move |_e, _w, cx| {
                    this.update(cx, |_toolbar, cx| cx.emit(event.clone()));
                })
            })
    }
}

impl Render for Toolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let enabled = self.has_remote && !self.busy;

        h_flex()
            .id("toolbar")
            .w_full()
            .h(px(32.))
            .flex_shrink_0()
            .px_2()
            .gap_1()
            .items_center()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .child(self.tool_button(
                "tb-fetch",
                "toolbar-fetch",
                &colors,
                enabled,
                ToolbarEvent::Fetch,
                cx,
            ))
            .child(self.tool_button(
                "tb-pull",
                "toolbar-pull",
                &colors,
                enabled,
                ToolbarEvent::Pull,
                cx,
            ))
            .child(self.tool_button(
                "tb-push",
                "toolbar-push",
                &colors,
                enabled,
                ToolbarEvent::Push,
                cx,
            ))
            .child(self.tool_button(
                "tb-branch",
                "toolbar-branch",
                &colors,
                true,
                ToolbarEvent::Branch,
                cx,
            ))
            // ahead/behind 徽标
            .child(
                h_flex()
                    .gap_1()
                    .px_2()
                    .child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(colors.input)
                            .text_size(px(11.))
                            .text_color(colors.green)
                            .child(shared(format!("↑{}", self.ahead))),
                    )
                    .child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(colors.input)
                            .text_size(px(11.))
                            .text_color(colors.red)
                            .child(shared(format!("↓{}", self.behind))),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .child(if self.busy {
                        shared(i18n::text(self.locale, "toolbar-busy"))
                    } else {
                        shared("")
                    }),
            )
            .child(self.tool_button(
                "tb-refresh",
                "toolbar-refresh",
                &colors,
                true,
                ToolbarEvent::Refresh,
                cx,
            ))
            .child(self.tool_button(
                "tb-settings",
                "toolbar-settings",
                &colors,
                true,
                ToolbarEvent::Settings,
                cx,
            ))
    }
}
