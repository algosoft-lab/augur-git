//! M1：右面板三件套——DetailPanel（提交/文件详情）、CommitPanel（提交输入）、
//! DiffViewer（底部面板 Diff tab，显示 git show 输出）
//!
//! 镜像 rgitui 的 detail_panel.rs / commit_panel.rs 职责，M1 渲染从简：
//! - DetailPanel：选中提交 → oid/作者/时间/消息/装饰；选中文件 → 路径/状态
//! - CommitPanel：多行输入 + 提交按钮（经事件链跑 git commit -m）
//! - DiffViewer：等宽文本展示 git show --stat 输出（高亮 diff 后续里程碑）

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, h_flex,
    input::{Input, InputEvent, InputState},
    theme::ThemeColor,
    v_flex,
};

use crate::git::shared;

/// CommitPanel → Workspace 事件
#[derive(Clone, Debug)]
pub enum CommitPanelEvent {
    /// 提交（git commit -m）
    Submit(String),
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

/// 底部面板 tab（镜像 rgitui 的 BottomPanelMode）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomPanelMode {
    Diff,
    History,
    Blame,
}

/// 右面板：tab 栏 + 详情
pub struct DetailPanel {
    content: DetailContent,
    pub mode: RightPanelMode,
}

impl DetailPanel {
    pub fn new() -> Self {
        Self {
            content: DetailContent::Empty,
            mode: RightPanelMode::Details,
        }
    }

    pub fn set_content(&mut self, content: DetailContent, cx: &mut Context<Self>) {
        self.content = content;
        cx.notify();
    }

    fn detail_tab(&self, colors: &ThemeColor, label: &str, active: bool) -> Stateful<Div> {
        h_flex()
            .id(SharedString::from(format!("right-tab-{label}")))
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
                    .child(shared(label)),
            )
    }
}

impl Render for DetailPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let mono = cx.theme().mono_font_family.clone();

        // tab 切换（M1：BranchHealth 为占位）
        let this = cx.entity();
        let tab_details = self.detail_tab(&colors, "详情", self.mode == RightPanelMode::Details);
        let tab_details = tab_details.on_click(move |_e, _w, cx| {
            this.update(cx, |panel, cx| {
                panel.mode = RightPanelMode::Details;
                cx.notify();
            });
        });
        let this = cx.entity();
        let tab_bh = self.detail_tab(
            &colors,
            "分支概览",
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
                DetailContent::Empty => self.empty_view(&colors).into_any_element(),
                DetailContent::Commit {
                    short,
                    subject,
                    author,
                    date,
                    decorations,
                } => self
                    .commit_view(&colors, &mono, short, subject, author, date, decorations)
                    .into_any_element(),
                DetailContent::File { path, staged, code } => self
                    .file_view(&colors, &mono, path, *staged, *code)
                    .into_any_element(),
            })
    }
}

impl DetailPanel {
    fn empty_view(&self, colors: &ThemeColor) -> impl IntoElement {
        v_flex()
            .id("detail-empty")
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(colors.muted_foreground)
                    .child("选择提交或文件查看详情"),
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
                            .child(shared(format!("作者 {author}"))),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(format!("时间 {date}"))),
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
        let (label, color) = match code {
            'M' => ("修改", colors.warning),
            'A' => ("新增", colors.green),
            'D' => ("删除", colors.red),
            'R' => ("重命名", colors.warning),
            'U' => ("冲突", colors.red),
            _ => ("未跟踪", colors.muted_foreground),
        };
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
                            .child(if staged { "已暂存" } else { "未暂存" }),
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
}

impl EventEmitter<CommitPanelEvent> for CommitPanel {}

impl CommitPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("提交信息（Enter 提交）"));

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
        }
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child("提交"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("commit-collapse")
                    .px_1()
                    .rounded_md()
                    .hover(|this| this.bg(colors.list_hover))
                    .text_size(px(12.))
                    .text_color(colors.muted_foreground)
                    .child(if self.collapsed { "▴" } else { "▾" })
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

        // 提交按钮（Ctrl+Enter 或点击）
        let btn_commit = cx.entity();
        let commit_btn = div()
            .id("btn-commit")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(if self.has_staged {
                Hsla::from(rgb(0x2F_81_F7))
            } else {
                colors.input
            })
            .text_color(if self.has_staged {
                gpui::white()
            } else {
                colors.muted_foreground
            })
            .text_size(px(12.))
            .child("提交")
            .on_click(move |_e, _w, cx| {
                btn_commit.update(cx, |panel, cx| panel.submit(cx));
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
                                    .child(if self.has_staged {
                                        "将提交暂存的变更"
                                    } else {
                                        "无暂存变更（暂存功能 M2）"
                                    }),
                            )
                            .child(div().flex_1())
                            .child(commit_btn),
                    ),
            )
    }
}

/// 底部面板：Diff tab（git show 输出，等宽文本）
pub struct DiffViewer {
    /// (标题, 输出文本)
    output: Option<(String, String)>,
    /// 输出是否来自失败命令（错误红字）
    failed: bool,
}

impl DiffViewer {
    pub fn new() -> Self {
        Self {
            output: None,
            failed: false,
        }
    }

    /// 显示命令输出（workspace 从 CommandDone 转发）
    pub fn set_output(
        &mut self,
        label: String,
        message: String,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        self.output = Some((label, message));
        self.failed = !success;
        cx.notify();
    }
}

impl Render for DiffViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let Some((label, text)) = &self.output else {
            return v_flex()
                .id("diff-empty")
                .size_full()
                .items_center()
                .justify_center()
                .bg(colors.background)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.muted_foreground)
                        .child("双击提交或点文件行 ✎ 查看 diff"),
                )
                .into_any_element();
        };

        // 按行拆分渲染（等宽字体；本 fork 无 whitespace_pre_wrap，逐行 child 最稳）
        let mono = cx.theme().mono_font_family.clone();
        let lines = text
            .lines()
            .map(|l| {
                div()
                    .w_full()
                    .text_size(px(12.))
                    .text_color(if self.failed {
                        colors.red
                    } else {
                        colors.foreground
                    })
                    .child(shared(if l.is_empty() { " " } else { l }))
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("diff-viewer")
            .size_full()
            .bg(colors.background)
            .child(
                h_flex()
                    .id("diff-header")
                    .w_full()
                    .h(px(24.))
                    .flex_shrink_0()
                    .px_3()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(if self.failed {
                                colors.red
                            } else {
                                colors.muted_foreground
                            })
                            .child(shared(format!("$ git {label}"))),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(if self.failed {
                                colors.red
                            } else {
                                colors.green
                            })
                            .child(if self.failed { "失败" } else { "成功" }),
                    ),
            )
            .child(
                v_flex()
                    .id("diff-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_3()
                    .py_2()
                    .font_family(mono)
                    .children(if lines.is_empty() {
                        vec![
                            div()
                                .text_size(px(12.))
                                .text_color(colors.muted_foreground)
                                .child("(无输出)"),
                        ]
                    } else {
                        lines
                    }),
            )
            .into_any_element()
    }
}
