//! M1 主界面：镜像 rgitui 的布局结构（TitleBar/Toolbar/TabBar/三栏/StatusBar/Welcome）
//!
//! 布局自上而下：
//!   TitleBar（自绘，无原生标题栏）→ Toolbar → TabBar →
//!   [侧栏(可拖拽调宽) | 中列(GraphView + 拖拽条 + 底部面板) | 右栏(详情 tab + CommitPanel)]
//!   → StatusBar
//!
//! 事件链（镜像 rgitui 的 workspace/events.rs）：
//!   面板交互 → Workspace 汇总 → GitView → 工作线程 git 子进程 → 事件回流各面板

mod welcome;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, InteractiveElementExt, Root, TitleBar, h_flex,
    input::{InputEvent, InputState},
    theme::{Theme, ThemeMode},
    v_flex,
};

use crate::core::config::{self, AppConfig, LanguagePreference};
use crate::core::i18n::{self, Locale};
use crate::git::graph::{GraphEvent, GraphView};
use crate::git::panel::{
    BottomPanel, BottomPanelEvent, CommitPanel, CommitPanelEvent,
    DetailContent, DetailPanel,
};
use crate::git::sidebar::{Sidebar, SidebarEvent};
use crate::git::toolbar::{Toolbar, ToolbarEvent};
use crate::git::{GitStatus, GitUiEvent, GitView};

/// 拖拽调宽事件类型（镜像 rgitui：on_drag 起始 + 根元素 on_drag_move 更新尺寸）
/// 必须实现 Render（on_drag 的 W: Render 约束，rgitui 同款空元素）
#[derive(Clone, Debug)]
pub struct SidebarResize;
#[derive(Clone, Debug)]
pub struct DetailPanelResize;
#[derive(Clone, Debug)]
pub struct DiffViewerResize;

impl Render for SidebarResize {
    fn render(
        &mut self,
        _w: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}
impl Render for DetailPanelResize {
    fn render(
        &mut self,
        _w: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}
impl Render for DiffViewerResize {
    fn render(
        &mut self,
        _w: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
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

        // Use the GitHub Dark palette for the application theme.
        Theme::change(ThemeMode::Dark, None, cx);
        let t = Theme::global_mut(cx);
        t.background = Hsla::from(rgb(0x0D1117));
        t.tab_bar = Hsla::from(rgb(0x161B22));
        t.title_bar = Hsla::from(rgb(0x161B22));
        t.input = Hsla::from(rgb(0x21262D));
        t.list_hover = Hsla::from(rgb(0x21262D));
        t.list_active = Hsla::from(rgb(0x264F78));
        t.border = Hsla::from(rgb(0x30363D));
        t.foreground = Hsla::from(rgb(0xE6EDF3));
        t.muted_foreground = Hsla::from(rgb(0x8B949E));
        t.table_head_foreground = Hsla::from(rgb(0x8B949E));
        t.blue = Hsla::from(rgb(0x2F81F7));
        t.accent = Hsla::from(rgb(0x2F81F7));
        t.green = Hsla::from(rgb(0x3FB950));
        t.red = Hsla::from(rgb(0xF85149));
        t.warning = Hsla::from(rgb(0xD29922));
        t.drag_border = Hsla::from(rgb(0x388BFD));

        cx.spawn(async move |cx| {
            let window_options = cx.update(initial_window_options);
            cx.open_window(window_options, |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(window, cx));
                cx.new(|cx| Root::new(workspace, window, cx))
            })
            .unwrap_or_else(|e| {
                log::error!("[workspace] failed to open window: {e}");
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
        WindowBounds::Windowed(Bounds::centered_at(
            visible_bounds.center(),
            clamped_size,
        ))
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
    /// 消息语义色（true=成功绿 / false=失败红；中性提示保持 muted）
    status_message_ok: Option<bool>,
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
        let sidebar = cx.new(|cx| Sidebar::new(window, cx, locale));
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

        // 侧栏：收起/切换分支/选中文件
        cx.subscribe(&sidebar, |workspace, _e, event, cx| match event {
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
                    view.run(
                        "fetch --all",
                        vec!["fetch".into(), "--all".into()],
                    );
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
                    view.run(
                        "commit",
                        vec!["commit".into(), "-m".into(), msg.clone()],
                    );
                });
                workspace.toolbar.update(cx, |tb, cx| tb.set_busy(true, cx));
            }
        })
        .detach();

