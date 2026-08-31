//! M1：Git 命令层（镜像 augur-com 的 core/serial.rs 双通道线程模式）
//!
//! 架构：
//! - 专用工作线程跑阻塞式 `git` 子进程，事件经 `std::sync::mpsc` 推给 UI（20ms 轮询 try_recv）
//! - UI → 后台指令：`std::sync::mpsc`（send 无阻塞，即发即返）
//! - 读写全走后台线程，UI 线程零阻塞
//! - 当前调用系统 git 可执行文件（PATH 查找）；后续里程碑可换 git2/libgit2 做对象级访问
//!
//! 输出解析全部为纯函数（可单测），解析规则见各函数注释。

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::core::commit_diff::{
    CommitDiffContext, merge_numstat_args, merge_patch_args, merge_raw_args,
    parent_query_args, parse_parent_line,
};
#[allow(unused_imports)]
pub use crate::core::diff::{
    DiffLine, DiffLineKind, FileChange, FileChangeStatus, parse_diff,
    stat_blocks,
};
use crate::core::diff::{merge_numstat, parse_numstat, parse_raw_records};
use crate::core::graph::LogRow;

mod branch_compare;
mod working_tree;

/// The kind of revision exposed by the comparison selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompareRevisionKind {
    Local,
    Remote,
    Tag,
    Commit,
}

/// A revision accepted by the read-only comparison worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareRevision {
    /// Short display value (branch name, tag name, or abbreviated SHA).
    pub name: String,
    /// Fully qualified ref or user-entered commit SHA passed to Git.
    pub full_name: String,
    pub kind: CompareRevisionKind,
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn git_command() -> Command {
    let mut command = Command::new("git");

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    command
}

/// 错误载荷：key 为 core/i18n 的文案键，detail 为原始错误串
/// （本层不做本地化，展示侧有 locale 时经 text_args 拼接，镜像 augur-pdf）
#[derive(Clone, Debug)]
pub struct GitError {
    pub key: &'static str,
    pub detail: String,
}

impl GitError {
    fn new(key: &'static str, detail: impl Into<String>) -> Self {
        Self {
            key,
            detail: detail.into(),
        }
    }
}

/// Co-author trailer parsed from a commit message body
/// (`Co-authored-by: Name <email>`)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoAuthor {
    pub name: String,
    /// Empty when the trailer carries no `<email>` part
    pub email: String,
}

impl CoAuthor {
    /// Display form ("Name <email>" or just "Name")
    pub fn display(&self) -> String {
        if self.email.is_empty() {
            self.name.clone()
        } else {
            format!("{} <{}>", self.name, self.email)
        }
    }
}

/// Full commit message (`git show -s --format=%B` parse product)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitMessage {
    pub subject: String,
    /// Message body after the subject with `Co-authored-by:` trailers removed
    pub body: String,
    pub co_authors: Vec<CoAuthor>,
}

/// 引用快照（remote / 远程分支 / 标签 / stash 清单，侧栏分区显示）
#[derive(Clone, Debug, Default)]
pub struct RefsInfo {
    /// 远程名清单（git remote）
    pub remotes: Vec<String>,
    /// 远程分支短名（origin/main 等，git branch -r）
    pub remote_branches: Vec<String>,
    /// 标签名（按创建时间倒序）
    pub tags: Vec<String>,
    /// stash 描述（"stash@{n}: " 前缀已剥除）
    pub stashes: Vec<String>,
    /// Local/remote branches and tags available to revision comparison.
    pub comparison_revisions: Vec<CompareRevision>,
}

/// 后台 → UI 事件
pub enum GitEvent {
    /// Repository status, tracked upstream, changed files, and branch list.
    Status {
        branch: String,
        /// Tracked upstream ref, when the current branch has one.
        upstream: Option<String>,
        files: Vec<FileStatus>,
        /// 本地分支列表（(名字, 是否当前分支)）
        branches: Vec<BranchInfo>,
        /// 领先上游提交数
        ahead: usize,
        /// 落后上游提交数
        behind: usize,
    },
    /// Commit log rows with parents for active-lane layout.
    Log { rows: Vec<LogRow> },
    /// 引用快照（侧栏 remotes/远程分支/标签/stash 分区）
    Refs(RefsInfo),
    /// Commit file metadata and line counts for the selected commit.
    CommitFiles {
        oid: String,
        files: Vec<FileChange>,
        merge_parent: Option<String>,
    },
    /// Full commit message for the selected or hovered commit.
    CommitMessage { oid: String, message: CommitMessage },
    /// Structured single-file commit diff payload.
    CommitFileDiff {
        oid: String,
        file: FileChange,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
    },
    /// Structured single-file working-tree diff payload.
    WorkingTreeFileDiff {
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
    },
    /// A working-tree diff failed without stopping the Git worker.
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
    /// A non-fatal branch comparison error. `file` is `None` for a request-level error.
    BranchCompareError {
        request_id: u64,
        file: Option<FileChange>,
        detail: String,
    },
    /// All files for a branch comparison have been attempted.
    BranchCompareFinished { request_id: u64 },
    /// A staged/working-tree mutation completed without stopping the worker.
    WorkingTreeOperationFinished {
        request_id: u64,
        action: WorkingTreeAction,
        scope: WorkingTreeScopeKind,
        success: bool,
        detail: String,
    },
    /// Status parsing or execution failed, but the worker can continue.
    StatusError(GitError),
    /// 通用命令执行结果（fetch/pull/push/commit/show…）
    CommandDone {
        label: String,
        success: bool,
        message: String,
    },
    /// 命令执行出错（key 为 i18n 键，展示侧本地化）
    Error(GitError),
}

/// 单个文件变更（git status --porcelain 解析）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStatus {
    /// 索引状态字符（M/A/D/R/C/U，空格 = 无）
    pub index: char,
    /// 工作区状态字符
    pub worktree: char,
    /// 文件路径
    pub path: String,
    /// Original path for a rename, when Git reports one.
    pub old_path: Option<String>,
}

impl FileStatus {
    /// 聚合状态码（显示用：索引优先于工作区）
    pub fn code(&self) -> char {
        if self.index != ' ' {
            self.index
        } else {
            self.worktree
        }
    }

    /// Whether the index contains a staged change.
    pub fn has_staged_changes(&self) -> bool {
        self.is_staged()
    }

