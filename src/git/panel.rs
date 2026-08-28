//! Presentation panels for commit details and commit input.
//!
//! The bottom commit diff is implemented in bottom_panel.rs; this module
//! re-exports its public panel types so existing workspace wiring remains
//! stable.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, h_flex,
    input::{Input, InputEvent, InputState},
    theme::ThemeColor,
    v_flex,
};

use crate::core::i18n::{self, Locale};
use crate::git::shared;

pub use crate::git::bottom_panel::{BottomPanel, BottomPanelEvent};

/// CommitPanel → Workspace 事件
#[derive(Clone, Debug)]
pub enum CommitPanelEvent {
    /// 提交（git commit -m）
    Submit(String),
}

/// 统一空态：24px muted 图标 + 一行 11px 提示（居中；图标可为内置 IconName 或本地 lucide）
fn empty_state(
    id: &'static str,
    colors: &ThemeColor,
    icon: AnyElement,
    hint: String,
) -> Stateful<Div> {
    v_flex()
        .id(id)
        .size_full()
        .items_center()
        .justify_center()
        .gap_1()
        .child(
            div()
                .size(px(24.))
                .text_color(colors.muted_foreground)
                .child(icon),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(colors.muted_foreground)
                .child(shared(hint)),
        )
}

/// 详情面板内容（Workspace 设置）
#[derive(Clone, Debug, Default)]
pub enum DetailContent {
    #[default]
    Empty,
    Commit {
        short: String,
        subject: String,
        author: String,
        date: String,
        decorations: String,
    },
    File {
        path: String,
        staged: bool,
        code: char,
    },
}

/// 右面板 tab（镜像 rgitui 的 RightPanelMode）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPanelMode {
    Details,
    BranchHealth,
}

/// 右面板：tab 栏 + 详情
pub struct DetailPanel {
    content: DetailContent,
    pub mode: RightPanelMode,
    /// 界面语言（Workspace 切换语言时同步）
    locale: Locale,
}

impl DetailPanel {
    pub fn new(locale: Locale) -> Self {
        Self {
            content: DetailContent::Empty,
            mode: RightPanelMode::Details,
            locale,
        }
    }

    /// 切换语言（Workspace::set_language 同步）
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    pub fn set_content(
        &mut self,
        content: DetailContent,
        cx: &mut Context<Self>,
    ) {
        self.content = content;
        cx.notify();
    }

    /// tab（key 为稳定 i18n 键作 id；title 为本地化文本）
    fn detail_tab(
        &self,
        colors: &ThemeColor,
        key: &str,
        title: String,
        active: bool,
    ) -> Stateful<Div> {
        h_flex()
            .id(SharedString::from(format!("right-tab-{key}")))
            .h_full()
            .px_3()
            .items_center()
            .cursor(CursorStyle::PointingHand)
            .when(active, |el| el.border_b_2().border_color(colors.accent))
            .hover(|s| s.bg(colors.list_hover))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(if active {
                        colors.foreground
                    } else {
                        colors.muted_foreground
                    })
                    .child(shared(title)),
            )
    }
}

impl Render for DetailPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let mono = cx.theme().mono_font_family.clone();

        // tab 切换（M1：BranchHealth 为占位）
        let this = cx.entity();
        let tab_details = self.detail_tab(
            &colors,
            "tab-details",
            i18n::text(self.locale, "tab-details"),
            self.mode == RightPanelMode::Details,
        );
        let tab_details = tab_details.on_click(move |_e, _w, cx| {
            this.update(cx, |panel, cx| {
                panel.mode = RightPanelMode::Details;
                cx.notify();
            });
        });
        let this = cx.entity();
        let tab_bh = self.detail_tab(
            &colors,
            "tab-branch-health",
            i18n::text(self.locale, "tab-branch-health"),
            self.mode == RightPanelMode::BranchHealth,
        );
        let tab_bh = tab_bh.on_click(move |_e, _w, cx| {
            this.update(cx, |panel, cx| {
                panel.mode = RightPanelMode::BranchHealth;
                cx.notify();
            });
        });

        v_flex()
            .id("detail-panel")
            .size_full()
            .bg(colors.background)
            .child(
                h_flex()
                    .id("detail-tab-bar")
                    .w_full()
                    .h(px(26.))
                    .flex_shrink_0()
                    .bg(colors.tab_bar)
                    .border_b_1()
                    .border_color(colors.border)
                    .items_end()
                    .gap_1()
                    .px_2()
                    .child(tab_details)
                    .child(tab_bh)
                    .child(div().flex_1()),
            )
            .child(match &self.content {
                DetailContent::Empty => {
                    self.empty_view(&colors).into_any_element()
                }
                DetailContent::Commit {
                    short,
                    subject,
                    author,
                    date,
                    decorations,
                } => self
                    .commit_view(
                        &colors,
                        &mono,
                        short,
                        subject,
                        author,
                        date,
                        decorations,
                    )
                    .into_any_element(),
                DetailContent::File { path, staged, code } => self
                    .file_view(&colors, &mono, path, *staged, *code)
                    .into_any_element(),
            })
    }
}

impl DetailPanel {
    fn empty_view(&self, colors: &ThemeColor) -> impl IntoElement {
        empty_state(
            "detail-empty",
            colors,
            Icon::new(IconName::Inbox).into_any_element(),
            i18n::text(self.locale, "detail-empty"),
        )
    }

