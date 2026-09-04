//! M1：仓库数据中枢（GitView）——工作线程句柄 + 快照事件派发
//!
//! 架构（镜像 rgitui 的 GitProject 职责 + augur-com 的双通道线程模式）：
//! - 本实体不渲染（不放布局树），只持有工作线程句柄并轮询事件
//! - 快照数据（分支/变更/日志）经 GitUiEvent 事件链分发给各面板
//! - 面板交互事件由 Workspace 汇总后经 GitCommand 下发工作线程

pub mod bottom_panel;
pub mod branch_compare;
pub mod changes_panel;
pub mod commit_message_dialog;
pub mod commit_preview;
pub mod diff_view;
pub mod graph;
pub mod panel;
mod revision_picker;
mod revision_picker_logic;
pub mod sidebar;
pub mod toolbar;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use gpui::{Context, EventEmitter, SharedString, Task};

use crate::core::diff::FileChange;
use crate::core::git::{
    self, BranchInfo, CheckoutTarget, CommitMessage, CompareRevision,
    FileStatus, GitError, GitEvent, LogScope, RefsInfo, WorkingTreeAction,
    WorkingTreeDiffKind, WorkingTreeScope, WorkingTreeScopeKind,
};
use crate::core::graph::LogRow;
use crate::core::i18n::{self, Locale};

