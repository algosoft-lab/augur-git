//! M1：仓库状态视图——分支 + 变更文件列表
//!
//! 架构（镜像 augur-com 的 serial/mod.rs）：
//! - 工作线程事件经 std mpsc 推送，UI 20ms 轮询 try_recv 派发
//! - 状态变更经 GitUiEvent 事件链回流 Workspace（状态栏/持久化）
//! - 后续里程碑：M2 提交历史 | M3 差异视图 | M4 提交/分支操作

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use gpui::*;
use gpui_component::{ActiveTheme, h_flex, v_flex, theme::ThemeColor};

use crate::core::git::{self, FileStatus, GitEvent};

/// GitView → Workspace 事件
#[derive(Clone, Debug)]
pub enum GitUiEvent {
    /// 状态更新（状态栏显示分支/变更数）
    StatusChanged { branch: String, changed: usize },
    /// 打开仓库成功（MRU 记录）
    RepoOpened(String),
    /// 错误（状态栏显示）
    Error(String),
}

/// 仓库连接状态（状态栏显示）
#[derive(Clone, Debug, Default)]
pub enum GitStatus {
    #[default]
    None,
    /// 首次扫描中（打开仓库到首个事件之间短暂出现）
    Scanning,
    /// 已就绪（label 为 分支 @ 仓库名）
    Ready(String),
    Error(String),
}

pub struct GitView {
    handle: Option<git::GitHandle>,
    rx: Option<mpsc::Receiver<GitEvent>>,
    status: GitStatus,
    /// 仓库绝对路径（显示/后续命令用）
    repo_path: String,
    branch: String,
    files: Vec<FileStatus>,
    /// 未跟踪文件显示开关（来自 config）
    show_untracked: bool,
}

impl EventEmitter<GitUiEvent> for GitView {}

impl GitView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, show_untracked: bool) -> Self {
        // 工作线程事件轮询（20ms，镜像 augur-com 的 poll_serial）
        cx.spawn(async move |this, cx| {
            loop {
                let _ = this.update(cx, |view, cx| view.poll_events(cx));
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await;
            }
        })
        .detach();

        Self {
            handle: None,
            rx: None,
            status: GitStatus::None,
            repo_path: String::new(),
            branch: String::new(),
            files: Vec::new(),
            show_untracked,
        }
    }

    /// 打开仓库（workspace 侧栏触发；路径校验同步执行，毫秒级）
    pub fn open_repo(&mut self, repo_path: &str, cx: &mut Context<Self>) {
        if self.handle.is_some() {
            return;
        }
        self.set_status(GitStatus::Scanning, cx);

        let (tx, rx) = mpsc::channel::<GitEvent>();
        match git::spawn_open(repo_path.to_string(), tx) {
            Ok(handle) => {
                log::info!("git: 已打开仓库 {repo_path}");
                self.repo_path = repo_path.to_string();
                self.handle = Some(handle);
                self.rx = Some(rx);
                self.set_status(
                    GitStatus::Ready(format!("扫描中 @ {}", dir_name(repo_path))),
                    cx,
                );
                cx.emit(GitUiEvent::RepoOpened(repo_path.to_string()));
            }
            Err(msg) => {
                log::error!("git: 打开失败: {msg}");
                self.handle = None;
                self.rx = None;
                self.repo_path.clear();
                self.branch.clear();
                self.files.clear();
                self.set_status(GitStatus::Error(msg), cx);
            }
        }
    }

    /// 关闭仓库（工作线程收到 Close 后退出）
    pub fn close_repo(&mut self, _cx: &mut Context<Self>) {
        if let Some(handle) = &self.handle {
            handle.close();
            self.handle = None;
            self.rx = None;
        }
    }

    /// 请求刷新状态
    pub fn refresh(&self) {
        if let Some(handle) = &self.handle {
            handle.refresh();
        }
    }

    /// 是否已打开仓库
    pub fn connected(&self) -> bool {
        matches!(self.status, GitStatus::Ready(_))
    }

    fn set_status(&mut self, status: GitStatus, cx: &mut Context<Self>) {
        if let GitStatus::Error(msg) = &status {
            cx.emit(GitUiEvent::Error(msg.clone()));
        }
        self.status = status;
        cx.notify();
    }

    /// 轮询工作线程事件并派发
    fn poll_events(&mut self, cx: &mut Context<Self>) {
        let Some(rx) = &self.rx else {
            return;
        };
        // 先 drain 事件（借用 rx），再处理（需要 &mut self）
        let mut events = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            events.push(evt);
        }
        if events.is_empty() {
            return;
        }

        for evt in events {
            match evt {
                GitEvent::Status { branch, files } => {
                    self.branch = branch.clone();
                    self.files = if self.show_untracked {
                        files
                    } else {
                        files
                            .into_iter()
                            .filter(|f| f.code() != '?')
                            .collect()
                    };
                    log::info!("git: 状态刷新 分支={branch} 变更={}", self.files.len());
                    self.set_status(
                        GitStatus::Ready(format!("{branch} @ {}", dir_name(&self.repo_path))),
                        cx,
                    );
                    cx.emit(GitUiEvent::StatusChanged {
                        branch,
                        changed: self.files.len(),
                    });
                }
                GitEvent::Error(msg) => {
                    log::error!("git: {msg}");
                    self.branch.clear();
                    self.files.clear();
                    self.handle = None;
                    self.rx = None;
                    self.set_status(GitStatus::Error(msg), cx);
                }
            }
        }
    }
}