    fn commit_view(
        &self,
        colors: &ThemeColor,
        mono: &SharedString,
        short: &str,
        subject: &str,
        author: &str,
        date: &str,
        decorations: &str,
    ) -> impl IntoElement {
        v_flex()
            .id("detail-commit")
            .w_full()
            .gap_2()
            .p_3()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(colors.input)
                            .font_family(mono.clone())
                            .text_size(px(12.))
                            .text_color(colors.accent)
                            .child(shared(short)),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(colors.muted_foreground)
                            .child(shared(decorations)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(13.))
                    .text_color(colors.foreground)
                    .child(shared(subject)),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text_args(
                                self.locale,
                                "detail-author",
                                &[("author", author)],
                            ))),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text_args(
                                self.locale,
                                "detail-date",
                                &[("date", date)],
                            ))),
                    ),
            )
    }

    fn file_view(
        &self,
        colors: &ThemeColor,
        mono: &SharedString,
        path: &str,
        staged: bool,
        code: char,
    ) -> impl IntoElement {
        let (key, color) = match code {
            'M' => ("file-modified", colors.warning),
            'A' => ("file-added", colors.green),
            'D' => ("file-deleted", colors.red),
            'R' => ("file-renamed", colors.warning),
            'U' => ("file-conflict", colors.red),
            _ => ("file-untracked", colors.muted_foreground),
        };
        let label = i18n::text(self.locale, key);
        v_flex()
            .id("detail-file")
            .w_full()
            .gap_2()
            .p_3()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(colors.input)
                            .text_size(px(11.))
                            .text_color(color)
                            .child(shared(label)),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(if staged {
                                i18n::text(self.locale, "file-staged")
                            } else {
                                i18n::text(self.locale, "file-unstaged")
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .font_family(mono.clone())
                    .text_size(px(12.))
                    .text_color(colors.foreground)
                    .child(shared(path)),
            )
    }
}

/// 提交输入面板
pub struct CommitPanel {
    input: Entity<InputState>,
    collapsed: bool,
    /// 是否有暂存变更（无暂存时提交按钮禁用）
    has_staged: bool,
    /// 界面语言（Workspace 切换语言时同步）
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
            InputState::new(window, cx)
                .placeholder(i18n::text(locale, "commit-placeholder"))
        });

        // Ctrl+Enter = 提交
        let input_entity = input.clone();
        cx.subscribe(&input_entity, |panel, _e, event, cx| {
            if matches!(
                event,
                InputEvent::PressEnter {
                    secondary: false,
                    ..
                }
            ) {
                let msg = panel.input.read(cx).value().to_string();
                if !msg.trim().is_empty() {
                    cx.emit(CommitPanelEvent::Submit(msg));
                }
            }
        })
        .detach();

        Self {
            input,
            collapsed: false,
            has_staged: false,
            locale,
        }
    }

    /// 切换语言（Workspace::set_language 同步）；placeholder 回填需 &mut Window
    pub fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        let placeholder = i18n::text(locale, "commit-placeholder");
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
        cx.notify();
    }

    pub fn set_has_staged(&mut self, has_staged: bool, cx: &mut Context<Self>) {
        if self.has_staged != has_staged {
            self.has_staged = has_staged;
            cx.notify();
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let msg = self.input.read(cx).value().to_string();
        if msg.trim().is_empty() {
            return;
        }
        // 提交后清空输入框（需要 window，subscribe 里没有——由 workspace 完成后通知）
        cx.emit(CommitPanelEvent::Submit(msg));
    }
}

impl Render for CommitPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();

        // 标题行：提交 + 收起
        let this = cx.entity();
        let header = h_flex()
            .id("commit-header")
            .w_full()
            .h(px(26.))
            .flex_shrink_0()
            .px_2()
            .items_center()
            .gap_2()
            .bg(colors.tab_bar)
            .border_t_1()
            .border_color(colors.border)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(self.locale, "commit-title"))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("commit-collapse")
                    .p_1()
                    .rounded_md()
                    .hover(|this| this.bg(colors.list_hover))
                    .text_size(px(12.))
                    .text_color(colors.muted_foreground)
                    .child(if self.collapsed {
                        Icon::new(IconName::ChevronDown)
                    } else {
                        Icon::new(IconName::ChevronUp)
                    })
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |panel, cx| {
                            panel.collapsed = !panel.collapsed;
                            cx.notify();
                        });
                    }),
            );

        if self.collapsed {
            return v_flex()
                .id("commit-panel")
                .w_full()
                .flex_shrink_0()
                .child(header);
        }

        // 提交按钮：无 staged 时不挂 on_click（灰态即禁用，杜绝空提交误触）
        let btn_commit = cx.entity();
        let commit_btn = div()
            .id("btn-commit")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(if self.has_staged {
                colors.blue
            } else {
                colors.input
            })
            .text_color(if self.has_staged {
                colors.primary_foreground
            } else {
                colors.muted_foreground
            })
            .text_size(px(12.))
            .child(shared(i18n::text(self.locale, "commit-btn")))
            .when(self.has_staged, |btn| {
                btn.on_click(move |_e, _w, cx| {
                    btn_commit.update(cx, |panel, cx| panel.submit(cx));
                })
            });

        v_flex()
            .id("commit-panel")
            .w_full()
            .flex_shrink_0()
            .child(header)
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .p_2()
                    .child(Input::new(&self.input).w_full().h_7())
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(colors.muted_foreground)
                                    .child(shared(if self.has_staged {
                                        i18n::text(
                                            self.locale,
                                            "commit-hint-staged",
                                        )
                                    } else {
                                        i18n::text(
                                            self.locale,
                                            "commit-hint-none",
                                        )
                                    })),
                            )
                            .child(div().flex_1())
                            .child(commit_btn),
                    ),
            )
    }
}
