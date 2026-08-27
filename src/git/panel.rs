//! M1：右面板三件套——DetailPanel（提交/文件详情）、CommitPanel（提交输入）、
//! BottomPanel（底部面板：选中提交文件清单 + 单文件 diff 分栏）
//!
//! 镜像 rgitui 的 detail_panel.rs / commit_panel.rs 职责，M1 渲染从简：
//! - DetailPanel：选中提交 → oid/作者/时间/消息/装饰；选中文件 → 路径/状态
//! - CommitPanel：多行输入 + 提交按钮（经事件链跑 git commit -m）
//! - BottomPanel：左 40% 文件清单（每文件 `+n −m` 绿/红色块条，镜像 rgitui
//!   DiffStat）+ 右 60% 染色 diff（+绿/−红/@@蓝），无 tab

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, h_flex,
    input::{Input, InputEvent, InputState},
    theme::ThemeColor,
    v_flex,
};

use crate::core::git::{
    DiffLine, DiffLineKind, FileChange, parse_diff, stat_blocks,
};
use crate::core::i18n::{self, Locale};
use crate::git::{lucide, shared};

/// CommitPanel → Workspace 事件
#[derive(Clone, Debug)]
pub enum CommitPanelEvent {
    /// 提交（git commit -m）
    Submit(String),
}

/// hunk 头紫色（GitHub Dark diff 主题色，主题 token 无 purple 字段）
fn diff_purple() -> Hsla {
    Hsla::from(rgb(0xBC8CFF))
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
                gpui::white()
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

/// BottomPanel → Workspace 事件
#[derive(Clone, Debug)]
pub enum BottomPanelEvent {
    /// 选中文件 → 右侧加载该文件在此提交的 diff（workspace 转发 GitView）
    ShowFileDiff { oid: String, path: String },
}

/// 底部面板：选中提交的文件清单 + 单文件 diff 分栏（无 tab）
///
/// 布局：头行（short oid 徽标 + 说明截断 + 总增删色块条）+ 左右分栏
/// （左 40% 文件清单：路径 + 每文件 `+n −m` 绿/红色块条；右 60% 染色 diff）。
/// 快速切换提交/文件时，过期结果按 oid/path 校验丢弃。
pub struct BottomPanel {
    locale: Locale,
    /// 当前选中提交 (oid, short, subject)；切换时重置清单与 diff
    commit: Option<(String, String, String)>,
    /// 提交的逐文件增删统计（git show --numstat）
    files: Vec<FileChange>,
    /// 选中文件索引
    selected: Option<usize>,
    /// 右侧 diff 文本（与 selected 对应；None = 未选/未加载）
    diff: Option<String>,
}

impl EventEmitter<BottomPanelEvent> for BottomPanel {}

impl BottomPanel {
    pub fn new(locale: Locale) -> Self {
        Self {
            locale,
            commit: None,
            files: Vec::new(),
            selected: None,
            diff: None,
        }
    }

    /// 切换语言（Workspace::set_language 同步）
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    /// 选中提交变化（GraphEvent::CommitSelected 转发）：重置清单/diff；
    /// workspace 随后触发 numstat 查询
    pub fn set_commit(
        &mut self,
        oid: &str,
        short: &str,
        subject: &str,
        cx: &mut Context<Self>,
    ) {
        self.commit =
            Some((oid.to_string(), short.to_string(), subject.to_string()));
        self.files.clear();
        self.selected = None;
        self.diff = None;
        cx.notify();
    }

    /// 文件清单到达（oid 与当前提交不符 = 过期结果，丢弃）
    pub fn set_files(
        &mut self,
        oid: &str,
        files: Vec<FileChange>,
        cx: &mut Context<Self>,
    ) {
        if self
            .commit
            .as_ref()
            .map_or(true, |(current, _, _)| current != oid)
        {
            return;
        }
        self.files = files;
        self.selected = None;
        self.diff = None;
        cx.notify();
    }

    /// 文件 diff 到达（oid/path 与当前选中不符 = 过期结果，丢弃）
    pub fn set_diff(
        &mut self,
        oid: &str,
        path: &str,
        diff: String,
        cx: &mut Context<Self>,
    ) {
        let selected_path = self
            .selected
            .and_then(|i| self.files.get(i))
            .map(|f| f.path.as_str());
        let stale = self
            .commit
            .as_ref()
            .map_or(true, |(current, _, _)| current != oid)
            || selected_path != Some(path);
        if stale {
            return;
        }
        self.diff = Some(diff);
        cx.notify();
    }

    /// 点击文件行：选中并请求该文件 diff（重复点击同文件不重复请求）
    fn select_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.selected == Some(index) {
            return;
        }
        let Some((oid, _, _)) = self.commit.clone() else {
            return;
        };
        let Some(file) = self.files.get(index) else {
            return;
        };
        self.selected = Some(index);
        self.diff = None;
        cx.emit(BottomPanelEvent::ShowFileDiff {
            oid,
            path: file.path.clone(),
        });
        cx.notify();
    }
}

