//! M0 骨架：暗黑主题 + 三区布局（左侧栏 / 仓库状态区 / 状态栏）。
//! 结构镜像 augur-com 的 workspace 模式，后续里程碑逐步填充：
//!   M1 仓库状态（已实现）| M2 提交历史 | M3 差异视图 | M4 提交/分支操作/打包
//!
//! 配置持久化：Workspace 持有 AppConfig 单一事实源，
//! 侧栏/状态区的任何变更经事件链回流，变更即存盘。

use gpui::*;
use gpui_component::{
    ActiveTheme, InteractiveElementExt, Root, TitleBar, h_flex,
    input::{Input, InputEvent, InputState},
    theme::{Theme, ThemeMode},
    v_flex,
};

use crate::core::config::{self, AppConfig};
use crate::git::{GitStatus, GitUiEvent, GitView};

/// 打开按钮高亮色（镜像 augur-com sendbar 的 BTN_BLUE）
const BTN_BLUE: u32 = 0x2F_81_F7;

pub fn run(app: Application) {
    app.run(|cx| {
        gpui_component::init(cx);

        // 暗黑主题（VSCode Dark+ 风格）
        Theme::change(ThemeMode::Dark, None, cx);

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
            width: px(660.),
            height: px(400.),
        }),
        ..Default::default()
    }
}

/// 侧栏事件
pub enum SidebarEvent {
    /// 打开仓库（用输入框路径）
    OpenRepo,
    /// 刷新状态
    Refresh,
    /// 打开最近仓库（输入框已回填）
    OpenRecent(String),
    /// 收起/展开侧栏
    ToggleCollapse,
}

pub struct Workspace {
    sidebar: Entity<Sidebar>,
    git_view: Entity<GitView>,
    /// 配置单一事实源（变更即存盘）
    config: AppConfig,
    /// 连接状态（状态栏显示）
    status: GitStatus,
    sidebar_collapsed: bool,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = config::load();
        let sidebar = cx.new(|cx| Sidebar::new(window, cx, &config));
        let git_view = cx.new(|cx| GitView::new(window, cx, config.view.show_untracked));

        // 侧栏事件 -> 打开仓库 / 刷新 / 持久化
        cx.subscribe(&sidebar, |workspace, _entity, event, cx| match event {
            SidebarEvent::OpenRepo => {
                let path = workspace.sidebar.read(cx).repo_path(cx);
                if workspace.git_view.read(cx).connected() {
                    // 已打开则先关后开（M0 简化，无仓库切换动画）
                    workspace.git_view.update(cx, |view, cx| view.close_repo(cx));
                }
                if !path.is_empty() {
                    workspace
                        .git_view
                        .update(cx, |view, cx| view.open_repo(&path, cx));
                }
            }
            SidebarEvent::Refresh => {
                workspace.git_view.update(cx, |view, _cx| view.refresh());
            }
            SidebarEvent::OpenRecent(path) => {
                if workspace.git_view.read(cx).connected() {
                    workspace.git_view.update(cx, |view, cx| view.close_repo(cx));
                }
                workspace
                    .git_view
                    .update(cx, |view, cx| view.open_repo(&path, cx));
            }
            SidebarEvent::ToggleCollapse => {
                workspace.sidebar_collapsed = !workspace.sidebar_collapsed;
                cx.notify();
            }
        })
        .detach();

        // 状态区事件 -> 状态栏 / MRU / 持久化
        cx.subscribe(&git_view, |workspace, _entity, event, cx| match event {
            GitUiEvent::StatusChanged { branch, changed } => {
                workspace.status =
                    GitStatus::Ready(format!("{branch} · {changed} 处变更"));
                cx.notify();
            }
            GitUiEvent::RepoOpened(path) => {
                workspace.config.repo.path = path.clone();
                workspace.config.push_recent(&path);
                let _ = config::save(&workspace.config);
                let recent = workspace.config.recent_repos.clone();
                workspace
                    .sidebar
                    .update(cx, |sidebar, _| sidebar.set_recent(recent));
            }
            GitUiEvent::Error(msg) => {
                workspace.status = GitStatus::Error(msg.clone());
                cx.notify();
            }
        })
        .detach();