/// GitView → Workspace 事件（事件即数据快照，面板各自持有副本）
#[derive(Clone, Debug)]
pub enum GitUiEvent {
    /// Repository status snapshot.
    StatusChanged {
        branch: String,
        head: Option<String>,
        upstream: Option<String>,
        ahead: usize,
        behind: usize,
        files: Vec<FileStatus>,
        branches: Vec<BranchInfo>,
    },
    /// One commit-graph log page; `replace` restarts the list.
    LogPageChanged {
        rows: Vec<LogRow>,
        replace: bool,
        has_more: bool,
    },
    /// 引用快照（侧栏 remotes/远程分支/标签/stash 分区）
    RefsChanged(RefsInfo),
    /// 选中提交的逐文件增删统计快照
    CommitFilesChanged {
        oid: String,
        files: Vec<FileChange>,
        merge_parent: Option<String>,
    },
    /// Full commit message snapshot for the graph hover preview.
    CommitMessageChanged { oid: String, message: CommitMessage },
    /// Structured selected-file commit diff.
    FileDiffChanged {
        oid: String,
        file: FileChange,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
    },
    /// Structured selected-file working-tree diff.
    WorkingTreeFileDiffChanged {
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
    },
    /// Non-fatal working-tree diff error for one request.
    WorkingTreeFileDiffError {
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
        detail: String,
    },
    /// Branch comparison file metadata.
    BranchCompareFiles {
        request_id: u64,
        files: Vec<FileChange>,
    },
    /// One file from a branch comparison.
    BranchCompareFileDiff {
        request_id: u64,
        file: FileChange,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
    },
    /// Non-fatal branch comparison error.
    BranchCompareError {
        request_id: u64,
        file: Option<FileChange>,
        detail: String,
    },
    /// All files for a branch comparison have been attempted.
    BranchCompareFinished { request_id: u64 },
    /// A comparison patch was written to `destination` with `bytes` bytes.
    BranchComparePatchExported {
        request_id: u64,
        destination: PathBuf,
        bytes: u64,
    },
    /// A comparison patch export failed without stopping the worker.
    BranchComparePatchError { request_id: u64, detail: String },
    /// Completed staged/working-tree mutation.
    WorkingTreeOperationFinished {
        request_id: u64,
        action: WorkingTreeAction,
        scope: WorkingTreeScopeKind,
        success: bool,
        detail: String,
    },
    /// 通用命令开始执行（fetch/pull/push/commit/show…）
    CommandStarted {
        label: String,
        /// Git subcommand (`args[0]`), used to derive a progress verb.
        subcommand: String,
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
    /// Non-fatal status refresh error; the Git worker remains available.
    StatusError(String),
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
                log::debug!(
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
            log::debug!(
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

    /// Set the commit-graph history scope and reload the first page.
    pub fn set_log_scope(&self, scope: LogScope) {
        if let Some(handle) = &self.handle {
            handle.log_query(scope);
        }
    }

    /// Request the next commit-graph page for the current scope.
    pub fn request_more_log_page(&self) {
        if let Some(handle) = &self.handle {
            handle.more_log_page();
        }
    }

    /// 执行任意 git 命令（fetch/pull/push/commit/checkout/show…）
    pub fn run(&self, label: impl Into<String>, args: Vec<String>) {
        if let Some(handle) = &self.handle {
            handle.run(label, args);
        }
    }

    /// Request a commit from staged changes, optionally amending the last commit.
    pub fn commit(&self, message: String, amend: bool) {
        log::info!("[git_commit] request queued: amend={amend}");
        if let Some(handle) = &self.handle {
            handle.commit(message, amend);
        }
    }

    /// Request a structured checkout operation on the Git worker.
    pub fn checkout(&self, target: CheckoutTarget) {
        log::info!("[git_checkout] requested target={target:?}");
        if let Some(handle) = &self.handle {
            handle.checkout(target);
        }
    }

    /// Request the complete message for a commit selected in the graph.
    pub fn copy_commit_message(&self, oid: String) {
        log::info!("[git_copy_message] requested oid={oid}");
        if let Some(handle) = &self.handle {
            handle.copy_commit_message(oid);
        }
    }

    /// 查询选中提交的逐文件增删统计（底部面板文件清单）
    pub fn commit_files(&self, oid: String) {
        if let Some(handle) = &self.handle {
            handle.commit_numstat(oid);
        }
    }

    /// Query the full commit message used by the graph hover preview.
    pub fn commit_message(&self, oid: String) {
        if let Some(handle) = &self.handle {
            handle.commit_message(oid);
        }
    }

    /// 查询选中提交内单文件 diff（底部面板右栏）
    pub fn file_diff(
        &self,
        oid: String,
        merge_parent: Option<String>,
        file: FileChange,
    ) {
        if let Some(handle) = &self.handle {
            handle.commit_file_diff(oid, merge_parent, file);
        }
    }

    /// Query each changed file in a commit for the aggregate diff view.
    pub fn file_diffs(
        &self,
        oid: String,
        merge_parent: Option<String>,
        files: Vec<FileChange>,
    ) {
        if let Some(handle) = &self.handle {
            for file in files {
                handle.commit_file_diff(
                    oid.clone(),
                    merge_parent.clone(),
                    file,
                );
            }
        }
    }

    /// Query one staged or unstaged working-tree file diff.
    pub fn working_tree_file_diff(
        &self,
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
    ) {
        log::debug!(
            "[git_diff] requesting working-tree file diff: request_id={}, kind={kind:?}",
            request_id
        );
        if let Some(handle) = &self.handle {
            handle.working_tree_file_diff(request_id, kind, file);
        }
    }

    /// Request a read-only comparison of two revisions.
    pub fn branch_compare(
        &self,
        request_id: u64,
        base: CompareRevision,
        target: CompareRevision,
    ) {
        log::info!(
            "[git_compare] requested: request_id={}, base={}, target={}",
            request_id,
            base.name,
            target.name
        );
        if let Some(handle) = &self.handle {
            handle.branch_compare(request_id, base, target);
        }
    }

    /// Cancel an in-flight revision comparison.
    pub fn cancel_branch_compare(&self) {
        if let Some(handle) = &self.handle {
            handle.cancel_branch_compare();
        }
    }

    /// Write the full diff between two revisions to a patch file.
    pub fn branch_compare_patch(
        &self,
        request_id: u64,
        base: CompareRevision,
        target: CompareRevision,
        destination: PathBuf,
    ) {
        log::info!(
            "[git_compare] patch export requested: request_id={}, base={}, target={}, file={}",
            request_id,
            base.name,
            target.name,
            destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unnamed>")
        );
        if let Some(handle) = &self.handle {
            handle.branch_compare_patch(request_id, base, target, destination);
        }
    }

    /// Apply a staged/working-tree mutation.
    pub fn working_tree_operation(
        &self,
        request_id: u64,
        action: WorkingTreeAction,
        scope: WorkingTreeScope,
    ) {
        log::info!(
            "[git_worktree] queueing operation: request_id={}, action={}, scope={:?}",
            request_id,
            action.description(),
            scope.kind()
        );
        if let Some(handle) = &self.handle {
            handle.working_tree_operation(request_id, action, scope);
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
                    head,
                    upstream,
                    files,
                    branches,
                    ahead,
                    behind,
                } => {
                    log::debug!(
                        "[git_view] status refreshed: branch={branch}, upstream={upstream:?}, files={}, branches={}, ahead={ahead}, behind={behind}",
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
                        head,
                        upstream,
                        ahead,
                        behind,
                        files,
                        branches,
                    });
                }
                GitEvent::LogPage {
                    rows,
                    replace,
                    has_more,
                } => {
                    log::debug!(
                        "[git_view] log page received: {} rows, replace={replace}, has_more={has_more}",
                        rows.len()
                    );
                    cx.emit(GitUiEvent::LogPageChanged {
                        rows,
                        replace,
                        has_more,
                    });
                }
                GitEvent::Refs(refs) => {
                    log::debug!(
                        "[git_view] refs refreshed: remotes={}, remote_branches={}, comparison_revisions={}, tags={}, stashes={}",
                        refs.remotes.len(),
                        refs.remote_branches.len(),
                        refs.comparison_revisions.len(),
                        refs.tags.len(),
                        refs.stashes.len()
                    );
                    cx.emit(GitUiEvent::RefsChanged(refs));
                }
                GitEvent::CommitFiles {
                    oid,
                    files,
                    merge_parent,
                } => {
                    log::debug!(
                        "[git_view] commit file list received: oid={oid}, files={}, merge_parent={}",
                        files.len(),
                        merge_parent.is_some()
                    );
                    cx.emit(GitUiEvent::CommitFilesChanged {
                        oid,
                        files,
                        merge_parent,
                    });
                }
                GitEvent::CommitMessage { oid, message } => {
                    log::debug!(
                        "[git_view] commit message received: oid={oid}, body_lines={}, co_authors={}",
                        message.body.lines().count(),
                        message.co_authors.len()
                    );
                    cx.emit(GitUiEvent::CommitMessageChanged { oid, message });
                }
                GitEvent::CommitFileDiff {
                    oid,
                    file,
                    patch,
                    old_source,
                    new_source,
                } => {
                    log::debug!(
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
                GitEvent::WorkingTreeFileDiff {
                    request_id,
                    kind,
                    file,
                    patch,
                    old_source,
                    new_source,
                } => {
                    log::debug!(
                        "[git_view] working-tree file diff received: request_id={}, kind={kind:?}, lines={}",
                        request_id,
                        patch.lines().count()
                    );
                    cx.emit(GitUiEvent::WorkingTreeFileDiffChanged {
                        request_id,
                        kind,
                        file,
                        patch,
                        old_source,
                        new_source,
                    });
                }
                GitEvent::WorkingTreeFileDiffError {
                    request_id,
                    kind,
                    file,
                    detail,
                } => {
                    log::warn!(
                        "[git_view] working-tree file diff failed: request_id={}, kind={kind:?}",
                        request_id
                    );
                    cx.emit(GitUiEvent::WorkingTreeFileDiffError {
                        request_id,
                        kind,
                        file,
                        detail,
                    });
                }
                GitEvent::BranchCompareFiles { request_id, files } => {
                    log::debug!(
                        "[git_view] branch comparison files received: request_id={}, files={}",
                        request_id,
                        files.len()
                    );
                    cx.emit(GitUiEvent::BranchCompareFiles {
                        request_id,
                        files,
                    });
                }
                GitEvent::BranchCompareFileDiff {
                    request_id,
                    file,
                    patch,
                    old_source,
                    new_source,
                } => {
                    log::debug!(
                        "[git_view] branch comparison file received: request_id={}, path={}, patch_bytes={}, old_source_bytes={}, new_source_bytes={}",
                        request_id,
                        file.path,
                        patch.len(),
                        old_source.as_ref().map_or(0, String::len),
                        new_source.as_ref().map_or(0, String::len)
                    );
                    cx.emit(GitUiEvent::BranchCompareFileDiff {
                        request_id,
                        file,
                        patch,
                        old_source,
                        new_source,
                    });
                }
                GitEvent::BranchCompareError {
                    request_id,
                    file,
                    detail,
                } => {
                    log::warn!(
                        "[git_view] branch comparison failed: request_id={}, file={}",
                        request_id,
                        file.as_ref()
                            .map(|file| file.path.as_str())
                            .unwrap_or("<request>")
                    );
                    cx.emit(GitUiEvent::BranchCompareError {
                        request_id,
                        file,
                        detail,
                    });
                }
                GitEvent::BranchCompareFinished { request_id } => {
                    log::info!(
                        "[git_view] branch comparison finished: request_id={}",
                        request_id
                    );
                    cx.emit(GitUiEvent::BranchCompareFinished { request_id });
                }
                GitEvent::BranchComparePatchExported {
                    request_id,
                    destination,
                    bytes,
                } => {
                    log::info!(
                        "[git_view] comparison patch exported: request_id={}, bytes={}",
                        request_id,
                        bytes
                    );
                    cx.emit(GitUiEvent::BranchComparePatchExported {
                        request_id,
                        destination,
                        bytes,
                    });
                }
                GitEvent::BranchComparePatchError { request_id, detail } => {
                    log::warn!(
                        "[git_view] comparison patch export failed: request_id={}",
                        request_id
                    );
                    cx.emit(GitUiEvent::BranchComparePatchError {
                        request_id,
                        detail,
                    });
                }
                GitEvent::WorkingTreeOperationFinished {
                    request_id,
                    action,
                    scope,
                    success,
                    detail,
                } => {
                    log::info!(
                        "[git_worktree] operation result: request_id={}, action={}, scope={scope:?}, success={success}",
                        request_id,
                        action.description()
                    );
                    cx.emit(GitUiEvent::WorkingTreeOperationFinished {
                        request_id,
                        action,
                        scope,
                        success,
                        detail,
                    });
                }
                GitEvent::StatusError(error) => {
                    log::warn!(
                        "[git_view] status refresh failed: {}",
                        error.key
                    );
                    cx.emit(GitUiEvent::StatusError(localized_error(
                        self.locale,
                        &error,
                    )));
                }
                GitEvent::CommandStarted { label, subcommand } => {
                    log::debug!("[git_view] command {label} started");
                    cx.emit(GitUiEvent::CommandStarted { label, subcommand });
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
                    } else if label == "copy-commit-message" {
                        log::info!(
                            "[git_copy_message] command completed: success={success}"
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
                    let message = localized_error(self.locale, &err);
                    cx.emit(GitUiEvent::Error(message.clone()));
                    self.handle = None;
                    self.rx = None;
                    keep_polling = false;
                    self.set_status(GitStatus::Error(message), cx);
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