        // 底部面板：选中文件 → 右栏加载该文件在此提交的 diff
        cx.subscribe(&bottom, |workspace, _e, event, cx| match event {
            BottomPanelEvent::ShowFileDiff { oid, path } => {
                workspace.git_view.update(cx, |view, _| {
                    view.file_diff(oid.clone(), path.clone())
                });
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
                let staged_count =
                    files.iter().filter(|f| f.is_staged()).count();
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
                    sb.set_status(
                        branch.clone(),
                        branches.clone(),
                        files.clone(),
                        cx,
                    );
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
                workspace.bottom.update(cx, |b, cx| {
                    b.set_diff(oid, path, diff.clone(), cx)
                });
            }
            GitUiEvent::CommandDone {
                label,
                success,
                message,
            } => {
                workspace
                    .toolbar
                    .update(cx, |tb, cx| tb.set_busy(false, cx));
                // 状态栏摘要（语义着色）+ 写操作后刷新快照
                let refresh_after = matches!(
                    label.as_str(),
                    "commit" | "checkout" | "fetch --all" | "pull" | "push"
                );
                workspace.status_message = Some(if *success {
                    i18n::text_args(
                        workspace.locale,
                        "command-success",
                        &[("label", label)],
                    )
                } else {
                    i18n::text_args(
                        workspace.locale,
                        "command-failed",
                        &[("label", label), ("error", first_line(message))],
                    )
                });
                workspace.status_message_ok = Some(*success);
                if *success && refresh_after {
                    workspace.git_view.update(cx, |view, _| view.refresh());
                }
                cx.notify();
            }
            GitUiEvent::RepoOpened(path) => {
                workspace.config.repo.path = path.clone();
                workspace.config.push_recent(&path);
                let _ = config::save(&workspace.config);
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
            status_message_ok: None,
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

    /// 自绘标题栏（M1.5 合并行）：logo + 应用名 + 仓库 tab（pill，× 关闭）+ 分支徽标。
    /// 原 TabBar 整行并入此处省 28px；最小化/最大化/关闭由系统绘制（DWM）。
    fn title_bar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let repo = crate::git::dir_name(&self.config.repo.path).to_string();
        let has_repo = self.git_connected() || !repo.is_empty();
        let tab_label = if repo.is_empty() {
            SharedString::from(i18n::text(self.locale, "no-repo-open"))
        } else {
            SharedString::from(repo)
        };

        // 仓库 tab 关闭 ×（常态可见；无仓库时禁用态灰）
        let this = cx.entity();
        let close_tab = div()
            .id("repo-tab-close")
            .size(px(14.))
            .rounded_sm()
            .flex()
            .items_center()
            .justify_center()
            .when(has_repo, |el| {
                el.cursor(CursorStyle::PointingHand)
                    .hover(|el| el.bg(colors.list_hover))
            })
            .text_color(if has_repo {
                colors.muted_foreground
            } else {
                colors.muted_foreground.opacity(0.4)
            })
            .child(Icon::new(IconName::Close))
            .when(has_repo, |el| {
                el.on_click(move |_e, _w, cx| {
                    this.update(cx, |ws, cx| ws.close_repo_tab(cx));
                })
            });

        // 仓库 tab pill（bg 仅在有仓库时着色）
        let repo_tab = h_flex()
            .id("repo-tab")
            .h(px(22.))
            .px_2()
            .items_center()
            .gap_1()
            .rounded_md()
            .max_w(px(240.))
            .when(has_repo, |el| el.bg(colors.input))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(if has_repo {
                        colors.foreground
                    } else {
                        colors.muted_foreground
                    })
                    .child(tab_label),
            )
            .child(close_tab);

        // 分支徽标（点击 → 侧栏分支区闪烁）
        let branch = match &self.status {
            GitStatus::Ready(label) => {
                label.split(" · ").next().unwrap_or("").to_string()
            }
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
                    .gap_1()
                    .bg(colors.input)
                    .hover(|this| this.bg(colors.list_hover))
                    .cursor(CursorStyle::PointingHand)
                    .text_size(px(11.))
                    .text_color(colors.blue)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(colors.blue)
                            .child(crate::git::lucide("git-branch")),
                    )
                    .child(branch)
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
                .gap_3()
                .on_double_click(|_event, window, _cx| {
                    window.zoom_window();
                })
                // 应用标识：logo 图标 + 名称（点击不响应，仅展示）
                .child(
                    h_flex()
                        .items_center()
                        .gap_1p5()
                        .text_color(colors.blue)
                        .child(
                            div()
                                .text_size(px(16.))
                                .child(crate::git::lucide("git-branch")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.foreground)
                                .child("augur-git"),
                        ),
                )
                .child(repo_tab)
                .child(div().flex_1())
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
                Ok(Ok(Some(paths))) => {
                    paths.first().map(|p| p.to_string_lossy().into_owned())
                }
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
        self.status_message_ok = None;
        cx.notify();
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
            GitStatus::Scanning => {
                (i18n::text(self.locale, "status-scanning"), colors.warning)
            }
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
        // 消息语义色：成功绿 / 失败红 / 中性提示 muted（无横幅区，反馈全在状态栏）
        let msg_color = match self.status_message_ok {
            Some(true) => colors.green,
            Some(false) => colors.red,
            None => colors.muted_foreground,
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
                                .text_color(msg_color)
                                .child(SharedString::from(msg)),
                        )
                    })
                    .child(
                        div().text_size(px(11.)).text_color(color).child(text),
                    ),
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
                    .p_1()
                    .rounded_md()
                    .hover(|this| this.bg(cx.theme().colors.input))
                    .text_size(px(12.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(Icon::new(IconName::PanelLeftOpen))
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |workspace, cx| {
                            workspace.sidebar_collapsed = false;
                            cx.notify();
                        });
                    }),
            )
    }

    /// 主内容区：侧栏 | 中列 | 右栏（拖拽调宽监听挂根元素）
    fn main_content(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                    let new_w = f32::from(e.event.position.x)
                        .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
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
                    let new_h = (f32::from(h)
                        - STATUS_BAR_HEIGHT
                        - f32::from(e.event.position.y))
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

    fn welcome(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        welcome::render_welcome(self, window, cx)
    }

    fn settings_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        welcome::render_settings_overlay(self, cx)
    }
}

impl Render for Workspace {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let has_repo =
            !self.config.repo.path.is_empty() || self.git_connected();

        v_flex()
            .id("workspace")
            .size_full()
            .relative()
            .bg(colors.background)
            .child(self.title_bar(window, cx))
            .child(self.toolbar.clone())
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