    /// Whether the working tree contains a change relative to the index.
    pub fn has_worktree_changes(&self) -> bool {
        self.worktree != ' '
    }

    /// Whether Git reports an unmerged index/worktree entry.
    pub fn is_conflicted(&self) -> bool {
        matches!(
            (self.index, self.worktree),
            ('U', _) | (_, 'U') | ('D', 'D') | ('A', 'A')
        )
    }

    /// Whether this entry represents an untracked file.
    pub fn is_untracked(&self) -> bool {
        self.index == '?' && self.worktree == '?'
    }

    /// Whether the file has a staged change, retaining the legacy API name.
    pub fn is_staged(&self) -> bool {
        self.index != ' ' && self.index != '?' && !self.is_conflicted()
    }

    /// Return the status character relevant to a particular working-tree group.
    pub fn code_for(&self, staged: bool) -> char {
        let code = if staged {
            if self.index != ' ' {
                self.index
            } else {
                self.worktree
            }
        } else if self.worktree != ' ' {
            self.worktree
        } else {
            self.index
        };
        if code == ' ' { self.code() } else { code }
    }
}

/// Which side of the working tree is being compared by a diff request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingTreeDiffKind {
    Staged,
    Unstaged,
}

/// A mutating operation exposed by the working-tree panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingTreeAction {
    Stage,
    Unstage,
    Discard,
}

impl WorkingTreeAction {
    pub fn description(self) -> &'static str {
        match self {
            Self::Stage => "stage",
            Self::Unstage => "unstage",
            Self::Discard => "discard",
        }
    }
}

/// The captured file snapshot targeted by a working-tree operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkingTreeScope {
    File(FileStatus),
    All(Vec<FileStatus>),
}

impl WorkingTreeScope {
    pub fn files(&self) -> Vec<&FileStatus> {
        match self {
            Self::File(file) => vec![file],
            Self::All(files) => files.iter().collect(),
        }
    }

    pub fn kind(&self) -> WorkingTreeScopeKind {
        match self {
            Self::File(_) => WorkingTreeScopeKind::File,
            Self::All(_) => WorkingTreeScopeKind::All,
        }
    }
}

/// Whether an operation targeted one file or a whole panel group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingTreeScopeKind {
    File,
    All,
}

/// 本地分支信息
#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    /// 是否当前 HEAD 分支
    pub is_head: bool,
}

/// A ref that can be checked out from the user interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckoutTarget {
    LocalBranch(String),
    RemoteBranch(String),
    Tag(String),
    Commit(String),
}

/// UI → 后台指令
pub enum GitCommand {
    /// 刷新仓库快照（status → branch → log 顺序执行，各发事件）
    Refresh,
    /// 执行任意 git 命令（label 供 UI 显示；args 不含 "git" 本身）
    Run { label: String, args: Vec<String> },
    /// Query file metadata and line counts for the selected commit.
    CommitNumstat { oid: String },
    /// 查询提交的完整提交信息（git show -s --format=%B）
    CommitMessage { oid: String },
    /// Query a structured single-file commit diff.
    CommitFileDiff {
        oid: String,
        merge_parent: Option<String>,
        file: FileChange,
    },
    /// Query a single file from the index or working tree.
    WorkingTreeFileDiff {
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
    },
    /// Compare two revisions without checkout.
    BranchCompare {
        request_id: u64,
        base: CompareRevision,
        target: CompareRevision,
    },
    /// Apply a staged/working-tree mutation to a captured file snapshot.
    WorkingTreeOperation {
        request_id: u64,
        action: WorkingTreeAction,
        scope: WorkingTreeScope,
    },
    /// 关闭工作线程
    Close,
}

