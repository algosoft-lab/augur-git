//! M1 主界面：镜像 rgitui 的布局结构（TitleBar/Toolbar/TabBar/三栏/StatusBar/Welcome）
//!
//! 布局自上而下：
//!   TitleBar（自绘，无原生标题栏）→ Toolbar → TabBar →
//!   [侧栏(可拖拽调宽) | 中列(GraphView + 拖拽条 + 底部面板) | 右栏(详情 tab + CommitPanel)]
//!   → StatusBar
//!
//! 事件链（镜像 rgitui 的 workspace/events.rs）：
//!   面板交互 → Workspace 汇总 → GitView → 工作线程 git 子进程 → 事件回流各面板

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, InteractiveElementExt, Root, TitleBar,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    theme::{Theme, ThemeMode},
    v_flex,
};

use crate::core::config::{self, AppConfig, LanguagePreference};
use crate::core::i18n::{self, Locale};
use crate::git::graph::{GraphEvent, GraphView};
use crate::git::panel::{
    BottomPanel, BottomPanelEvent, CommitPanel, CommitPanelEvent, DetailContent, DetailPanel,
};
use crate::git::sidebar::{Sidebar, SidebarEvent};
use crate::git::toolbar::{Toolbar, ToolbarEvent};
use crate::git::{GitStatus, GitUiEvent, GitView, shared};

/// 拖拽调宽事件类型（镜像 rgitui：on_drag 起始 + 根元素 on_drag_move 更新尺寸）
/// 必须实现 Render（on_drag 的 W: Render 约束，rgitui 同款空元素）
#[derive(Clone, Debug)]
pub struct SidebarResize;
#[derive(Clone, Debug)]
pub struct DetailPanelResize;
#[derive(Clone, Debug)]
pub struct DiffViewerResize;

impl Render for SidebarResize {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
impl Render for DetailPanelResize {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
impl Render for DiffViewerResize {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

const MIN_SIDEBAR_WIDTH: f32 = 180.0;
const MAX_SIDEBAR_WIDTH: f32 = 400.0;
const MIN_DETAIL_WIDTH: f32 = 220.0;
const MAX_DETAIL_WIDTH: f32 = 600.0;
const MIN_DIFF_HEIGHT: f32 = 100.0;
const MAX_DIFF_HEIGHT: f32 = 500.0;
/// 状态栏高度（拖拽底部边界计算用）
const STATUS_BAR_HEIGHT: f32 = 24.0;

pub fn run(app: Application) {
    app.run(|cx| {
        gpui_component::init(cx);

        // 暗黑主题（VSCode Dark+ 风格）
        Theme::change(ThemeMode::Dark, None, cx);
        // 灰度文字可读性覆盖：shadcn 暗色默认 muted_foreground(neutral-400 #a3a3a3)
        // 与表头灰(#525252)在纯黑背景上偏暗，统一提亮（层级仍低于 foreground #fafafa）
        Theme::global_mut(cx).muted_foreground = Hsla::from(rgb(0xB4B4B4));
        Theme::global_mut(cx).table_head_foreground = Hsla::from(rgb(0xA3A3A3));

        cx.spawn(async move |cx| {
            let window_options = cx.update(initial_window_options);
            cx.open_window(window_options, |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(window, cx));
                cx.new(|cx| Root::new(workspace, window, cx))
            })
            .unwrap_or_else(|e| {
                log::error!("Failed to open window: {e}");
                std::process::exit(1);
            })
        })
        .detach();
    });
}

/// 窗口选项：全平台客户端自绘标题栏（无原生标题栏，同 augur-term/augur-com）
fn initial_window_options(cx: &mut App) -> WindowOptions {
    let desired_size = size(px(1280.), px(800.));
    let primary_display = cx.primary_display();

    let window_bounds = if let Some(display) = primary_display.clone() {
        let visible_bounds = display.visible_bounds();
        let clamped_size = desired_size.min(&visible_bounds.size);
        WindowBounds::Windowed(Bounds::centered_at(visible_bounds.center(), clamped_size))
    } else {
        WindowBounds::centered(desired_size, cx)
    };

    WindowOptions {
        window_bounds: Some(window_bounds),
        display_id: primary_display.map(|display| display.id()),
        titlebar: Some(TitleBar::title_bar_options()),
        window_min_size: Some(gpui::Size {
            width: px(860.),
            height: px(480.),
        }),
        ..Default::default()
    }
}

/// 面板尺寸布局状态（M1 内存态；持久化 M4）
struct LayoutState {
    sidebar_width: f32,
    detail_width: f32,
    diff_height: f32,
}

pub struct Workspace {
    git_view: Entity<GitView>,
    /// 仓库路径输入框（侧栏/Welcome 共享）
    repo_path_input: Entity<InputState>,
    sidebar: Entity<Sidebar>,
    graph: Entity<GraphView>,
    toolbar: Entity<Toolbar>,
    detail: Entity<DetailPanel>,
    commit: Entity<CommitPanel>,
    /// 底部面板（选中提交文件清单 + 单文件 diff 分栏）
    bottom: Entity<BottomPanel>,
    /// 配置单一事实源（变更即存盘）
    config: AppConfig,
    /// 连接状态（状态栏显示）
    status: GitStatus,
    /// 状态栏操作消息（命令结果/提示）
    status_message: Option<String>,
    layout: LayoutState,
    sidebar_collapsed: bool,
    /// 界面语言偏好（设置弹层切换；单一事实源为 config.language）
    language_preference: LanguagePreference,
    /// 当前语言环境（渲染取文案用；随偏好解析）
    locale: Locale,
    /// 设置弹层开关（当前含语言切换）
    show_settings: bool,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = config::load();
        let language_preference = config.language;
        let locale = i18n::resolve(&language_preference);
        let git_view = cx.new(|cx| GitView::new(locale, cx));

