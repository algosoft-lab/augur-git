//! M1：Toolbar 工具栏（镜像 rgitui toolbar.rs；M1.5 图标化 ghost 风格）
//!
//! 左组：Fetch/Pull/Push（图标+文字）+ ahead/behind 徽标 + Branch
//! 右组：busy Spinner / 刷新 / 设置
//! 动作经 ToolbarEvent 事件链下发 Workspace → GitCommand（git 子进程后台执行）

use gpui::prelude::*;
use gpui::*;
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable, h_flex};

use crate::core::i18n::{self, Locale};
use crate::git::{lucide, shared};

/// Toolbar → Workspace 事件
#[derive(Clone, Debug)]
pub enum ToolbarEvent {
    Fetch,
    PullMerge,
    PullRebase,
    Push,
    PushForce,
    Branch,
    Compare,
    Refresh,
    Settings,
}

pub struct Toolbar {
    ahead: usize,
    behind: usize,
    /// 是否有远程（无远程时 fetch/pull/push 禁用）
    has_remote: bool,
    /// 操作进行中（右侧 Spinner）
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

    pub fn set_ahead_behind(
        &mut self,
        ahead: usize,
        behind: usize,
        cx: &mut Context<Self>,
    ) {
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

    /// 工具按钮：图标(14px)+文字(12px) ghost 风格（无常态底色，hover 提亮）
    fn tool_button(
        &self,
        id: &'static str,
        icon: Icon,
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
            .opacity(if enabled { 1.0 } else { 0.45 })
            .hover(|this| this.bg(colors.list_hover))
            .text_size(px(12.))
            .text_color(colors.foreground)
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(colors.muted_foreground)
                            .child(icon),
                    )
                    .child(shared(i18n::text(self.locale, label_key))),
            )
            .when(enabled, |el| {
                el.on_click(move |_e, _w, cx| {
                    this.update(cx, |_toolbar, cx| cx.emit(event.clone()));
                })
            })
    }

    /// ahead/behind 徽标（箭头图标 + 计数，11px 微字号）
    fn count_badge(
        &self,
        id: &'static str,
        icon: IconName,
        n: usize,
        color: Hsla,
        colors: &gpui_component::theme::ThemeColor,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_1()
            .py_0p5()
            .rounded_sm()
            .bg(colors.input)
            .child(
                h_flex()
                    .gap_0p5()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(color)
                    .child(div().text_size(px(11.)).child(Icon::new(icon)))
                    .child(shared(format!("{n}"))),
            )
    }
}

impl Render for Toolbar {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                lucide("download"),
                "toolbar-fetch",
                &colors,
                enabled,
                ToolbarEvent::Fetch,
                cx,
            ))
            .child(self.tool_button(
                "tb-pull-merge",
                Icon::new(IconName::ArrowDown),
                "toolbar-pull-merge",
                &colors,
                enabled,
                ToolbarEvent::PullMerge,
                cx,
            ))
            .child(self.tool_button(
                "tb-pull-rebase",
                lucide("git-commit-horizontal"),
                "toolbar-pull-rebase",
                &colors,
                enabled,
                ToolbarEvent::PullRebase,
                cx,
            ))
            .child(self.tool_button(
                "tb-push",
                Icon::new(IconName::ArrowUp),
                "toolbar-push",
                &colors,
                enabled,
                ToolbarEvent::Push,
                cx,
            ))
            .child(self.tool_button(
                "tb-push-force",
                Icon::new(IconName::TriangleAlert),
                "toolbar-push-force",
                &colors,
                enabled,
                ToolbarEvent::PushForce,
                cx,
            ))
            .child(self.tool_button(
                "tb-branch",
                lucide("git-branch"),
                "toolbar-branch",
                &colors,
                true,
                ToolbarEvent::Branch,
                cx,
            ))
            .child(self.tool_button(
                "tb-compare",
                lucide("git-branch"),
                "toolbar-compare",
                &colors,
                !self.busy,
                ToolbarEvent::Compare,
                cx,
            ))
            // ahead/behind 徽标
            .child(self.count_badge(
                "tb-ahead",
                IconName::ArrowUp,
                self.ahead,
                colors.green,
                &colors,
            ))
            .child(self.count_badge(
                "tb-behind",
                IconName::ArrowDown,
                self.behind,
                colors.red,
                &colors,
            ))
            .child(div().flex_1())
            // 忙碌指示（动画 Spinner 替代静态文字）
            .when(self.busy, |el| {
                el.child(Spinner::new().with_size(px(14.)).color(colors.blue))
            })
            .child(self.tool_button(
                "tb-refresh",
                lucide("refresh-cw"),
                "toolbar-refresh",
                &colors,
                true,
                ToolbarEvent::Refresh,
                cx,
            ))
            .child(self.tool_button(
                "tb-settings",
                Icon::new(IconName::Settings),
                "toolbar-settings",
                &colors,
                true,
                ToolbarEvent::Settings,
                cx,
            ))
    }
}