impl BottomPanel {
    /// 左栏：文件清单（40% 宽；每行 路径 + 色块条，点击选中）
    fn file_list(
        &self,
        colors: &ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();

        // 清单为空：合并提交（numstat 无输出）或空提交
        if self.files.is_empty() {
            return empty_state(
                "bottom-files-empty",
                colors,
                Icon::new(IconName::Info).into_any_element(),
                i18n::text(self.locale, "bottom-merge-empty"),
            )
            .w(relative(0.4))
            .flex_shrink_0()
            .into_any_element();
        }

        let rows = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let this = cx.entity();
                let selected = self.selected == Some(i);
                // 二进制文件无行数：灰 BIN 字样代替色块条
                let stat = if f.is_binary() {
                    div()
                        .text_size(px(10.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(self.locale, "bottom-bin")))
                        .into_any_element()
                } else {
                    stat_bar(
                        colors,
                        f.added.unwrap_or(0),
                        f.deleted.unwrap_or(0),
                    )
                    .into_any_element()
                };
                h_flex()
                    .id(SharedString::from(format!("bottom-file-{i}")))
                    .w_full()
                    .h(px(22.))
                    .flex_shrink_0()
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .bg(if selected {
                        colors.list_active
                    } else {
                        colors.background
                    })
                    .hover(|this| {
                        if !selected {
                            this.bg(colors.list_hover)
                        } else {
                            this
                        }
                    })
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |panel, cx| panel.select_file(i, cx));
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(mono.clone())
                            .text_size(px(11.))
                            .text_color(colors.foreground)
                            .truncate()
                            .child(shared(f.path.clone())),
                    )
                    .child(stat)
            })
            .collect::<Vec<_>>();

        div()
            .id("bottom-files")
            .w(relative(0.4))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .py_1()
            .children(rows)
            .into_any_element()
    }

    /// 右栏：选中文件的染色 diff（parse_diff 驱动：双列行号 gutter + 类别染色，
    /// hunk 头紫字淡紫底通栏；+绿/−红 淡底；元信息灰）
    fn diff_view(
        &self,
        colors: &ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();

        let body: Vec<AnyElement> = match self.selected {
            None => vec![
                empty_state(
                    "bottom-diff-none",
                    colors,
                    Icon::new(IconName::File).into_any_element(),
                    i18n::text(self.locale, "bottom-no-file"),
                )
                .into_any_element(),
            ],
            Some(_) => match &self.diff {
                // 加载中（本地 git 毫秒级，短暂省略号）
                None => vec![
                    div()
                        .text_size(px(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared("…"))
                        .into_any_element(),
                ],
                Some(text) if text.trim().is_empty() => vec![
                    empty_state(
                        "bottom-diff-empty",
                        colors,
                        Icon::new(IconName::File).into_any_element(),
                        i18n::text(self.locale, "diff-no-output"),
                    )
                    .into_any_element(),
                ],
                // 逐行渲染（本 fork 无 whitespace_pre_wrap，此法最稳）
                Some(text) => parse_diff(text)
                    .iter()
                    .map(|l| diff_row(colors, &mono.clone(), l))
                    .collect(),
            },
        };

        div()
            .id("bottom-diff")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .py_1()
            .children(body)
            .into_any_element()
    }
}