/// 工作线程句柄（UI 侧持有）
pub struct GitHandle {
    cmd_tx: Sender<GitCommand>,
    compare_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl GitHandle {
    /// 请求刷新仓库快照（std mpsc 无界通道，send 即返回）
    pub fn refresh(&self) {
        let _ = self.cmd_tx.send(GitCommand::Refresh);
    }

    /// 执行任意 git 命令
    pub fn run(&self, label: impl Into<String>, args: Vec<String>) {
        let _ = self.cmd_tx.send(GitCommand::Run {
            label: label.into(),
            args,
        });
    }

    /// Check out a branch, tag, or commit using structured Git arguments.
    pub fn checkout(&self, target: CheckoutTarget) {
        self.run("checkout", checkout_args(target));
    }

    /// Create a commit from the staged changes, optionally amending HEAD.
    pub fn commit(&self, message: String, amend: bool) {
        self.run("commit", commit_args(message, amend));
    }

    /// Query the complete commit message without including the commit diff.
    pub fn copy_commit_message(&self, oid: String) {
        self.run("copy-commit-message", commit_message_args(oid));
    }

    /// 查询提交的逐文件增删统计
    pub fn commit_numstat(&self, oid: String) {
        let _ = self.cmd_tx.send(GitCommand::CommitNumstat { oid });
    }

    /// 查询提交的完整提交信息
    pub fn commit_message(&self, oid: String) {
        let _ = self.cmd_tx.send(GitCommand::CommitMessage { oid });
    }

    /// Query a structured single-file commit diff.
    pub fn commit_file_diff(
        &self,
        oid: String,
        merge_parent: Option<String>,
        file: FileChange,
    ) {
        let _ = self.cmd_tx.send(GitCommand::CommitFileDiff {
            oid,
            merge_parent,
            file,
        });
    }

    /// Query a single staged or unstaged working-tree file diff.
    pub fn working_tree_file_diff(
        &self,
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
    ) {
        let _ = self.cmd_tx.send(GitCommand::WorkingTreeFileDiff {
            request_id,
            kind,
            file,
        });
    }

    /// Start a read-only revision comparison and cancel the previous request.
    pub fn branch_compare(
        &self,
        request_id: u64,
        base: CompareRevision,
        target: CompareRevision,
    ) {
        self.compare_generation
            .store(request_id, std::sync::atomic::Ordering::Release);
        let _ = self.cmd_tx.send(GitCommand::BranchCompare {
            request_id,
            base,
            target,
        });
    }

    /// Cancel an in-flight revision comparison at the next file boundary.
    pub fn cancel_branch_compare(&self) {
        self.compare_generation
            .store(0, std::sync::atomic::Ordering::Release);
    }

    /// Apply a staged/working-tree mutation without blocking the UI.
    pub fn working_tree_operation(
        &self,
        request_id: u64,
        action: WorkingTreeAction,
        scope: WorkingTreeScope,
    ) {
        let _ = self.cmd_tx.send(GitCommand::WorkingTreeOperation {
            request_id,
            action,
            scope,
        });
    }

    /// 请求关闭工作线程
    pub fn close(&self) {
        self.cancel_branch_compare();
        let _ = self.cmd_tx.send(GitCommand::Close);
    }
}

/// 打开仓库并启动工作线程
///
/// 注意：路径校验在 UI 线程同步执行（毫秒级，可接受），失败即时返回错误；
/// 之后所有 git 命令都在后台线程跑，UI 线程只经通道通信。
pub fn spawn_open(
    repo_path: String,
    event_tx: Sender<GitEvent>,
) -> Result<GitHandle, GitError> {
    let path = Path::new(&repo_path);
    if !path.is_dir() {
        return Err(GitError::new("err-path-not-exist", repo_path));
    }
    // TODO: 子模块/worktree 的 .git 可能是文件而非目录，后续里程碑补判
    if !path.join(".git").exists() {
        return Err(GitError::new("err-not-a-repo", repo_path));
    }

    let (cmd_tx, cmd_rx) = mpsc::channel::<GitCommand>();
    let compare_generation =
        std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    thread::spawn({
        let compare_generation = compare_generation.clone();
        move || worker_loop(repo_path, cmd_rx, event_tx, compare_generation)
    });

    Ok(GitHandle {
        cmd_tx,
        compare_generation,
    })
}

/// 工作线程：命令处理（git 子进程阻塞执行）
fn worker_loop(
    repo_path: String,
    cmd_rx: Receiver<GitCommand>,
    event_tx: Sender<GitEvent>,
    compare_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
) {
    // 打开即刷新一次
    refresh_all(&repo_path, &event_tx);

    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(GitCommand::Refresh) => refresh_all(&repo_path, &event_tx),
            Ok(GitCommand::Run { label, args }) => {
                run_git(&repo_path, &label, &args, &event_tx);
            }
            Ok(GitCommand::CommitNumstat { oid }) => {
                run_numstat(&repo_path, &oid, &event_tx);
            }
            Ok(GitCommand::CommitMessage { oid }) => {
                run_commit_message(&repo_path, &oid, &event_tx);
            }
            Ok(GitCommand::CommitFileDiff {
                oid,
                merge_parent,
                file,
            }) => {
                run_file_diff(
                    &repo_path,
                    &oid,
                    merge_parent.as_deref(),
                    &file,
                    &event_tx,
                );
            }
            Ok(GitCommand::WorkingTreeFileDiff {
                request_id,
                kind,
                file,
            }) => {
                working_tree::run_file_diff(
                    &repo_path, request_id, kind, &file, &event_tx,
                );
            }
            Ok(GitCommand::BranchCompare {
                request_id,
                base,
                target,
            }) => {
                if compare_generation.load(std::sync::atomic::Ordering::Acquire)
                    == request_id
                {
                    branch_compare::spawn_comparison(
                        repo_path.clone(),
                        request_id,
                        base,
                        target,
                        event_tx.clone(),
                        compare_generation.clone(),
                    );
                }
            }
            Ok(GitCommand::WorkingTreeOperation {
                request_id,
                action,
                scope,
            }) => {
                let scope_kind = scope.kind();
                let result =
                    working_tree::apply_operation(&repo_path, action, &scope);
                let (success, detail) = match result {
                    Ok(()) => (true, String::new()),
                    Err(detail) => (false, detail),
                };
                // A mutation can partially complete, so always publish a
                // fresh status before reporting the operation result.
                refresh_status(&repo_path, &event_tx);
                log::info!(
                    "[git_worktree] operation finished: action={}, scope={scope_kind:?}, files={}, success={success}",
                    action.description(),
                    scope.files().len()
                );
                let _ = event_tx.send(GitEvent::WorkingTreeOperationFinished {
                    request_id,
                    action,
                    scope: scope_kind,
                    success,
                    detail,
                });
            }
            Ok(GitCommand::Close) | Err(RecvTimeoutError::Disconnected) => {
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// 仓库快照刷新：status + branch 合并为一个 Status 事件，log 独立事件
fn refresh_all(repo_path: &str, event_tx: &Sender<GitEvent>) {
    refresh_status(repo_path, event_tx);
    run_log(repo_path, event_tx);
    run_refs(repo_path, event_tx);
}

/// Refresh only the status snapshot after a local index/worktree operation.
fn refresh_status(repo_path: &str, event_tx: &Sender<GitEvent>) {
    let status = run_status(repo_path);
    let branches = run_branches(repo_path);
    match status {
        Ok((branch, upstream, files, ahead, behind)) => {
            let _ = event_tx.send(GitEvent::Status {
                branch,
                upstream,
                files,
                branches,
                ahead,
                behind,
            });
        }
        Err(error) => {
            log::warn!("[git_worktree] status refresh failed: {}", error.key);
            let _ = event_tx.send(GitEvent::StatusError(error));
        }
    }
}

/// 执行 git 命令（子进程阻塞，只在工作线程跑）
fn run_git(
    repo_path: &str,
    label: &str,
    args: &[String],
    event_tx: &Sender<GitEvent>,
) {
    let output = git_command().arg("-C").arg(repo_path).args(args).output();
    let event = match output {
        Ok(output) if output.status.success() => GitEvent::CommandDone {
            label: label.to_string(),
            success: true,
            message: String::from_utf8_lossy(&output.stdout).into_owned(),
        },
        Ok(output) => GitEvent::CommandDone {
            label: label.to_string(),
            success: false,
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(e) => GitEvent::Error(GitError::new("err-git-run", e.to_string())),
    };
    let _ = event_tx.send(event);
}

fn checkout_args(target: CheckoutTarget) -> Vec<String> {
    let mut args = vec!["checkout".to_string()];
    match target {
        CheckoutTarget::RemoteBranch(name) => {
            args.push("--track".to_string());
            args.push(name);
        }
        CheckoutTarget::LocalBranch(name)
        | CheckoutTarget::Tag(name)
        | CheckoutTarget::Commit(name) => args.push(name),
    }
    args
}

fn commit_args(message: String, amend: bool) -> Vec<String> {
    let mut args = vec!["commit".to_string()];
    if amend {
        args.push("--amend".to_string());
    }
    args.push("-m".to_string());
    args.push(message);
    args
}

fn commit_message_args(oid: String) -> Vec<String> {
    vec![
        "show".to_string(),
        "--no-patch".to_string(),
        "--format=%B".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--no-notes".to_string(),
        oid,
    ]
}

/// Query structured file metadata for a commit.
fn run_numstat(repo_path: &str, oid: &str, event_tx: &Sender<GitEvent>) {
    let merge_parent = match resolve_merge_parent(repo_path, oid) {
        Ok(parent) => parent,
        Err(detail) => {
            let _ = event_tx
                .send(GitEvent::Error(GitError::new("err-numstat", detail)));
            return;
        }
    };
    let (raw, stats) = if let Some(parent) = merge_parent.as_deref() {
        (
            git_command()
                .args(merge_raw_args(repo_path, parent, oid))
                .output(),
            git_command()
                .args(merge_numstat_args(repo_path, parent, oid))
                .output(),
        )
    } else {
        (
            git_command()
                .args([
                    "--no-pager",
                    "-c",
                    "core.quotePath=false",
                    "-C",
                    repo_path,
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "-r",
                    "-M",
                    "--raw",
                    "--no-color",
                    "--no-ext-diff",
                    "-z",
                    oid,
                ])
                .output(),
            git_command()
                .args([
                    "--no-pager",
                    "-c",
                    "core.quotePath=false",
                    "-C",
                    repo_path,
                    "show",
                    "--numstat",
                    "--format=",
                    "--no-color",
                    "--no-ext-diff",
                    "--find-renames",
                    oid,
                ])
                .output(),
        )
    };

    match (raw, stats) {
        (Ok(raw), Ok(stats))
            if raw.status.success() && stats.status.success() =>
        {
            let raw_files = parse_raw_records(&raw.stdout);
            let stat_files =
                parse_numstat(&String::from_utf8_lossy(&stats.stdout));
            let files = if raw_files.is_empty() {
                stat_files
            } else {
                merge_numstat(raw_files, stat_files)
            };
            log::debug!(
                "[git_diff] loaded commit file metadata: oid={}, files={}",
                oid,
                files.len()
            );
            let _ = event_tx.send(GitEvent::CommitFiles {
                oid: oid.to_string(),
                files,
                merge_parent,
            });
        }
        (Ok(raw), Ok(stats)) => {
            let detail = if !raw.status.success() {
                String::from_utf8_lossy(&raw.stderr).into_owned()
            } else {
                String::from_utf8_lossy(&stats.stderr).into_owned()
            };
            let _ = event_tx
                .send(GitEvent::Error(GitError::new("err-numstat", detail)));
        }
        (Err(error), _) | (_, Err(error)) => {
            let _ = event_tx.send(GitEvent::Error(GitError::new(
                "err-git-run",
                error.to_string(),
            )));
        }
    }
}

fn resolve_merge_parent(
    repo_path: &str,
    oid: &str,
) -> Result<Option<String>, String> {
    let output = git_command()
        .args(parent_query_args(repo_path, oid))
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let context: CommitDiffContext =
        parse_parent_line(&String::from_utf8_lossy(&output.stdout))?;
    Ok(context.merge_parent)
}

/// 查询选中提交的完整提交信息（详情面板正文 + co-author）
fn run_commit_message(repo_path: &str, oid: &str, event_tx: &Sender<GitEvent>) {
    let output = git_command()
        .args([
            "--no-pager",
            "-C",
            repo_path,
            "show",
            "-s",
            "--no-color",
            "--format=%B",
            oid,
        ])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let message =
                parse_commit_message(&String::from_utf8_lossy(&output.stdout));
            log::debug!(
                "[git_commit_message] loaded commit message: oid={}, body_lines={}, co_authors={}",
                oid,
                message.body.lines().count(),
                message.co_authors.len()
            );
            let _ = event_tx.send(GitEvent::CommitMessage {
                oid: oid.to_string(),
                message,
            });
        }
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let _ = event_tx.send(GitEvent::Error(GitError::new(
                "err-commit-message",
                detail,
            )));
        }
        Err(e) => {
            let _ = event_tx.send(GitEvent::Error(GitError::new(
                "err-git-run",
                e.to_string(),
            )));
        }
    }
}

/// 解析 `git show -s --format=%B` 的原始提交信息
///
/// 首行为 subject，其余为 body；`Co-authored-by:` trailer 行（大小写不敏感）
/// 从 body 剥离并收集进 `co_authors`，其余行原样保留。
pub fn parse_commit_message(text: &str) -> CommitMessage {
    let text = text.trim_matches(['\n', '\r']);
    let mut lines = text.split('\n');
    let subject = lines.next().unwrap_or("").trim_end().to_string();
    let mut body_lines = Vec::new();
    let mut co_authors = Vec::new();
    for line in lines {
        match parse_co_author_line(line) {
            Some(co_author) => co_authors.push(co_author),
            None => body_lines.push(line),
        }
    }
    CommitMessage {
        subject,
        body: body_lines.join("\n").trim().to_string(),
        co_authors,
    }
}

/// 识别一行 `Co-authored-by: Name <email>` trailer（大小写不敏感）
fn parse_co_author_line(line: &str) -> Option<CoAuthor> {
    let (token, value) = line.trim().split_once(':')?;
    if !token.trim().eq_ignore_ascii_case("co-authored-by") {
        return None;
    }
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    // "Name <email>"；缺 `<email>` 时整个值视作名字
    let (name, email) = match value.split_once('<') {
        Some((name, rest)) => (
            name.trim().to_string(),
            rest.trim_end_matches('>').trim().to_string(),
        ),
        None => (value.to_string(), String::new()),
    };
    Some(CoAuthor { name, email })
}

const MAX_BLOB_SIZE: usize = 10 * 1024 * 1024;

/// Query a single file patch and, when possible, its complete old/new blobs.
fn run_file_diff(
    repo_path: &str,
    oid: &str,
    merge_parent: Option<&str>,
    file: &FileChange,
    event_tx: &Sender<GitEvent>,
) {
    let args = if let Some(parent) = merge_parent {
        merge_patch_args(repo_path, parent, oid, file)
    } else {
        let path = if matches!(file.status, FileChangeStatus::Deleted) {
            file.old_path.as_deref().unwrap_or(&file.new_path)
        } else {
            &file.new_path
        };
        let mut args = vec![
            "--no-pager".to_string(),
            "-C".to_string(),
            repo_path.to_string(),
            "show".to_string(),
            "--format=".to_string(),
            "--no-color".to_string(),
            "--no-ext-diff".to_string(),
            "--find-renames".to_string(),
            oid.to_string(),
            "--".to_string(),
        ];
        if file.status.is_rename() {
            if let Some(old_path) = file.old_path.as_deref() {
                args.push(old_path.to_string());
            }
        }
        args.push(path.to_string());
        args
    };
    let output = git_command().args(&args).output();
    match output {
        Ok(output) if output.status.success() => {
            let patch = String::from_utf8_lossy(&output.stdout).into_owned();
            let (old_source, new_source) = if file.is_binary() {
                (None, None)
            } else {
                (
                    read_blob(repo_path, file.old_blob.as_deref()),
                    read_blob(repo_path, file.new_blob.as_deref()),
                )
            };
            log::debug!(
                "[git_diff] loaded file diff: oid={}, binary={}, patch_bytes={}",
                oid,
                file.is_binary(),
                patch.len()
            );
            let _ = event_tx.send(GitEvent::CommitFileDiff {
                oid: oid.to_string(),
                file: file.clone(),
                patch,
                old_source,
                new_source,
            });
        }
        Ok(output) => {
            let msg = String::from_utf8_lossy(&output.stderr);
            let _ = event_tx
                .send(GitEvent::Error(GitError::new("err-file-diff", msg)));
        }
        Err(e) => {
            let _ = event_tx.send(GitEvent::Error(GitError::new(
                "err-git-run",
                e.to_string(),
            )));
        }
    }
}

fn read_blob(repo_path: &str, oid: Option<&str>) -> Option<String> {
    read_blob_spec(repo_path, oid?)
}

fn read_blob_spec(repo_path: &str, spec: &str) -> Option<String> {
    let output = git_command()
        .args(["--no-pager", "-C", repo_path, "cat-file", "blob", spec])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > MAX_BLOB_SIZE {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Execute git status and return (branch, upstream, files, ahead, behind).
fn run_status(
    repo_path: &str,
) -> Result<(String, Option<String>, Vec<FileStatus>, usize, usize), GitError> {
    let output = git_command()
        .args([
            "-C",
            repo_path,
            "status",
            "--porcelain=v1",
            "-z",
            "-b",
            "--untracked-files=all",
        ])
        .output()
        .map_err(|error| GitError::new("err-git-status", error.to_string()))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError::new(
            "err-git-status",
            if detail.is_empty() {
                output
                    .status
                    .code()
                    .map(|code| format!("git status exited with status {code}"))
                    .unwrap_or_else(|| {
                        "git status terminated unexpectedly".to_string()
                    })
            } else {
                detail
            },
        ));
    }
    parse_status(&output.stdout)
}

/// Parse `git status --porcelain=v1 -z -b` output.
///
/// NUL separation is required here because Git can report paths containing
/// newlines or the traditional rename separator (` -> `).
///
/// Format:
/// ```text
/// ## main...origin/main [ahead 1, behind 2]
///  M src/foo.rs
/// ?? new.txt
/// R  new.txt\0old.txt
/// ```
fn parse_status(
    output: &[u8],
) -> Result<(String, Option<String>, Vec<FileStatus>, usize, usize), GitError> {
    let mut branch = String::new();
    let mut upstream = None;
    let mut ahead = 0;
    let mut behind = 0;
    let mut files = Vec::new();
    let mut records = output.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(rest) = record.strip_prefix(b"## ") {
            let rest = decode_status_text(rest)?;
            let (parsed_branch, parsed_upstream) = parse_branch_info(&rest);
            branch = parsed_branch;
            upstream = parsed_upstream;
            ahead = parse_count(&rest, "[ahead ");
            behind = parse_count(&rest, "behind ");
            continue;
        }

        if record.len() < 4 || record[2] != b' ' {
            return Err(GitError::new(
                "err-git-status",
                "Git returned an invalid porcelain status record",
            ));
        }
        let index = record[0] as char;
        let worktree = record[1] as char;
        let path = decode_status_path(&record[3..])?;
        let old_path = if index == 'R'
            || index == 'C'
            || worktree == 'R'
            || worktree == 'C'
        {
            let old_path = records.next().ok_or_else(|| {
                GitError::new(
                    "err-git-status",
                    "Git returned an incomplete rename status record",
                )
            })?;
            Some(decode_status_path(old_path)?)
        } else {
            None
        };
        files.push(FileStatus {
            index,
            worktree,
            path,
            old_path,
        });
    }
    Ok((branch, upstream, files, ahead, behind))
}

