//! M1：仓库数据中枢（GitView）——工作线程句柄 + 快照事件派发
//!
//! 架构（镜像 rgitui 的 GitProject 职责 + augur-com 的双通道线程模式）：
//! - 本实体不渲染（不放布局树），只持有工作线程句柄并轮询事件
//! - 快照数据（分支/变更/日志）经 GitUiEvent 事件链分发给各面板
//! - 面板交互事件由 Workspace 汇总后经 GitCommand 下发工作线程

pub mod graph;
pub mod panel;
pub mod sidebar;
pub mod toolbar;

use std::sync::mpsc;
use std::time::Duration;

use gpui::{Context, EventEmitter, SharedString};

use crate::core::git::{
    self, BranchInfo, FileChange, FileStatus, GitError, GitEvent, RefsInfo,
};
use crate::core::graph::LogRow;
use crate::core::i18n::{self, Locale};

/// GitView → Workspace 事件（事件即数据快照，面板各自持有副本）
#[derive(Clone, Debug)]
pub enum GitUiEvent {
    /// 仓库状态快照
    StatusChanged {
        branch: String,
        ahead: usize,
        behind: usize,
        files: Vec<FileStatus>,
        branches: Vec<BranchInfo>,
    },
    /// 提交日志快照
    LogChanged { rows: Vec<LogRow> },
    /// 引用快照（侧栏 remotes/远程分支/标签/stash 分区）
    RefsChanged(RefsInfo),
    /// 选中提交的逐文件增删统计快照
    CommitFilesChanged { oid: String, files: Vec<FileChange> },
    /// 选中提交内单文件 diff 文本
    FileDiffChanged {
        oid: String,
        path: String,
        diff: String,
    },
    /// 通用命令执行结果（fetch/pull/push/commit/show…）
    CommandDone {
        label: String,
        success: bool,
        message: String,
    },
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
    /// 界面语言（错误文案本地化用；Workspace 切换语言时同步）
    locale: Locale,
}

impl EventEmitter<GitUiEvent> for GitView {}