        // 仓库路径输入框（共享：侧栏 + Welcome 页都用它）
        let repo_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::text(locale, "repo-path-placeholder"))
                .default_value(config.repo.path.clone())
        });
        let sidebar = cx.new(|cx| Sidebar::new(window, cx, &config, &repo_path_input, locale));
        let graph = cx.new(|_cx| GraphView::new(locale));
        let toolbar = cx.new(|_cx| Toolbar::new(locale));
        let detail = cx.new(|_cx| DetailPanel::new(locale));
        let commit = cx.new(|cx| CommitPanel::new(window, cx, locale));
        let bottom = cx.new(|_cx| BottomPanel::new(locale));

        // 输入框回车 = 打开仓库（侧栏/Welcome 共用）
        let input = repo_path_input.clone();
        cx.subscribe(&input, |workspace, _e, event, cx| {
            if matches!(
                event,
                InputEvent::PressEnter {
                    secondary: false,
                    ..
                }
            ) {
                workspace.open_repo_from_input(cx);
            }
        })
        .detach();

        // ---- 面板交互事件 → Workspace 汇总 → GitView 命令 ----

        // 侧栏：最近仓库/收起/切换分支/选中文件
        cx.subscribe(&sidebar, |workspace, _e, event, cx| match event {
            SidebarEvent::OpenRecent(path) => {
                if workspace.git_view.read(cx).connected() {
                    workspace.git_view.update(cx, |view, _| view.close_repo());
                }
                workspace
                    .git_view
                    .update(cx, |view, cx| view.open_repo(&path, cx));
            }
            SidebarEvent::ToggleCollapse => {
                workspace.sidebar_collapsed = !workspace.sidebar_collapsed;
                cx.notify();
            }
            SidebarEvent::BranchSelected(name) => {
                workspace.status_message = Some(i18n::text_args(
                    workspace.locale,
                    "branch-selected",
                    &[("name", name)],
                ));
                cx.notify();
            }
            SidebarEvent::CheckoutBranch(name) => {
                workspace.git_view.update(cx, |view, _| {
                    view.run("checkout", vec!["checkout".into(), name.clone()]);
                });
                workspace.toolbar.update(cx, |tb, cx| tb.set_busy(true, cx));
            }
            SidebarEvent::FileSelected { path, staged, code } => {
                workspace.detail.update(cx, |d, cx| {
                    d.set_content(
                        DetailContent::File {
                            path: path.clone(),
                            staged: *staged,
                            code: *code,
                        },
                        cx,
                    )
                });
            }
        })
        .detach();

        // 工具栏：fetch/pull/push/refresh
        cx.subscribe(&toolbar, |workspace, _e, event, cx| match event {
            ToolbarEvent::Fetch => {
                workspace.git_view.update(cx, |view, _| {
                    view.run("fetch --all", vec!["fetch".into(), "--all".into()]);
                });
                workspace.toolbar.update(cx, |tb, cx| tb.set_busy(true, cx));
            }
            ToolbarEvent::Pull => {
                workspace.git_view.update(cx, |view, _| {
                    view.run("pull", vec!["pull".into()]);
                });
                workspace.toolbar.update(cx, |tb, cx| tb.set_busy(true, cx));
            }
            ToolbarEvent::Push => {
                workspace.git_view.update(cx, |view, _| {
                    view.run("push", vec!["push".into()]);
                });
                workspace.toolbar.update(cx, |tb, cx| tb.set_busy(true, cx));
            }
            ToolbarEvent::Branch => {
                workspace.sidebar.update(cx, |sb, cx| sb.flash_branches(cx));
            }
            ToolbarEvent::Refresh => {
                workspace.git_view.update(cx, |view, _| view.refresh());
            }
            ToolbarEvent::Settings => {
                workspace.show_settings = true;
                cx.notify();
            }
        })
        .detach();

        // 提交图：选中 → 右侧详情 + 底部面板文件清单（numstat 查询）
        cx.subscribe(&graph, |workspace, _e, event, cx| match event {
            GraphEvent::CommitSelected {
                oid,
                short,
                subject,
                author,
                date,
                decorations,
            } => {
                workspace.detail.update(cx, |d, cx| {
                    d.set_content(
                        DetailContent::Commit {
                            short: short.clone(),
                            subject: subject.clone(),
                            author: author.clone(),
                            date: date.clone(),
                            decorations: decorations.clone(),
                        },
                        cx,
                    )
                });
                workspace
                    .bottom
                    .update(cx, |b, cx| b.set_commit(oid, short, subject, cx));
                workspace
                    .git_view
                    .update(cx, |view, _| view.commit_files(oid.clone()));
            }
        })
        .detach();

        // 提交面板：提交
        cx.subscribe(&commit, |workspace, _e, event, cx| match event {
            CommitPanelEvent::Submit(msg) => {
                workspace.git_view.update(cx, |view, _| {
                    view.run("commit", vec!["commit".into(), "-m".into(), msg.clone()]);
                });
                workspace.toolbar.update(cx, |tb, cx| tb.set_busy(true, cx));
            }
        })
        .detach();

        // 底部面板：选中文件 → 右栏加载该文件在此提交的 diff
        cx.subscribe(&bottom, |workspace, _e, event, cx| match event {
            BottomPanelEvent::ShowFileDiff { oid, path } => {
                workspace
                    .git_view
                    .update(cx, |view, _| view.file_diff(oid.clone(), path.clone()));
            }
        })
        .detach();

        // ---- GitView 快照事件 → 各面板/状态栏/持久化 ----
        cx.subscribe(&git_view, |workspace, _e, event, cx| match event {
            GitUiEvent::StatusChanged {
                branch,
                ahead,
                behind,
                files,
                branches,
            } => {
                let has_staged = files.iter().any(|f| f.is_staged());
                let staged_count = files.iter().filter(|f| f.is_staged()).count();
                let unstaged_count = files.len() - staged_count;
                workspace.status = GitStatus::Ready(i18n::text_args(
                    workspace.locale,
                    "status-summary",
                    &[
                        ("branch", branch),
                        ("ahead", &ahead.to_string()),
                        ("behind", &behind.to_string()),
                        ("staged", &staged_count.to_string()),
                        ("unstaged", &unstaged_count.to_string()),
                    ],
                ));
                workspace.sidebar.update(cx, |sb, cx| {
                    sb.set_status(branch.clone(), branches.clone(), files.clone(), cx);
                });
                workspace.toolbar.update(cx, |tb, cx| {
                    tb.set_ahead_behind(*ahead, *behind, cx);
                });
                workspace.commit.update(cx, |cp, cx| {
                    cp.set_has_staged(has_staged, cx);
                });
                cx.notify();
            }
            GitUiEvent::LogChanged { rows } => {
                workspace
                    .graph
                    .update(cx, |g, cx| g.set_rows(rows.clone(), cx));
            }
            GitUiEvent::RefsChanged(refs) => {
                workspace
                    .sidebar
                    .update(cx, |sb, cx| sb.set_refs(refs.clone(), cx));
            }
            GitUiEvent::CommitFilesChanged { oid, files } => {
                workspace
                    .bottom
                    .update(cx, |b, cx| b.set_files(oid, files.clone(), cx));
            }
            GitUiEvent::FileDiffChanged { oid, path, diff } => {
                workspace
                    .bottom
                    .update(cx, |b, cx| b.set_diff(oid, path, diff.clone(), cx));
            }
            GitUiEvent::CommandDone {
                label,
                success,
                message,
            } => {
                workspace
                    .toolbar
                    .update(cx, |tb, cx| tb.set_busy(false, cx));
                // 状态栏摘要 + 写操作后刷新快照
                let refresh_after = matches!(
                    label.as_str(),
                    "commit" | "checkout" | "fetch --all" | "pull" | "push"
                );
                workspace.status_message = Some(if *success {
                    i18n::text_args(workspace.locale, "command-success", &[("label", label)])
                } else {
                    i18n::text_args(
                        workspace.locale,
                        "command-failed",
                        &[("label", label), ("error", first_line(message))],
                    )
                });
                if *success && refresh_after {
                    workspace.git_view.update(cx, |view, _| view.refresh());
                }
                cx.notify();
            }
            GitUiEvent::RepoOpened(path) => {
                workspace.config.repo.path = path.clone();
                workspace.config.push_recent(&path);
                let _ = config::save(&workspace.config);
                let recent = workspace.config.recent_repos.clone();
                workspace.sidebar.update(cx, |sb, _| sb.set_recent(recent));
                cx.notify();
            }
            GitUiEvent::Error(msg) => {
                workspace.status = GitStatus::Error(msg.clone());
                cx.notify();
            }
        })
        .detach();

        // 启动时自动打开上次的仓库（镜像 rgitui：启动解析保存的 workspace）
        if !config.repo.path.is_empty() {
            let path = config.repo.path.clone();
            git_view.update(cx, |view, cx| view.open_repo(&path, cx));
        }

        Self {
            git_view,
            repo_path_input,
            sidebar,
            graph,
            toolbar,
            detail,
            commit,
            bottom,
            config,
            status: GitStatus::None,
            status_message: None,
            layout: LayoutState {
                sidebar_width: 250.0,
                detail_width: 320.0,
                diff_height: 260.0,
            },
            sidebar_collapsed: false,
            language_preference,
            locale,
            show_settings: false,
        }
    }

    /// 切换界面语言：立即生效（下一次 render 即用新 locale 取文案）并持久化
    /// （镜像 augur-pdf set_language）。`System` 选项即时按当前系统语言解析。
    fn set_language(
        &mut self,
        preference: LanguagePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.language_preference = preference;
        self.locale = i18n::resolve(&preference);
        // 各子面板持有自己的 locale 副本，切换语言时同步（含输入框 placeholder）
        let locale = self.locale;
        self.git_view.update(cx, |view, _| view.set_locale(locale));
        self.sidebar.update(cx, |sb, cx| sb.set_locale(locale, cx));
        self.toolbar.update(cx, |tb, cx| tb.set_locale(locale, cx));
        self.graph.update(cx, |g, cx| g.set_locale(locale, cx));
        self.detail.update(cx, |d, cx| d.set_locale(locale, cx));
        self.commit
            .update(cx, |cp, cx| cp.set_locale(locale, window, cx));
        self.bottom.update(cx, |b, cx| b.set_locale(locale, cx));
        let placeholder = i18n::text(locale, "repo-path-placeholder");
        self.repo_path_input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
        log::info!(
            "[workspace] language preference: {:?}, locale: {}",
            preference,
            self.locale.id()
        );
        self.config.language = preference;
        let _ = config::save(&self.config);
        cx.notify();
    }

    /// 自绘标题栏：仓库名 + 分支徽标 + 双击最大化。
    /// 最小化/最大化/关闭按钮由系统绘制（DWM），不要自绘以免重叠。
    fn title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let repo = crate::git::dir_name(&self.config.repo.path).to_string();
        let repo_label = if repo.is_empty() {
            "augur-git".to_string()
        } else {
            repo
        };

        // 分支徽标（点击 → 侧栏分支区闪烁）
        let branch = match &self.status {
            GitStatus::Ready(label) => label.split(" · ").next().unwrap_or("").to_string(),
            _ => String::new(),
        };
        let this = cx.entity();
        let branch_badge = if branch.is_empty() {
            None
        } else {
            Some(
                h_flex()
                    .id("title-branch")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(colors.input)
                    .hover(|this| this.bg(colors.list_hover))
                    .cursor(CursorStyle::PointingHand)
                    .text_size(px(12.))
                    .text_color(colors.blue)
                    .child(SharedString::from(format!("⎇ {branch}")))
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |ws, cx| ws.emit_sidebar_focus(cx));
                    }),
            )
        };

        TitleBar::new().child(
            h_flex()
                .id("title-bar-content")
                .w_full()
                .h_full()
                .items_center()
                .px_2()
                .gap_2()
                .on_double_click(|_event, window, _cx| {
                    window.zoom_window();
                })
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.muted_foreground)
                        .child(repo_label),
                )
                .when_some(branch_badge, |el, badge| el.child(badge)),
        )
    }

    fn emit_sidebar_focus(&mut self, cx: &mut Context<Self>) {
        self.sidebar.update(cx, |sb, cx| sb.flash_branches(cx));
    }

    /// 用共享输入框的路径打开仓库（侧栏按钮 / Welcome / 回车共用）
    fn open_repo_from_input(&mut self, cx: &mut Context<Self>) {
        let path = self.repo_path_input.read(cx).value().to_string();
        if self.git_view.read(cx).connected() {
            self.git_view.update(cx, |view, _| view.close_repo());
        }
        if !path.is_empty() {
            self.git_view
                .update(cx, |view, cx| view.open_repo(&path, cx));
        }
    }

    /// 系统文件夹选择器（浏览…按钮）
    fn pick_repo_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from(i18n::text(
                self.locale,
                "repo-folder-prompt",
            ))),
        });
        cx.spawn(async move |this, cx| {
            let path = match rx.await {
                Ok(Ok(Some(paths))) => paths.first().map(|p| p.to_string_lossy().into_owned()),
                _ => None,
            };
            let Some(path) = path else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                if ws.git_view.read(cx).connected() {
                    ws.git_view.update(cx, |view, _| view.close_repo());
                }
                ws.git_view.update(cx, |view, cx| view.open_repo(&path, cx));
            });
        })
        .detach();
    }

    /// 关闭仓库 tab（× 按钮）：停工作线程 + 清自动打开路径 + 回 Welcome 页
    fn close_repo_tab(&mut self, cx: &mut Context<Self>) {
        self.git_view.update(cx, |view, _| view.close_repo());
        self.config.repo.path.clear();
        let _ = config::save(&self.config);
        self.status = GitStatus::None;
        self.status_message = None;
        cx.notify();
    }

    /// TabBar：单仓库 tab（带关闭 ×）+ 尾部打开按钮（多仓库 M2）
    fn tab_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let repo = crate::git::dir_name(&self.config.repo.path).to_string();
        let has_repo = self.git_connected() || !repo.is_empty();
        let tab_label = if repo.is_empty() {
            SharedString::from(i18n::text(self.locale, "no-repo-open"))
        } else {
            SharedString::from(repo)
        };
        // 关闭按钮（×，镜像 algocode：Button ghost + label 渲染；常态可见，点击停线程回 Welcome）
        let this = cx.entity();
        let close_btn = Button::new("tab-close")
            .label("×")
            .ghost()
            .size(px(14.))
            .custom(
                ButtonCustomVariant::new(cx).foreground(colors.tab_active_foreground.opacity(0.6)),
            )
            .when(!has_repo, |btn| btn.disabled(true))
            .on_click(move |_e, _w, cx| {
                this.update(cx, |ws, cx| ws.close_repo_tab(cx));
            });
        h_flex()
            .id("tab-bar")
            .w_full()
            .h(px(28.))
            .flex_shrink_0()
            .px_2()
            .gap_1()
            .items_center()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .child(
                h_flex()
                    .id("tab-repo")
                    .h_full()
                    .px_3()
                    .items_center()
                    .gap_2()
                    .bg(colors.tab_active)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(colors.tab_active_foreground)
                            .child(tab_label),
                    )
                    .child(close_btn),
            )
            .child(div().flex_1())
    }

    fn git_connected(&self) -> bool {
        matches!(self.status, GitStatus::Ready(_))
    }

    /// 状态栏（镜像 rgitui status_bar：分支 · ahead/behind · 变更数 · 仓库路径 · 消息）
    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let (text, color) = match &self.status {
            GitStatus::None => (
                i18n::text(self.locale, "no-repo-open"),
                colors.muted_foreground,
            ),
            GitStatus::Scanning => (i18n::text(self.locale, "status-scanning"), colors.warning),
            GitStatus::Ready(label) => (format!("● {label}"), colors.green),
            GitStatus::Error(msg) => (format!("✗ {msg}"), colors.red),
        };
        let repo = &self.config.repo.path;
        let left = if repo.is_empty() {
            i18n::text(self.locale, "status-no-repo-selected")
        } else {
            repo.clone()
        };
        let msg = self.status_message.clone();
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
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .child(SharedString::from(left)),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when_some(msg, |row, msg| {
                        row.child(
                            div()
                                .text_size(px(11.))
                                .text_color(colors.muted_foreground)
                                .child(SharedString::from(msg)),
                        )
                    })
                    .child(div().text_size(px(11.)).text_color(color).child(text)),
            )
    }

    /// 收起态侧栏：细条 + 展开按钮
    fn collapsed_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        v_flex()
            .id("sidebar-rail")
            .w(px(28.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().colors.background)
            .items_center()
            .pt_2()
            .child(
                div()
                    .id("btn-expand")
                    .px_1()
                    .rounded_md()
                    .hover(|this| this.bg(cx.theme().colors.input))
                    .text_size(px(12.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("»")
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |workspace, cx| {
                            workspace.sidebar_collapsed = false;
                            cx.notify();
                        });
                    }),
            )
    }

    /// 主内容区：侧栏 | 中列 | 右栏（拖拽调宽监听挂根元素）
    fn main_content(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let sidebar_width = px(self.layout.sidebar_width);
        let detail_width = px(self.layout.detail_width);
        let diff_height = px(self.layout.diff_height);

        h_flex()
            .id("main-content")
            .size_full()
            .min_h_0()
            // 拖拽调宽（cx.listener：&mut Self + 上下文内 notify）
            .on_drag_move::<SidebarResize>(cx.listener(
                |this, e: &DragMoveEvent<SidebarResize>, _, cx| {
                    let new_w =
                        f32::from(e.event.position.x).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
                    this.layout.sidebar_width = new_w;
                    cx.notify();
                },
            ))
            .on_drag_move::<DetailPanelResize>(cx.listener(
                |this, e: &DragMoveEvent<DetailPanelResize>, window, cx| {
                    let w = window.bounds().size.width;
                    let new_w = (f32::from(w) - f32::from(e.event.position.x))
                        .clamp(MIN_DETAIL_WIDTH, MAX_DETAIL_WIDTH);
                    this.layout.detail_width = new_w;
                    cx.notify();
                },
            ))
            .on_drag_move::<DiffViewerResize>(cx.listener(
                |this, e: &DragMoveEvent<DiffViewerResize>, window, cx| {
                    let h = window.bounds().size.height;
                    let new_h = (f32::from(h) - STATUS_BAR_HEIGHT - f32::from(e.event.position.y))
                        .clamp(MIN_DIFF_HEIGHT, MAX_DIFF_HEIGHT);
                    this.layout.diff_height = new_h;
                    cx.notify();
                },
            ))
            // 左：侧栏（收起态显示细条）+ 拖拽条
            .child(
                div()
                    .relative()
                    .w(sidebar_width)
                    .h_full()
                    .flex_shrink_0()
                    .child(if self.sidebar_collapsed {
                        self.collapsed_rail(cx).into_any_element()
                    } else {
                        self.sidebar.clone().into_any_element()
                    })
                    .child(
                        div()
                            .id("sidebar-resize-handle")
                            .absolute()
                            .top_0()
                            .right(px(-3.))
                            .h_full()
                            .w(px(5.))
                            .cursor_col_resize()
                            .hover(|s| s.bg(colors.drag_border))
                            .on_drag(SidebarResize, |val, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| val.clone())
                            }),
                    ),
            )
            // 中：GraphView + 拖拽条 + 底部面板
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
                            .hover(|s| s.bg(colors.drag_border))
                            .on_drag(DiffViewerResize, |val, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| val.clone())
                            }),
                    )
                    .child(
                        v_flex()
                            .h(diff_height)
                            .flex_shrink_0()
                            .child(self.bottom_panel(cx)),
                    ),
            )
            // 右：详情 tab + CommitPanel
            .child(
                div()
                    .relative()
                    .w(detail_width)
                    .h_full()
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(colors.border)
                    .flex()
                    .flex_col()
                    .child(div().flex_1().min_h_0().child(self.detail.clone()))
                    .child(
                        div()
                            .id("detail-resize-handle")
                            .absolute()
                            .top_0()
                            .left(px(-3.))
                            .h_full()
                            .w(px(5.))
                            .cursor_col_resize()
                            .hover(|s| s.bg(colors.drag_border))
                            .on_drag(DetailPanelResize, |val, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| val.clone())
                            }),
                    )
                    .child(self.commit.clone()),
            )
    }

    /// 底部面板：选中提交文件清单 + 单文件 diff 分栏（无 tab，面板自带头行）
    fn bottom_panel(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        self.bottom.clone()
    }

    /// Welcome 页（未打开仓库时）：Logo + 路径输入行（输入/回车/浏览…）+ 最近仓库
    fn welcome(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();

        // 打开按钮（读共享输入框）
        let btn_open = cx.entity();
        let open_btn = div()
            .id("welcome-open")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(Hsla::from(rgb(0x2F_81_F7)))
            .text_color(gpui::white())
            .text_size(px(12.))
            .child(shared(i18n::text(self.locale, "welcome-open")))
            .on_click(move |_e, _w, cx| {
                btn_open.update(cx, |ws, cx| ws.open_repo_from_input(cx));
            });

        // 浏览…（系统文件夹选择器）
        let btn_browse = cx.entity();
        let browse_btn = div()
            .id("welcome-browse")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(colors.input)
            .text_color(colors.foreground)
            .text_size(px(12.))
            .child(shared(i18n::text(self.locale, "welcome-browse")))
            .on_click(move |_e, _w, cx| {
                btn_browse.update(cx, |ws, cx| ws.pick_repo_folder(cx));
            });

        let recents = self
            .config
            .recent_repos
            .iter()
            .map(|path| {
                let this = cx.entity();
                let path = path.clone();
                h_flex()
                    .id(SharedString::from(format!("welcome-recent-{path}")))
                    .w(px(380.))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(|this| this.bg(colors.list_hover))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(colors.muted_foreground)
                            .child(SharedString::from(path.clone())),
                    )
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |ws, cx| {
                            ws.git_view.update(cx, |view, cx| view.open_repo(&path, cx));
                        });
                    })
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("welcome")
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(48.))
                    .h(px(48.))
                    .rounded(px(12.))
                    .bg(colors.input)
                    .text_size(px(24.))
                    .text_color(colors.accent)
                    .child("⎇"),
            )
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child("augur-git"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(i18n::text(self.locale, "app-tagline"))),
            )
            // 路径输入行：输入框 + 打开 + 浏览…
            .child(
                h_flex()
                    .w(px(380.))
                    .gap_2()
                    .child(Input::new(&self.repo_path_input).flex_1())
                    .child(open_btn)
                    .child(browse_btn),
            )
            .when(!recents.is_empty(), |w| {
                w.child(
                    v_flex()
                        .w(px(380.))
                        .gap_0p5()
                        .mt_2()
                        .child(
                            div()
                                .px_2()
                                .text_size(px(11.))
                                .text_color(colors.muted_foreground)
                                .child(shared(i18n::text(self.locale, "recent-repos"))),
                        )
                        .children(recents),
                )
            })
    }

    /// 设置弹层（镜像 augur-pdf about 弹层：遮罩点击关闭、卡片阻止冒泡；
    /// 当前仅界面语言三选一，后续设置项在此扩展）
    fn settings_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let locale = self.locale;
        let current = self.language_preference;

        // 语言按钮：当前项 primary、其余 ghost（augur-pdf 同款单选样式）
        let lang_btn =
            |id: &'static str, key: &str, pref: LanguagePreference, cx: &Context<Self>| {
                let this = cx.entity();
                Button::new(id)
                    .label(i18n::text(locale, key))
                    .flex_1()
                    .when(pref == current, |b| b.primary())
                    .when(pref != current, |b| b.ghost())
                    .on_click(move |_e, window, cx| {
                        this.update(cx, |ws, cx| ws.set_language(pref, window, cx));
                    })
            };

        let this_close = cx.entity();
        v_flex()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .bg(colors.background.opacity(0.9))
            .flex()
            .items_center()
            .justify_center()
            // 点击遮罩关闭
            .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                this_close.update(cx, |ws, cx| {
                    ws.show_settings = false;
                    cx.notify();
                });
            })
            .child(
                v_flex()
                    .id("settings-card")
                    .items_start()
                    .gap_3()
                    .p_6()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .min_w(px(380.))
                    .max_w(px(460.))
                    .when(cx.theme().shadow, |el| el.shadow_md())
                    // 阻止冒泡：点卡片内部不触发遮罩的关闭
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(colors.foreground)
                            .child(shared(i18n::text(locale, "settings-title"))),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.foreground)
                                    .child(shared(i18n::text(locale, "language-title"))),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .child(lang_btn(
                                        "lang-system",
                                        "language-system",
                                        LanguagePreference::System,
                                        cx,
                                    ))
                                    .child(lang_btn(
                                        "lang-chinese",
                                        "language-chinese",
                                        LanguagePreference::SimplifiedChinese,
                                        cx,
                                    ))
                                    .child(lang_btn(
                                        "lang-english",
                                        "language-english",
                                        LanguagePreference::English,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        Button::new("settings-close")
                            .label(i18n::text(locale, "settings-close"))
                            .ghost()
                            .w_full()
                            .on_click({
                                let this = cx.entity();
                                move |_e, _w, cx| {
                                    this.update(cx, |ws, cx| {
                                        ws.show_settings = false;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let has_repo = !self.config.repo.path.is_empty() || self.git_connected();

        v_flex()
            .id("workspace")
            .size_full()
            .relative()
            .bg(colors.background)
            .child(self.title_bar(window, cx))
            .child(self.toolbar.clone())
            .child(self.tab_bar(cx))
            .child(if has_repo {
                self.main_content(window, cx).into_any_element()
            } else {
                self.welcome(window, cx).into_any_element()
            })
            .child(self.status_bar(cx))
            .when(self.show_settings, |el| el.child(self.settings_overlay(cx)))
    }
}

/// 命令输出取首行（状态栏摘要）
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}