fn decode_status_text(bytes: &[u8]) -> Result<String, GitError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        GitError::new(
            "err-git-status",
            "Git returned non-UTF-8 branch or status metadata",
        )
    })
}

fn decode_status_path(bytes: &[u8]) -> Result<String, GitError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        GitError::new(
            "err-git-status-path",
            "Git returned a non-UTF-8 path that cannot be handled safely",
        )
    })
}

fn parse_branch_info(rest: &str) -> (String, Option<String>) {
    if let Some(branch) = rest.strip_prefix("No commits yet on ") {
        (branch.to_string(), None)
    } else if let Some(branch) = rest.strip_prefix("Initial commit on ") {
        (branch.to_string(), None)
    } else {
        let Some((branch, tracking)) = rest.split_once("...") else {
            return (rest.to_string(), None);
        };
        let upstream = tracking
            .split_once(" [")
            .map(|(name, _)| name)
            .unwrap_or(tracking)
            .trim();
        (
            branch.to_string(),
            (!upstream.is_empty()).then(|| upstream.to_string()),
        )
    }
}

fn parse_count(text: &str, marker: &str) -> usize {
    let Some(index) = text.find(marker) else {
        return 0;
    };
    text[index + marker.len()..]
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .unwrap_or("")
        .parse()
        .unwrap_or(0)
}