impl Render for GitView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        v_flex()
            .id("git-view")
            .size_full()
            .bg(colors.background)
            .child(self.header(&colors))
            .child(self.file_list(&colors))
            .child(self.placeholder_sections(&colors))
    }
}

impl GitView {
    /// 头部：分支 + 变更统计
    fn header(&self, colors: &ThemeColor) -> impl IntoElement {
        let counts = count_by_code(&self.files);
        h_flex()
            .id("git-header")
            .w_full()
            .h_9()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .child(
                h_flex()
                    .id("branch-badge")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(colors.input)
                    .text_size(px(12.))
                    .text_color(colors.blue)
                    .child(if self.branch.is_empty() {
                        SharedString::from("未打开仓库")
                    } else {
                        SharedString::from(format!("⎇ {}", self.branch))
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(status_pill(colors, 'M', counts.get(&'M').copied().unwrap_or(0)))
                    .child(status_pill(colors, 'A', counts.get(&'A').copied().unwrap_or(0)))
                    .child(status_pill(colors, 'D', counts.get(&'D').copied().unwrap_or(0)))
                    .child(status_pill(colors, '?', counts.get(&'?').copied().unwrap_or(0))),
            )
            .child(div().flex_1())
    }

    /// 变更文件列表（M0 全量重绘；行缓存渲染 M2 日志视图再引入）
    fn file_list(&self, colors: &ThemeColor) -> impl IntoElement {
        let rows: Vec<_> = self
            .files
            .iter()
            .map(|f| {
                let (color, label) = status_style(colors, f.code());
                h_flex()
                    .id(SharedString::from(format!("file-{}", f.path)))
                    .w_full()
                    .h_6()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .child(
                        div()
                            .w(px(22.))
                            .text_size(px(12.))
                            .text_color(color)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(colors.foreground)
                            .child(f.path.clone()),
                    )
            })
            .collect();

        v_flex()
            .id("git-files")
            .flex_1()
            .overflow_y_scroll()
            .child(if rows.is_empty() {
                v_flex()
                    .id("git-empty")
                    .size_full()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(colors.muted)
                            .child(match &self.status {
                                GitStatus::None => SharedString::from("选择或打开一个 Git 仓库（左侧栏）"),
                                GitStatus::Scanning => SharedString::from("扫描中…"),
                                GitStatus::Error(msg) => SharedString::from(format!("✗ {msg}")),
                                GitStatus::Ready(_) => SharedString::from("✓ 工作区干净"),
                            }),
                    )
                    .into_any_element()
            } else {
                v_flex().id("git-file-rows").children(rows).into_any_element()
            })
    }

    /// 占位区块：提交区/历史/差异（后续里程碑填充）
    fn placeholder_sections(&self, colors: &ThemeColor) -> impl IntoElement {
        v_flex()
            .id("git-placeholders")
            .w_full()
            .flex_shrink_0()
            .border_t_1()
            .border_color(colors.border)
            .child(placeholder_row(colors, "提交区", "M2 里程碑"))
            .child(placeholder_row(colors, "提交历史", "M3 里程碑"))
    }
}

/// 状态码统计（索引/工作区聚合）
fn count_by_code(files: &[FileStatus]) -> HashMap<char, usize> {
    let mut counts = HashMap::new();
    for f in files {
        *counts.entry(f.code()).or_insert(0) += 1;
    }
    counts
}

/// 变更数小徽标
fn status_pill(colors: &ThemeColor, code: char, count: usize) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("pill-{code}")))
        .px_1()
        .rounded_sm()
        .bg(colors.input)
        .text_size(px(11.))
        .text_color(status_color(colors, code))
        .child(format!("{code}{count}"))
}

/// 状态码 → 颜色/标签
fn status_style(colors: &ThemeColor, code: char) -> (Hsla, &'static str) {
    let color = status_color(colors, code);
    let label = match code {
        'M' => "修改",
        'A' => "新增",
        'D' => "删除",
        'R' => "重命名",
        'C' => "复制",
        'U' => "冲突",
        _ => "未跟踪",
    };
    (color, label)
}

fn status_color(colors: &ThemeColor, code: char) -> Hsla {
    match code {
        'M' | 'R' | 'C' => colors.warning,
        'A' => colors.green,
        'D' | 'U' => colors.red,
        _ => colors.muted,
    }
}

/// 占位行（后续里程碑替换为真实区块）
fn placeholder_row(colors: &ThemeColor, title: &str, milestone: &str) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("ph-{title}")))
        .w_full()
        .h_7()
        .items_center()
        .px_3()
        .gap_2()
        .child(
            div()
                .w(px(64.))
                .text_size(px(12.))
                .text_color(colors.muted)
                .child(SharedString::from(title)),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .text_color(colors.muted)
                .child(SharedString::from(format!("{milestone} 实现"))),
        )
}

/// 路径尾段目录名（显示用）
fn dir_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}
