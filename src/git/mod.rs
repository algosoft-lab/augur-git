//! M1：仓库数据中枢（GitView）——工作线程句柄 + 快照事件派发
//!
//! 架构（镜像 rgitui 的 GitProject 职责 + augur-com 的双通道线程模式）：
//! - 本实体不渲染（不放布局树），只持有工作线程句柄并轮询事件
//! - 快照数据（分支/变更/日志）经 GitUiEvent 事件链分发给各面板
//! - 面板交互事件由 Workspace 汇总后经 GitCommand 下发工作线程

pub mod bottom_panel;
pub mod diff_view;
pub mod graph;
pub mod panel;
pub mod sidebar;
pub mod toolbar;

use std::sync::mpsc;
use std::time::Duration;

use gpui::{Context, EventEmitter, SharedString, Task};

use crate::core::diff::FileChange;
use crate::core::git::{
    self, BranchInfo, CheckoutTarget, FileStatus, GitError, GitEvent, RefsInfo,
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
    /// Structured selected-file commit diff.
    FileDiffChanged {
        oid: String,
        file: FileChange,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
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
    /// Foreground event delivery is enabled only for the active repository tab.
    poll_task: Option<Task<()>>,
    status: GitStatus,
    /// 仓库绝对路径（显示/后续命令用）
    repo_path: String,
    /// 界面语言（错误文案本地化用；Workspace 切换语言时同步）
    locale: Locale,
}

impl EventEmitter<GitUiEvent> for GitView {}

impl GitView {
    pub fn new(locale: Locale, _cx: &mut Context<Self>) -> Self {
        Self {
            handle: None,
            rx: None,
            poll_task: None,
            status: GitStatus::None,
            repo_path: String::new(),
            locale,
        }
    }

    /// Enable or disable foreground delivery of worker events for this tab.
    ///
    /// Git work continues on the worker thread while a tab is inactive; only
    /// UI event consumption is paused. Events remain queued in `rx` until the
    /// tab is activated again.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if active {
            if self.poll_task.as_ref().is_some_and(|task| task.is_ready()) {
                self.poll_task = None;
            }
            if self.poll_task.is_none() {
                log::info!(
                    "[workspace_tabs] GitView event polling started: {}",
                    dir_name(&self.repo_path)
                );
                self.poll_task = Some(cx.spawn(async move |this, cx| {
                    loop {
                        let keep_polling = this
                            .update(cx, |view, cx| view.poll_events(cx))
                            .unwrap_or(false);
                        if !keep_polling {
                            break;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(20))
                            .await;
                    }
                }));
            }
        } else if self.poll_task.take().is_some() {
            log::info!(
                "[workspace_tabs] GitView event polling stopped: {}",
                dir_name(&self.repo_path)
            );
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
        if self.poll_task.take().is_some() {
            log::info!(
                "[workspace_tabs] GitView event polling stopped: {}",
                dir_name(&self.repo_path)
            );
        }
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

    /// Request a structured checkout operation on the Git worker.
    pub fn checkout(&self, target: CheckoutTarget) {
        log::info!("[git_checkout] requested target={target:?}");
        if let Some(handle) = &self.handle {
            handle.checkout(target);
        }
    }

    /// 查询选中提交的逐文件增删统计（底部面板文件清单）
    pub fn commit_files(&self, oid: String) {
        if let Some(handle) = &self.handle {
            handle.commit_numstat(oid);
        }
    }

    /// 查询选中提交内单文件 diff（底部面板右栏）
    pub fn file_diff(&self, oid: String, file: FileChange) {
        if let Some(handle) = &self.handle {
            handle.commit_file_diff(oid, file);
        }
    }

    /// Query each changed file in a commit for the aggregate diff view.
    pub fn file_diffs(&self, oid: String, files: Vec<FileChange>) {
        if let Some(handle) = &self.handle {
            for file in files {
                handle.commit_file_diff(oid.clone(), file);
            }
        }
    }

    fn set_status(&mut self, status: GitStatus, cx: &mut Context<Self>) {
        if let GitStatus::Error(msg) = &status {
            cx.emit(GitUiEvent::Error(msg.clone()));
        }
        self.status = status;
    }

    /// 轮询工作线程事件并派发
    fn poll_events(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };
        // 先 drain 事件（借用 rx），再处理（需要 &mut self）
        let mut events = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            events.push(evt);
        }
        if events.is_empty() {
            return true;
        }

        let mut keep_polling = true;
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
                GitEvent::CommitFileDiff {
                    oid,
                    file,
                    patch,
                    old_source,
                    new_source,
                } => {
                    log::info!(
                        "[git_view] commit file diff received: oid={oid}, lines={}",
                        patch.lines().count()
                    );
                    cx.emit(GitUiEvent::FileDiffChanged {
                        oid,
                        file,
                        patch,
                        old_source,
                        new_source,
                    });
                }
                GitEvent::CommandDone {
                    label,
                    success,
                    message,
                } => {
                    if label == "checkout" {
                        log::info!(
                            "[git_checkout] command completed: success={success}"
                        );
                    }
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
                    keep_polling = false;
                    self.set_status(
                        GitStatus::Error(localized_error(self.locale, &err)),
                        cx,
                    );
                }
            }
        }
        keep_polling && self.rx.is_some()
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