/// 执行 git branch，返回本地分支列表
fn run_branches(repo_path: &str) -> Vec<BranchInfo> {
    let Ok(output) = git_command()
        .args([
            "-C",
            repo_path,
            "branch",
            "--format=%(HEAD)%09%(refname:short)",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut branches = Vec::new();
    for line in text.lines() {
        let Some((head, name)) = line.split_once('\t') else {
            continue;
        };
        branches.push(BranchInfo {
            name: name.to_string(),
            is_head: head == "*",
        });
    }
    branches
}

/// 收集侧栏引用清单（四个只读快命令，任一失败按空处理不影响其余分区）
fn run_refs(repo_path: &str, event_tx: &Sender<GitEvent>) {
    let out = |args: &[&str]| -> String {
        let mut cmd = git_command();
        cmd.arg("-C").arg(repo_path);
        for arg in args {
            cmd.arg(arg);
        }
        match cmd.output() {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).into_owned()
            }
            _ => String::new(),
        }
    };
    let comparison_revisions = git_command()
        .args(branch_compare::comparison_ref_args(repo_path))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            branch_compare::parse_comparison_refs(&String::from_utf8_lossy(
                &output.stdout,
            ))
        })
        .unwrap_or_default();
    let refs = RefsInfo {
        remotes: non_empty_lines(&out(&["remote"])),
        remote_branches: non_empty_lines(&out(&[
            "branch",
            "-r",
            "--format=%(refname:short)",
        ])),
        tags: non_empty_lines(&out(&["tag", "--sort=-creatordate"])),
        stashes: parse_stashes(&out(&["stash", "list"])),
        comparison_revisions,
    };
    let _ = event_tx.send(GitEvent::Refs(refs));
}

