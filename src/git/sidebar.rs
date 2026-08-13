//! M1：Sidebar 侧栏——仓库路径/打开 + 本地分支分区 + 变更文件分区
//!
//! 镜像 rgitui sidebar 的分区结构（本 M1 实现 分支/暂存/未暂存 三区）：
//! - 分支区：点击选中 → 点击行内 checkout 按钮切换分支
//! - 暂存/未暂存区：文件行点击 → 详情面板显示；行内 ✎ 按钮 → diff
//! 顶部保留 M0 的仓库路径输入 + 打开/刷新（多仓库切换入口）

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, h_flex, input::{Input, InputState}, v_flex,
};

use crate::core::config::AppConfig;
use crate::core::git::{BranchInfo, FileStatus};
use crate::git::shared;

/// 侧栏事件
pub enum SidebarEvent {
    /// 用输入框路径打开仓库
    OpenRepo,
    /// 刷新仓库快照
    Refresh,
    /// 打开最近仓库（输入框已回填）
    OpenRecent(String),
    /// 收起/展开侧栏
    ToggleCollapse,
    /// 选中分支（详情面板显示）
    BranchSelected(String),
    /// 切换分支（git checkout）
    CheckoutBranch(String),
    /// 选中变更文件（详情面板显示）
    FileSelected {
        path: String,
        staged: bool,
        code: char,
    },
    /// 查看文件 diff（git show/HEAD diff）
    FileDiff { path: String, staged: bool },
}