        Self {
            sidebar,
            git_view,
            config,
            status: GitStatus::None,
            sidebar_collapsed: false,
        }
    }

    /// 标题栏：应用名 + 双击最大化。
    /// 注意：最小化/最大化/关闭按钮由系统原生绘制（DWM），不要自绘以免重叠。
    fn title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(
            h_flex()
                .id("title-bar-content")
                .w_full()
                .h_full()
                .items_center()
                .px_2()
                .on_double_click(|_event, window, _cx| {
                    window.zoom_window();
                })
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .text_color(cx.theme().colors.muted)
                        .child("augur-git"),
                ),
        )
    }

    /// 状态栏（内联渲染，直接读 workspace 状态）
    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, color) = match &self.status {
            GitStatus::None => ("未打开仓库".to_string(), cx.theme().colors.muted),
            GitStatus::Scanning => ("扫描中…".to_string(), cx.theme().colors.warning),
            GitStatus::Ready(label) => (format!("● {label}"), cx.theme().colors.green),
            GitStatus::Error(msg) => (format!("✗ {msg}"), cx.theme().colors.red),
        };
        let repo = &self.config.repo.path;
        let left = if repo.is_empty() {
            "未选择仓库".to_string()
        } else {
            format!("仓库: {repo}")
        };
        h_flex()
            .id("status-bar")
            .w_full()
            .h_6()
            .flex_shrink_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().colors.background)
            .px_3()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(cx.theme().colors.foreground)
                    .child("augur-git"),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(div().text_size(px(12.)).text_color(cx.theme().colors.muted).child(left))
                    .child(div().text_size(px(12.)).text_color(color).child(text)),
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
                    .text_color(cx.theme().colors.muted)
                    .child("»")
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |workspace, cx| {
                            workspace.sidebar_collapsed = false;
                            cx.notify();
                        });
                    }),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("workspace")
            .size_full()
            .bg(cx.theme().colors.background)
            .child(self.title_bar(window, cx))
            .child(
                h_flex()
                    .id("main-area")
                    .size_full()
                    .child(if self.sidebar_collapsed {
                        self.collapsed_rail(cx).into_any_element()
                    } else {
                        self.sidebar.clone().into_any_element()
                    })
                    .child(self.git_view.clone()),
            )
            .child(self.status_bar(cx))
    }
}

/// 左侧栏：仓库路径输入 + 打开/刷新 + 最近仓库
pub struct Sidebar {
    repo_path_input: Entity<InputState>,
    /// 最近打开的仓库（来自 config，打开成功后刷新）
    recent_repos: Vec<String>,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Sidebar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, config: &AppConfig) -> Self {
        let repo_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("仓库路径，如 D:\\repo")
                .default_value(config.repo.path.clone())
        });

        // 回车 = 打开仓库
        let input = repo_path_input.clone();
        cx.subscribe(&input, |_sidebar, _e, event, cx| {
            if matches!(event, InputEvent::PressEnter { secondary: false, .. }) {
                cx.emit(SidebarEvent::OpenRepo);
            }
        })
        .detach();

        Self {
            repo_path_input,
            recent_repos: config.recent_repos.clone(),
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
            .bg(if has_path { Hsla::from(rgb(BTN_BLUE)) } else { input_bg })
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

        // 最近仓库列表（点击 = 回填输入框 + 打开）
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
                    .hover(|this| this.bg(colors.input))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(colors.foreground)
                            .child(path.clone()),
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
            .id("sidebar")
            .w(px(250.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(colors.border)
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
                    .id("sidebar-recent")
                    .flex_1()
                    .overflow_y_scroll()
                    .w_full()
                    .p_2()
                    .gap_0p5()
                    .child(
                        div()
                            .px_2()
                            .text_size(px(12.))
                            .text_color(colors.muted)
                            .child("最近仓库"),
                    )
                    .children(recents),
            )
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sidebar(window, cx)
    }
}