/// 非空行列表（trim + 去空行）
fn non_empty_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// 解析 `git stash list`："stash@{0}: WIP on main: abc x" → "WIP on main: abc x"
/// （无 ": " 的畸形行跳过）
fn parse_stashes(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            l.split_once(": ").map(|(_, desc)| desc.trim().to_string())
        })
        .collect()
}

/// Execute git log in the topological order used by the active-lane layout.
fn run_log(repo_path: &str, event_tx: &Sender<GitEvent>) {
    let output = git_command().args(log_args(repo_path)).output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let rows = parse_log(&text);
            let _ = event_tx.send(GitEvent::Log { rows });
        }
        Ok(output) => {
            let msg = String::from_utf8_lossy(&output.stderr);
            let _ = event_tx
                .send(GitEvent::Error(GitError::new("err-git-log", msg)));
        }
        Err(e) => {
            let _ = event_tx.send(GitEvent::Error(GitError::new(
                "err-git-run",
                e.to_string(),
            )));
        }
    }
}

fn log_args(repo_path: &str) -> Vec<String> {
    vec![
        "-C".into(),
        repo_path.into(),
        "log".into(),
        "--all".into(),
        "--topo-order".into(),
        "--max-count=200".into(),
        "--date=format:%Y-%m-%d %H:%M".into(),
        "--pretty=format:%H%x00%h%x00%an%x00%ai%x00%at%x00%s%x00%D%x00%P"
            .into(),
    ]
}