/// diff 单行：[旧行号 32px][新行号 32px][内容 flex_1]；染色铺满整行含 gutter
fn diff_row(
    colors: &ThemeColor,
    mono: &SharedString,
    line: &DiffLine,
) -> AnyElement {
    let (fg, bg) = match line.kind {
        DiffLineKind::Add => (colors.green, Some(colors.green.opacity(0.12))),
        DiffLineKind::Del => (colors.red, Some(colors.red.opacity(0.12))),
        DiffLineKind::Hunk => (diff_purple(), Some(diff_purple().opacity(0.1))),
        DiffLineKind::Meta => (colors.muted_foreground, None),
        DiffLineKind::Context => (colors.foreground, None),
    };
    let gutter = |n: Option<u32>| -> Div {
        div()
            .w(px(32.))
            .flex_shrink_0()
            .px_1()
            .font_family(mono.clone())
            .text_size(px(11.))
            .text_color(colors.muted_foreground.opacity(0.7))
            .child(shared(match n {
                Some(n) => n.to_string(),
                None => String::new(),
            }))
    };
    h_flex()
        .w_full()
        .items_stretch()
        .when_some(bg, |el, b| el.bg(b))
        .text_color(fg)
        .child(gutter(line.old_no))
        .child(gutter(line.new_no))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px_2()
                .font_family(mono.clone())
                .text_size(px(12.))
                .child(shared(if line.text.is_empty() {
                    " ".to_string()
                } else {
                    line.text.clone()
                })),
        )
        .into_any_element()
}

impl Render for BottomPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();

        // 未选提交：整面板空态
        let Some((_, short, subject)) = &self.commit else {
            return empty_state(
                "bottom-no-commit",
                &colors,
                lucide("git-commit-horizontal").into_any_element(),
                i18n::text(self.locale, "bottom-no-commit"),
            )
            .into_any_element();
        };

        // 头行：short oid 徽标 + 说明 + 总增删色块条（二进制不计入）
        let (total_add, total_del) =
            self.files.iter().fold((0, 0), |(a, d), f| {
                (a + f.added.unwrap_or(0), d + f.deleted.unwrap_or(0))
            });
        let header = h_flex()
            .id("bottom-header")
            .w_full()
            .h(px(24.))
            .flex_shrink_0()
            .px_2()
            .gap_2()
            .items_center()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .px_2()
                    .rounded_sm()
                    .bg(colors.input)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(11.))
                    .text_color(colors.accent)
                    .child(shared(short.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .truncate()
                    .child(shared(subject.clone())),
            )
            .child(stat_bar(&colors, total_add, total_del));

        v_flex()
            .id("bottom-panel")
            .size_full()
            .bg(colors.background)
            .child(header)
            .child(
                // 左右分栏（h_flex 强制 items_center，容器改用显式 flex_row）
                div()
                    .id("bottom-body")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(self.file_list(&colors, cx))
                    // 分隔线（1px，不参与弹性布局）
                    .child(
                        div()
                            .w(px(1.))
                            .flex_shrink_0()
                            .h_full()
                            .bg(colors.border),
                    )
                    .child(self.diff_view(&colors, cx)),
            )
            .into_any_element()
    }
}

/// 增删色块条（镜像 rgitui DiffStat）：`+n`/`−m` 数字 + 5 块 4×10px 圆角小矩形，
/// 绿块数按 stat_blocks 比例分配，零变更全灰
fn stat_bar(colors: &ThemeColor, added: usize, deleted: usize) -> Div {
    let (green, red) = stat_blocks(added, deleted);
    let mut bar = h_flex().gap(px(1.)).items_center();
    for i in 0..5 {
        let color = if i < green {
            colors.green
        } else if i < green + red {
            colors.red
        } else {
            colors.muted_foreground.opacity(0.5)
        };
        bar = bar.child(div().w(px(4.)).h(px(10.)).rounded(px(1.)).bg(color));
    }
    h_flex()
        .gap_1()
        .items_center()
        .flex_shrink_0()
        .child(
            div()
                .text_size(px(10.))
                .text_color(colors.green)
                .child(shared(format!("+{added}"))),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(colors.red)
                .child(shared(format!("-{deleted}"))),
        )
        .child(bar)
}