pub struct Sidebar {
    repo_path_input: Entity<InputState>,
    recent_repos: Vec<String>,
    /// 本地分支列表
    branches: Vec<BranchInfo>,
    /// 当前分支
    branch: String,
    /// 变更文件（staged/unstaged 已分类）
    staged: Vec<FileStatus>,
    unstaged: Vec<FileStatus>,
    /// 选中行（(是否暂存, 索引)）
    selected: Option<(bool, usize)>,
    /// 分支区高亮截止时刻（标题栏徽标点击，时间戳过期自动消失）
    flash_branches_until: Option<Instant>,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Sidebar {
    /// 输入框由 Workspace 创建（Welcome 页共用），此处只接收引用
    pub fn new(
        _window: &mut Window,
        _cx: &mut Context<Self>,
        config: &AppConfig,
        repo_path_input: &Entity<InputState>,
    ) -> Self {
        Self {
            repo_path_input: repo_path_input.clone(),
            recent_repos: config.recent_repos.clone(),
            branches: Vec::new(),
            branch: String::new(),
            staged: Vec::new(),
            unstaged: Vec::new(),
            selected: None,
            flash_branches_until: None,
        }
    }

    /// 当前输入框仓库路径
    pub fn repo_path(&self, cx: &App) -> String {
        self.repo_path_input.read(cx).value().to_string()
    }

    /// 刷新最近仓库列表（打开成功后由 workspace 调用）
    pub fn set_recent(&mut self, recent: Vec<String>) {
        self.recent_repos = recent;
    }

    /// 接收仓库状态快照（workspace 从 GitUiEvent 转发）
    pub fn set_status(
        &mut self,
        branch: String,
        branches: Vec<BranchInfo>,
        files: Vec<FileStatus>,
        cx: &mut Context<Self>,
    ) {
        self.branch = branch;
        self.branches = branches;
        self.staged = files.iter().filter(|f| f.is_staged()).cloned().collect();
        self.unstaged = files.iter().filter(|f| !f.is_staged()).cloned().collect();
        // 选中行可能已消失
        if let Some((staged, i)) = self.selected {
            let list = if staged { &self.staged } else { &self.unstaged };
            if i >= list.len() {
                self.selected = None;
            }
        }
        cx.notify();
    }

    /// 标题栏分支徽标点击：高亮分支区 800ms
    pub fn flash_branches(&mut self, cx: &mut Context<Self>) {
        self.flash_branches_until = Some(Instant::now() + Duration::from_millis(800));
        cx.notify();
    }

    fn select_file(&mut self, staged: bool, index: usize, cx: &mut Context<Self>) {
        let list = if staged { &self.staged } else { &self.unstaged };
        let Some(file) = list.get(index) else {
            return;
        };
        self.selected = Some((staged, index));
        cx.emit(SidebarEvent::FileSelected {
            path: file.path.clone(),
            staged,
            code: file.code(),
        });
        cx.notify();
    }

    fn select_branch(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(branch) = self.branches.get(index) else {
            return;
        };
        cx.emit(SidebarEvent::BranchSelected(branch.name.clone()));
    }

    fn sidebar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let input_bg = colors.input;

        // 打开按钮（路径非空时高亮）
        let btn_open = cx.entity();
        let has_path = !self.repo_path(cx).is_empty();
        let open_btn = div()
            .id("btn-open")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(if has_path { Hsla::from(rgb(0x2F_81_F7)) } else { input_bg })
            .text_color(if has_path { gpui::white() } else { colors.muted })
            .text_size(px(12.))
            .child("打开")
            .on_click(move |_e, _w, cx| {
                btn_open.update(cx, |_sidebar, cx| cx.emit(SidebarEvent::OpenRepo));
            });

        // 刷新按钮
        let btn_refresh = cx.entity();
        let refresh_btn = div()
            .id("btn-refresh")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(input_bg)
            .text_color(colors.foreground)
            .text_size(px(12.))
            .child("刷新")
            .on_click(move |_e, _w, cx| {
                btn_refresh.update(cx, |_sidebar, cx| cx.emit(SidebarEvent::Refresh));
            });

        // 收起按钮
        let btn_collapse = cx.entity();
        let collapse_btn = div()
            .id("btn-collapse")
            .px_1()
            .py_0p5()
            .rounded_md()
            .hover(|this| this.bg(colors.input))
            .text_size(px(12.))
            .text_color(colors.muted)
            .child("«")
            .on_click(move |_e, _w, cx| {
                btn_collapse.update(cx, |_sidebar, cx| cx.emit(SidebarEvent::ToggleCollapse));
            });

        v_flex()
            .id("sidebar")
            .w_full()
            .h_full()
            .bg(colors.background)
            .child(
                v_flex()
                    .id("sidebar-repo")
                    .w_full()
                    .gap_2()
                    .p_3()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(colors.muted)
                                    .child("仓库"),
                            )
                            .child(div().flex_1())
                            .child(collapse_btn),
                    )
                    .child(Input::new(&self.repo_path_input).w_full().h_7())
                    .child(
                        h_flex()
                            .gap_2()
                            .child(open_btn)
                            .child(refresh_btn)
                            .child(div().flex_1()),
                    ),
            )
            .child(
                v_flex()
                    .id("sidebar-sections")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.branch_section(cx))
                    .child(self.change_section(cx, true, &self.staged))
                    .child(self.change_section(cx, false, &self.unstaged))
                    .child(self.recent_section(cx)),
            )
    }

    /// 本地分支分区
    fn branch_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        // 时间戳过期即不高亮（不在此处改状态，避免 render 中 notify）
        let flash = self
            .flash_branches_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false);
        let rows = self
            .branches
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let this = cx.entity();
                let name = b.name.clone();
                let is_head = b.is_head;
                h_flex()
                    .id(SharedString::from(format!("branch-{name}")))
                    .w_full()
                    .h_6()
                    .flex_shrink_0()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .rounded_sm()
                    .hover(|this| this.bg(colors.list_hover))
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |sidebar, cx| sidebar.select_branch(i, cx));
                    })
                    .child(
                        div()
                            .w(px(14.))
                            .text_size(px(12.))
                            .text_color(if is_head { colors.green } else { colors.muted })
                            .child(if is_head { "●" } else { "○" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(if is_head {
                                colors.foreground
                            } else {
                                colors.muted_foreground
                            })
                            .child(shared(name.clone())),
                    )
                    .when(!is_head, |row| {
                        let this = cx.entity();
                        row.child(
                            div()
                                .id(SharedString::from(format!("checkout-{name}")))
                                .px_1()
                                .rounded_sm()
                                .hover(|this| this.bg(colors.input))
                                .text_size(px(11.))
                                .text_color(colors.muted)
                                .child("⇥")
                                .on_click(move |_e, _w, cx| {
                                    this.update(cx, |_sidebar, cx| {
                                        cx.emit(SidebarEvent::CheckoutBranch(name.clone()));
                                    });
                                }),
                        )
                    })
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("branch-section")
            .w_full()
            .gap_0p5()
            .p_2()
            .child(
                h_flex()
                    .id("branch-section-header")
                    .w_full()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(if flash { colors.list_active } else { colors.background })
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.muted)
                            .child("分支"),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(colors.muted)
                            .child(self.branches.len().to_string()),
                    ),
            )
            .children(rows)
    }

    /// 变更文件分区（暂存/未暂存）
    fn change_section(
        &self,
        cx: &Context<Self>,
        staged: bool,
        files: &[FileStatus],
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let title = if staged { "暂存" } else { "变更" };
        let rows = files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let this = cx.entity();
                // 第二个 on_click 也用 this —— Entity 需 clone
                let this_diff = this.clone();
                let path = f.path.clone();
                let (code_color, label) = status_style(&colors, f.code());
                let selected = self.selected == Some((staged, i));
                h_flex()
                    .id(SharedString::from(format!("file-{staged}-{i}")))
                    .w_full()
                    .h_6()
                    .flex_shrink_0()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .rounded_sm()
                    .bg(if selected { colors.list_active } else { colors.background })
                    .hover(|this| {
                        if !selected {
                            this.bg(colors.list_hover)
                        } else {
                            this
                        }
                    })
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |sidebar, cx| sidebar.select_file(staged, i, cx));
                    })
                    .child(
                        div()
                            .w(px(24.))
                            .text_size(px(11.))
                            .text_color(code_color)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(colors.foreground)
                            .child(shared(path.clone())),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("file-diff-{staged}-{i}")))
                            .px_1()
                            .rounded_sm()
                            .hover(|this| this.bg(colors.input))
                            .text_size(px(11.))
                            .text_color(colors.muted)
                            .child("✎")
                            .on_click(move |_e, _w, cx| {
                                this_diff.update(cx, |_sidebar, cx| {
                                    cx.emit(SidebarEvent::FileDiff {
                                        path: path.clone(),
                                        staged,
                                    });
                                });
                            }),
                    )
            })
            .collect::<Vec<_>>();

        v_flex()
            .id(SharedString::from(format!("change-section-{staged}")))
            .w_full()
            .gap_0p5()
            .p_2()
            .child(section_header(cx, title, files.len()))
            .children(rows)
    }

    /// 最近仓库
    fn recent_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let recents = self
            .recent_repos
            .iter()
            .map(|path| {
                let this = cx.entity();
                let path = path.clone();
                h_flex()
                    .id(SharedString::from(format!("recent-{path}")))
                    .w_full()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .hover(|this| this.bg(colors.list_hover))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(colors.muted_foreground)
                            .child(shared(path.clone())),
                    )
                    .on_click(move |_e, window, cx| {
                        this.update(cx, |sidebar, cx| {
                            // 回填输入框（on_click 回调里才有 &mut Window）
                            sidebar.repo_path_input.update(cx, |input, cx| {
                                input.set_value(path.clone(), window, cx);
                            });
                            cx.emit(SidebarEvent::OpenRecent(path.clone()));
                        });
                    })
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("recent-section")
            .w_full()
            .gap_0p5()
            .p_2()
            .child(section_header(cx, "最近仓库", self.recent_repos.len()))
            .children(recents)
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sidebar(window, cx)
    }
}

/// 分区标题（名字 + 计数）
fn section_header(
    cx: &Context<Sidebar>,
    title: &str,
    count: usize,
) -> impl IntoElement {
    let colors = cx.theme().colors.clone();
    h_flex()
        .id(SharedString::from(format!("section-{title}")))
        .w_full()
        .px_2()
        .py_0p5()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_size(px(11.))
                .text_color(colors.muted)
                .child(shared(title)),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(colors.muted)
                .child(count.to_string()),
        )
}

/// 状态码 → 颜色/标签
fn status_style(colors: &gpui_component::theme::ThemeColor, code: char) -> (Hsla, &'static str) {
    let color = match code {
        'M' | 'R' | 'C' => colors.warning,
        'A' => colors.green,
        'D' | 'U' => colors.red,
        _ => colors.muted,
    };
    let label = match code {
        'M' => "改",
        'A' => "增",
        'D' => "删",
        'R' => "移",
        'C' => "拷",
        'U' => "冲",
        _ => "?",
    };
    (color, label)
}