/// Parse structured `git log --pretty=format:...` output.
///
/// Each line starts with a 40-character hexadecimal object id followed by
/// NUL-separated commit fields. Malformed records are ignored defensively.
fn parse_log(text: &str) -> Vec<LogRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\0').collect();
        let Some(oid) = fields.first().copied() else {
            continue;
        };
        if oid.len() != 40 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if fields.len() < 6 {
            continue;
        }
        let parents = fields
            .get(7)
            .copied()
            .unwrap_or("")
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        // `%at` is the author timestamp used for relative-time display.
        let timestamp = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        rows.push(LogRow {
            oid: oid.to_string(),
            short: fields[1].to_string(),
            author: fields[2].to_string(),
            date: fields[3].to_string(),
            timestamp,
            subject: fields[5].to_string(),
            decorations: fields.get(6).copied().unwrap_or("").to_string(),
            parents,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_args_preserve_target_as_one_argument() {
        assert_eq!(
            checkout_args(CheckoutTarget::LocalBranch(
                "feature/ui polish".into()
            )),
            vec!["checkout", "feature/ui polish"]
        );
        assert_eq!(
            checkout_args(CheckoutTarget::RemoteBranch(
                "origin/功能/导航".into()
            )),
            vec!["checkout", "--track", "origin/功能/导航"]
        );
        assert_eq!(
            checkout_args(CheckoutTarget::Tag("release/v1.2.3".into())),
            vec!["checkout", "release/v1.2.3"]
        );
        assert_eq!(
            checkout_args(CheckoutTarget::Commit(
                "0123456789abcdef0123456789abcdef01234567".into()
            )),
            vec!["checkout", "0123456789abcdef0123456789abcdef01234567"]
        );
    }

    #[test]
    fn commit_args_support_amend_without_splitting_message() {
        let message = "subject\n\nbody with spaces".to_string();
        assert_eq!(
            commit_args(message.clone(), false),
            vec!["commit".to_string(), "-m".to_string(), message.clone()]
        );
        assert_eq!(
            commit_args(message.clone(), true),
            vec![
                "commit".to_string(),
                "--amend".to_string(),
                "-m".to_string(),
                message,
            ]
        );
    }

    #[test]
    fn commit_message_args_preserve_oid_as_one_argument() {
        let oid = "0123456789abcdef0123456789abcdef01234567".to_string();
        assert_eq!(
            commit_message_args(oid.clone()),
            vec![
                "show".to_string(),
                "--no-patch".to_string(),
                "--format=%B".to_string(),
                "--no-color".to_string(),
                "--no-ext-diff".to_string(),
                "--no-notes".to_string(),
                oid,
            ]
        );
    }

    #[test]
    fn working_tree_diff_args_separate_staged_and_unstaged_comparisons() {
        let file = FileStatus {
            index: 'R',
            worktree: 'M',
            path: "new.rs".into(),
            old_path: Some("old.rs".into()),
        };
        let staged = working_tree::working_tree_diff_args(
            "repo",
            WorkingTreeDiffKind::Staged,
            &file,
        );
        let unstaged = working_tree::working_tree_diff_args(
            "repo",
            WorkingTreeDiffKind::Unstaged,
            &file,
        );
        let separator = staged
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or_default();
        assert_eq!(
            &staged[separator + 1..],
            ["old.rs".to_string(), "new.rs".to_string()]
        );
        assert!(staged.iter().any(|argument| argument == "--cached"));
        assert!(!unstaged.iter().any(|argument| argument == "--cached"));
        let unstaged_separator = unstaged
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or_default();
        assert_eq!(
            &unstaged[unstaged_separator + 1..],
            ["old.rs".to_string(), "new.rs".to_string()]
        );
    }

    #[test]
    fn untracked_diff_args_compare_against_a_portable_null_path() {
        let args = working_tree::untracked_diff_args("repo", "new file.txt");
        assert!(
            args.windows(2)
                .any(|pair| { pair[0] == "--" && pair[1] == "/dev/null" })
        );
        assert_eq!(args.last().map(String::as_str), Some("new file.txt"));
    }

    #[test]
    fn parse_numstat_normal_binary_rename() {
        let text = "12\t3\tsrc/main.rs\n-\t-\tassets/logo.png\n5\t0\told.rs => new.rs\n";
        let files = parse_numstat(text);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].added, Some(12));
        assert_eq!(files[0].deleted, Some(3));
        // 二进制：增删均为 None
        assert!(files[1].is_binary());
        // 重命名：路径原样保留 "old => new"
        assert_eq!(files[2].path, "old.rs => new.rs");
        assert_eq!(files[2].added, Some(5));
        // 空/畸形行跳过
        assert!(parse_numstat("").is_empty());
        assert!(parse_numstat("garbage\n12\t3\n").is_empty());
    }

    #[test]
    fn stat_blocks_ratios() {
        assert_eq!(stat_blocks(0, 0), (0, 0));
        assert_eq!(stat_blocks(10, 0), (5, 0));
        assert_eq!(stat_blocks(0, 10), (0, 5));
        // ⌈1×5/2⌉=3 绿 2 红
        assert_eq!(stat_blocks(1, 1), (3, 2));
        assert_eq!(stat_blocks(3, 1), (4, 1));
        assert_eq!(stat_blocks(6, 4), (3, 2));
    }

    #[test]
    fn parse_status_normal() {
        let output = b"## main...origin/main [ahead 1, behind 2]\0 M src/a.rs\0A  new.rs\0?? untracked.txt\0";
        let (branch, upstream, files, ahead, behind) =
            parse_status(output).unwrap();
        assert_eq!(branch, "main");
        assert_eq!(upstream.as_deref(), Some("origin/main"));
        assert_eq!(ahead, 1);
        assert_eq!(behind, 2);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].index, ' ');
        assert_eq!(files[0].worktree, 'M');
        assert_eq!(files[0].path, "src/a.rs");
        assert!(files[0].is_staged() == false);
        assert!(files[1].is_staged());
        assert!(!files[2].is_staged());
        assert!(files[2].is_untracked());
        assert_eq!(files[2].code(), '?');
    }

    #[test]
    fn file_status_separates_staged_worktree_and_mixed_changes() {
        let output =
            b"## main\0M  staged.rs\0 M changed.rs\0MM mixed.rs\0?? new.txt\0";
        let (_, _, files, _, _) = parse_status(output).unwrap();
        assert!(files[0].has_staged_changes());
        assert!(!files[0].has_worktree_changes());
        assert!(!files[1].has_staged_changes());
        assert!(files[1].has_worktree_changes());
        assert!(files[2].has_staged_changes());
        assert!(files[2].has_worktree_changes());
        assert!(!files[3].has_staged_changes());
        assert!(files[3].has_worktree_changes());
        assert_eq!(files[2].code_for(true), 'M');
        assert_eq!(files[2].code_for(false), 'M');
    }

    #[test]
    fn parse_status_no_upstream() {
        let output = b"## main\0 M a.rs\0";
        let (branch, upstream, _files, ahead, behind) =
            parse_status(output).unwrap();
        assert_eq!(branch, "main");
        assert!(upstream.is_none());
        assert_eq!(ahead, 0);
        assert_eq!(behind, 0);
    }

    #[test]
    fn parse_status_rename_and_detached() {
        let output = b"## HEAD (no branch)\0R  new.txt\0old.txt\0";
        let (branch, upstream, files, _, _) = parse_status(output).unwrap();
        assert_eq!(branch, "HEAD (no branch)");
        assert!(upstream.is_none());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].index, 'R');
        assert_eq!(files[0].path, "new.txt");
        assert_eq!(files[0].old_path.as_deref(), Some("old.txt"));
    }

    #[test]
    fn parse_status_tracks_worktree_rename_and_copy_paths() {
        let output =
            b"## main\0 R renamed.txt\0old.txt\0 C copied.txt\0source.txt\0";
        let (_, _, files, _, _) = parse_status(output).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].worktree, 'R');
        assert_eq!(files[0].path, "renamed.txt");
        assert_eq!(files[0].old_path.as_deref(), Some("old.txt"));
        assert_eq!(files[1].worktree, 'C');
        assert_eq!(files[1].path, "copied.txt");
        assert_eq!(files[1].old_path.as_deref(), Some("source.txt"));
    }

    #[test]
    fn parse_status_z_preserves_special_paths() {
        let output = b"## main\0 M path -> literal\nname.rs\0?? unicode-\xE4\xB8\xAD.txt\0";
        let (_, _, files, _, _) = parse_status(output).unwrap();
        assert_eq!(files[0].path, "path -> literal\nname.rs");
        assert_eq!(files[1].path, "unicode-中.txt");
    }

    #[test]
    fn parse_status_rejects_non_utf8_paths() {
        let error = parse_status(b"## main\0 M invalid-\xFF.txt\0")
            .expect_err("non-UTF-8 paths must not reach Git operations");
        assert_eq!(error.key, "err-git-status-path");
    }

    #[test]
    fn conflicted_status_variants_are_not_staged() {
        for (index, worktree) in [
            ('U', 'U'),
            ('U', ' '),
            (' ', 'U'),
            ('D', 'D'),
            ('A', 'A'),
            ('A', 'U'),
            ('U', 'A'),
        ] {
            let file = FileStatus {
                index,
                worktree,
                path: "conflict.rs".to_string(),
                old_path: None,
            };
            assert!(file.is_conflicted(), "{index}{worktree}");
            assert!(!file.has_staged_changes(), "{index}{worktree}");
        }
    }

    #[test]
    fn parse_log_plain() {
        let text = "0123456789abcdef0123456789abcdef01234567\0short\0Lionel Fung\02026-08-13 20:00\01756123456\0M0 框架\0HEAD -> main\0\n";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].oid, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(rows[0].subject, "M0 框架");
        assert_eq!(rows[0].decorations, "HEAD -> main");
        assert_eq!(rows[0].timestamp, 17_561_234_56);
        assert!(rows[0].parents.is_empty());
    }

    #[test]
    fn parse_log_with_parents() {
        let text = "0123456789abcdef0123456789abcdef01234567\0s\0a\0d\01756123456\0merge 分支\0HEAD -> main\089abcdef0123456789abcdef0123456789abcdef ffff0123456789abcdef0123456789abcdef01\n";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].oid, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(rows[0].subject, "merge 分支");
        assert_eq!(rows[0].parents.len(), 2);
        assert_eq!(
            rows[0].parents[0],
            "89abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn parse_log_skips_bad_lines() {
        let text = "garbage\n0123456789abcdef0123456789abcdef01234567\0s\0a\0d\0提交\0\n";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn log_args_use_topological_order_without_ascii_graph() {
        let args = log_args("repo with spaces");

        assert!(args.windows(2).any(|pair| {
            pair == ["-C".to_string(), "repo with spaces".to_string()]
        }));
        assert!(args.iter().any(|arg| arg == "--all"));
        assert!(args.iter().any(|arg| arg == "--topo-order"));
        assert!(!args.iter().any(|arg| arg == "--graph"));
    }

    #[test]
    fn parse_stashes_strips_prefix() {
        let text = "stash@{0}: WIP on main: ab1234 x\nstash@{1}: On dev: fix\n";
        assert_eq!(
            parse_stashes(text),
            vec!["WIP on main: ab1234 x", "On dev: fix"]
        );
        // 畸形行跳过
        assert!(parse_stashes("").is_empty());
        assert!(parse_stashes("no-colon-line\n").is_empty());
    }

    #[test]
    fn non_empty_lines_trims_and_skips_blank() {
        assert_eq!(
            non_empty_lines("origin\n\n upstream \n"),
            vec!["origin", "upstream"]
        );
        assert!(non_empty_lines("\n \n").is_empty());
    }

    #[test]
    fn parse_diff_numbering() {
        let text = "diff --git a/f.rs b/f.rs\nindex 111..222 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -2,4 +2,5 @@ fn main()\n ctx\n-del old\n+add new\n more\n";
        let lines = parse_diff(text);
        // 头 4 行 Meta + hunk + 4 行体
        assert_eq!(lines.len(), 9);
        assert_eq!(lines[0].kind, DiffLineKind::Meta);
        assert!(lines[1].kind == DiffLineKind::Meta);
        // ---/+++ 不计入删除/新增（无行号）
        assert_eq!(lines[2].kind, DiffLineKind::Meta);
        assert_eq!(lines[3].kind, DiffLineKind::Meta);
        assert_eq!(lines[4].kind, DiffLineKind::Hunk);
        assert_eq!(lines[4].old_no, None);
        assert_eq!(
            lines[5],
            DiffLine {
                kind: DiffLineKind::Context,
                old_no: Some(2),
                new_no: Some(2),
                text: " ctx".into()
            }
        );
        assert_eq!(
            lines[6],
            DiffLine {
                kind: DiffLineKind::Del,
                old_no: Some(3),
                new_no: None,
                text: "-del old".into()
            }
        );
        assert_eq!(
            lines[7],
            DiffLine {
                kind: DiffLineKind::Add,
                old_no: None,
                new_no: Some(3),
                text: "+add new".into()
            }
        );
        // 删一行加一行后上下文行号对齐：旧 4 新 4
        assert_eq!(lines[8].kind, DiffLineKind::Context);
        assert_eq!(lines[8].old_no, Some(4));
        assert_eq!(lines[8].new_no, Some(4));
    }

    #[test]
    fn parse_diff_missing_counts_and_empty_context() {
        // 省略计数形式 "@@ -1 +1 @@"；空上下文行（前导空格被剥成空串）
        let text = "@@ -1 +1 @@\n-a\n+\n\n";
        let lines = parse_diff(text);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].kind, DiffLineKind::Hunk);
        assert_eq!(lines[1].old_no, Some(1));
        assert_eq!(lines[1].new_no, None);
        assert_eq!(lines[2].old_no, None);
        assert_eq!(lines[2].new_no, Some(1));
        assert_eq!(lines[3].kind, DiffLineKind::Context);
        assert_eq!(lines[3].old_no, Some(2));
        assert_eq!(lines[3].new_no, Some(2));
    }

    #[test]
    fn parse_diff_bad_hunk_header_is_meta() {
        let lines = parse_diff("@@ garbage @@\nplain\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, DiffLineKind::Meta);
        assert_eq!(lines[1].kind, DiffLineKind::Meta);
        assert_eq!(lines[1].old_no, None);
    }

    #[test]
    fn parse_commit_message_splits_subject_body_co_authors() {
        let text = "feat(ui): add detail view\n\nLonger description\nspanning lines.\n\nCo-authored-by: Alice <alice@example.com>\nco-authored-by: Bob\nSigned-off-by: Carol <carl@example.com>\n";
        let message = parse_commit_message(text);
        assert_eq!(message.subject, "feat(ui): add detail view");
        assert_eq!(
            message.body,
            "Longer description\nspanning lines.\n\nSigned-off-by: Carol <carl@example.com>"
        );
        assert_eq!(
            message.co_authors,
            vec![
                CoAuthor {
                    name: "Alice".into(),
                    email: "alice@example.com".into()
                },
                CoAuthor {
                    name: "Bob".into(),
                    email: String::new()
                },
            ]
        );
        // 非 co-author trailer（如 Signed-off-by）保留在 body
        assert!(message.body.contains("Signed-off-by"));
        assert_eq!(
            message.co_authors[0].display(),
            "Alice <alice@example.com>"
        );
        assert_eq!(message.co_authors[1].display(), "Bob");
    }

    #[test]
    fn parse_commit_message_subject_only_trims_trailing_newlines() {
        let message = parse_commit_message("just subject\n\n\n");
        assert_eq!(message.subject, "just subject");
        assert!(message.body.is_empty());
        assert!(message.co_authors.is_empty());
    }

    #[test]
    fn parse_commit_message_empty_and_blank_co_author_values() {
        assert_eq!(parse_commit_message(""), CommitMessage::default());
        // 畸形（空值）trailer 不计入 co_authors，行原样保留在 body
        let message = parse_commit_message("s\n\nCo-authored-by:\nreal note\n");
        assert!(message.co_authors.is_empty());
        assert!(message.body.contains("Co-authored-by:"));
        assert!(message.body.contains("real note"));
    }
}