impl GitView {
    pub fn new(locale: Locale, cx: &mut Context<Self>) -> Self {
        // 工作线程事件轮询（20ms，镜像 augur-com 的 poll_serial）
        cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |view, cx| view.poll_events(cx)).is_err() {
                    break;
                }
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
            locale,
        }
    }

    /// 切换语言（Workspace::set_language 同步）
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// 打开仓库（workspace 触发；路径校验同步执行，毫秒级）
    pub fn open_repo(&mut self, repo_path: &str, cx: &mut Context<Self>) {
        if self.handle.is_some() {
            return;
        }
        self.set_status(GitStatus::Scanning, cx);

        let (tx, rx) = mpsc::channel::<GitEvent>();
        match git::spawn_open(repo_path.to_string(), tx) {
            Ok(handle) => {
                log::info!("[git_view] repository opened");
                self.repo_path = repo_path.to_string();
                self.handle = Some(handle);
                self.rx = Some(rx);
                self.set_status(
                    GitStatus::Ready(i18n::text_args(
                        self.locale,
                        "status-scanning-at",
                        &[("repo", dir_name(repo_path))],
                    )),
                    cx,
                );
                cx.emit(GitUiEvent::RepoOpened(repo_path.to_string()));
            }
            Err(err) => {
                log::error!("[git_view] repository open failed: {}", err.key);
                self.handle = None;
                self.rx = None;
                self.repo_path.clear();
                self.set_status(
                    GitStatus::Error(localized_error(self.locale, &err)),
                    cx,
                );
            }
        }
    }

    /// 关闭仓库（工作线程收到 Close 后退出）
    pub fn close_repo(&mut self) {
        if let Some(handle) = &self.handle {
            handle.close();
            self.handle = None;
            self.rx = None;
        }
    }

    /// 请求刷新仓库快照
    pub fn refresh(&self) {
        if let Some(handle) = &self.handle {
            handle.refresh();
        }
    }

    /// 执行任意 git 命令（fetch/pull/push/commit/checkout/show…）
    pub fn run(&self, label: impl Into<String>, args: Vec<String>) {
        if let Some(handle) = &self.handle {
            handle.run(label, args);
        }
    }

    /// 查询选中提交的逐文件增删统计（底部面板文件清单）
    pub fn commit_files(&self, oid: String) {
        if let Some(handle) = &self.handle {
            handle.commit_numstat(oid);
        }
    }

    /// 查询选中提交内单文件 diff（底部面板右栏）
    pub fn file_diff(&self, oid: String, path: String) {
        if let Some(handle) = &self.handle {
            handle.commit_file_diff(oid, path);
        }
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
                GitEvent::Status {
                    branch,
                    files,
                    branches,
                    ahead,
                    behind,
                } => {
                    log::info!(
                        "[git_view] status refreshed: branch={branch}, files={}, branches={}, ahead={ahead}, behind={behind}",
                        files.len(),
                        branches.len()
                    );
                    let repo = dir_name(&self.repo_path);
                    self.set_status(
                        GitStatus::Ready(format!("{branch} @ {repo}")),
                        cx,
                    );
                    cx.emit(GitUiEvent::StatusChanged {
                        branch,
                        ahead,
                        behind,
                        files,
                        branches,
                    });
                }
                GitEvent::Log { rows } => {
                    log::info!("[git_view] log refreshed: {} rows", rows.len());
                    cx.emit(GitUiEvent::LogChanged { rows });
                }
                GitEvent::Refs(refs) => {
                    log::info!(
                        "[git_view] refs refreshed: remotes={}, remote_branches={}, tags={}, stashes={}",
                        refs.remotes.len(),
                        refs.remote_branches.len(),
                        refs.tags.len(),
                        refs.stashes.len()
                    );
                    cx.emit(GitUiEvent::RefsChanged(refs));
                }
                GitEvent::CommitFiles { oid, files } => {
                    log::info!(
                        "[git_view] commit file list received: oid={oid}, files={}",
                        files.len()
                    );
                    cx.emit(GitUiEvent::CommitFilesChanged { oid, files });
                }
                GitEvent::CommitFileDiff { oid, path, diff } => {
                    log::info!(
                        "[git_view] commit file diff received: oid={oid}, lines={}",
                        diff.lines().count()
                    );
                    cx.emit(GitUiEvent::FileDiffChanged { oid, path, diff });
                }
                GitEvent::CommandDone {
                    label,
                    success,
                    message,
                } => {
                    log::info!(
                        "[git_view] command {label}: {}",
                        if success { "succeeded" } else { "failed" }
                    );
                    cx.emit(GitUiEvent::CommandDone {
                        label,
                        success,
                        message,
                    });
                }
                GitEvent::Error(err) => {
                    log::error!("[git_view] Git operation failed: {}", err.key);
                    self.handle = None;
                    self.rx = None;
                    self.set_status(
                        GitStatus::Error(localized_error(self.locale, &err)),
                        cx,
                    );
                }
            }
        }
    }
}

/// 路径尾段目录名（显示用）
pub fn dir_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// 错误载荷 → 本地化文案（core 传 i18n 键 + 原始串，展示侧拼接；统一 { $detail } 占位）
fn localized_error(locale: Locale, err: &GitError) -> String {
    i18n::text_args(locale, err.key, &[("detail", &err.detail)])
}

/// 共享字符串工具（面板常用）
pub fn shared(s: impl Into<String>) -> SharedString {
    SharedString::from(s.into())
}

/// 本地 lucide 图标（assets/icons/*.svg，经 main.rs AppAssets 提供；
/// 内置 IconName 枚举不含这些 git 类图标，只能按路径引用）
pub fn lucide(name: &'static str) -> gpui_component::Icon {
    gpui_component::Icon::empty()
        .path(SharedString::from(format!("icons/{name}.svg")))
}
