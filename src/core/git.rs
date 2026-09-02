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

pub mod agent_operation;
pub mod automation;
mod branch_compare;
mod commit_log;
mod working_tree;

pub use commit_log::LogScope;

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

impl CompareRevision {
    /// Construct a commit revision from a user-provided hexadecimal object id.
    ///
    /// The comparison worker resolves the value against the repository object
    /// database later. This constructor only validates the format accepted by
    /// the revision picker (7 to 64 hexadecimal characters).
    pub fn from_commit_id(value: impl AsRef<str>) -> Option<Self> {
        let value = value.as_ref().trim();
        Self::is_supported_commit_id(value).then(|| Self {
            name: value.to_string(),
            full_name: value.to_string(),
            kind: CompareRevisionKind::Commit,
        })
    }

    /// Return whether a value has the supported raw commit-id format.
    pub fn is_supported_commit_id(value: &str) -> bool {
        matches!(value.len(), 7..=64)
            && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
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

/// Repository refs snapshot used by the sidebar and comparison picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashInfo {
    /// Stable Git selector, such as `stash@{0}`.
    pub reference: String,
    /// User-visible stash description without the selector prefix.
    pub description: String,
}

/// Repository refs snapshot used by the sidebar and comparison picker.
#[derive(Clone, Debug, Default)]
pub struct RefsInfo {
    /// 远程名清单（git remote）
    pub remotes: Vec<String>,
    /// Remote-tracking branch short names (`origin/main` etc.); symbolic
    /// HEAD aliases are not included.
    pub remote_branches: Vec<String>,
    /// 标签名（按创建时间倒序）
    pub tags: Vec<String>,
    /// Stash entries with both their Git selectors and display descriptions.
    pub stashes: Vec<StashInfo>,
    /// Local/remote branches and tags available to revision comparison.
    pub comparison_revisions: Vec<CompareRevision>,
}

/// 后台 → UI 事件
pub enum GitEvent {
    /// Repository status, tracked upstream, changed files, and branch list.
    Status {
        branch: String,
        /// Current commit id, or `None` for an unborn branch.
        head: Option<String>,
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
    /// One page of commit log rows with parents for active-lane layout.
    /// `replace` restarts the graph list; otherwise the page appends.
    LogPage {
        rows: Vec<LogRow>,
        replace: bool,
        has_more: bool,
    },
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
    /// Set the commit-graph history scope and reload the first log page.
    LogQuery { scope: LogScope },
    /// Fetch the next commit-graph page for the current log scope.
    MoreLogPage,
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

    /// Set the commit-graph history scope and reload the first page.
    pub fn log_query(&self, scope: LogScope) {
        let _ = self.cmd_tx.send(GitCommand::LogQuery { scope });
    }

    /// Request the next commit-graph page for the current scope.
    pub fn more_log_page(&self) {
        let _ = self.cmd_tx.send(GitCommand::MoreLogPage);
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
    let mut log_state = commit_log::LogState::default();
    refresh_all(&repo_path, &event_tx, &mut log_state);

    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(GitCommand::Refresh) => {
                refresh_all(&repo_path, &event_tx, &mut log_state)
            }
            Ok(GitCommand::LogQuery { scope }) => {
                commit_log::set_scope(
                    &repo_path,
                    &mut log_state,
                    scope,
                    &event_tx,
                );
            }
            Ok(GitCommand::MoreLogPage) => {
                commit_log::request_more(&repo_path, &mut log_state, &event_tx);
            }
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
fn refresh_all(
    repo_path: &str,
    event_tx: &Sender<GitEvent>,
    log_state: &mut commit_log::LogState,
) {
    refresh_status(repo_path, event_tx);
    commit_log::run_page(repo_path, log_state, true, event_tx);
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
                head: read_head(repo_path),
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

fn read_head(repo_path: &str) -> Option<String> {
    let output = git_command()
        .args(["-C", repo_path, "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!head.is_empty()).then_some(head)
}

/// Execute a git command (blocking subprocess; worker thread only).
///
/// Logs every invocation at the worker boundary: successes at info level
/// (visible with `RUST_LOG=info`), failures at warn level with the full
/// arguments, exit status, and git output so `debug.log` keeps an actionable
/// trail even under the default filter.
fn run_git(
    repo_path: &str,
    label: &str,
    args: &[String],
    event_tx: &Sender<GitEvent>,
) {
    let output = git_command().arg("-C").arg(repo_path).args(args).output();
    match output {
        Ok(output) if output.status.success() => {
            log::info!(
                "[git_command] command ok: label={label}, args={args:?}, {}",
                truncated(&String::from_utf8_lossy(&output.stdout))
            );
            let _ = event_tx.send(GitEvent::CommandDone {
                label: label.to_string(),
                success: true,
                message: String::from_utf8_lossy(&output.stdout).into_owned(),
            });
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let message =
                match (stderr.trim().is_empty(), stdout.trim().is_empty()) {
                    (false, false) => format!("{stderr}\n{stdout}"),
                    (false, true) => stderr.clone(),
                    (true, false) => stdout.clone(),
                    (true, true) => {
                        format!("git exited with {:?}", output.status.code())
                    }
                };
            log::warn!(
                "[git_command] command failed: label={label}, args={args:?}, \
                 exit={:?}, stderr={}, stdout={}",
                output.status.code(),
                truncated(&stderr),
                truncated(&stdout)
            );
            let _ = event_tx.send(GitEvent::CommandDone {
                label: label.to_string(),
                success: false,
                message,
            });
        }
        Err(e) => {
            log::warn!(
                "[git_command] command spawn failed: label={label}, \
                 args={args:?}, error={e}"
            );
            let _ = event_tx.send(GitEvent::Error(GitError::new(
                "err-git-run",
                e.to_string(),
            )));
        }
    }
}

/// Bound logged git output so a chatty command cannot flood debug.log.
fn truncated(text: &str) -> String {
    const LIMIT: usize = 2000;
    let text = text.trim();
    if text.chars().count() <= LIMIT {
        format!("output={text:?}")
    } else {
        let head: String = text.chars().take(LIMIT).collect();
        format!("output={head:?}…(truncated)")
    }
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
        remote_branches: parse_remote_branches(&out(&[
            "for-each-ref",
            "refs/remotes",
            "--format=%(refname:short)%09%(symref)",
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

/// Remote-tracking branch short names from `for-each-ref refs/remotes`,
/// dropping symbolic refs such as the `origin/HEAD` alias (whose short name
/// newer Git resolves to the bare remote name).
fn parse_remote_branches(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (name, symref) = line.split_once('\t')?;
            let name = name.trim();
            (!name.is_empty() && symref.trim().is_empty())
                .then(|| name.to_string())
        })
        .collect()
}

/// Parse `git stash list` while retaining the selector needed by mutations.
fn parse_stashes(text: &str) -> Vec<StashInfo> {
    text.lines()
        .filter_map(|l| {
            let (reference, description) = l.split_once(": ")?;
            let reference = reference.trim();
            is_stash_reference(reference).then(|| StashInfo {
                reference: reference.to_string(),
                description: description.trim().to_string(),
            })
        })
        .collect()
}

/// Accept only the numeric selectors emitted by `git stash list`.
fn is_stash_reference(value: &str) -> bool {
    let Some(index) = value
        .strip_prefix("stash@{")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return false;
    };
    !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_keeps_short_output_and_bounds_long_output() {
        assert_eq!(truncated("  hello\n"), "output=\"hello\"");
        let long = "x".repeat(3000);
        let logged = truncated(&long);
        assert!(logged.starts_with("output=\""));
        assert!(logged.ends_with("(truncated)"));
        assert!(logged.len() < long.len());
    }

    #[test]
    fn commit_revision_constructor_trims_and_validates_hex_ids() {
        let revision =
            CompareRevision::from_commit_id(format!("  {}  ", "a".repeat(40)))
                .expect("valid commit id");
        assert_eq!(revision.name, "a".repeat(40));
        assert_eq!(revision.full_name, revision.name);
        assert_eq!(revision.kind, CompareRevisionKind::Commit);
        assert!(CompareRevision::from_commit_id("abcdef").is_none());
        assert!(CompareRevision::from_commit_id("not-a-sha").is_none());
        assert!(CompareRevision::from_commit_id("a".repeat(65)).is_none());
    }

    #[test]
    fn parse_remote_branches_skips_symref_head_alias() {
        let text = "origin\trefs/remotes/origin/master\n\
                    origin/build\t\n\
                    origin/master\t\n\
                    \t\n";
        assert_eq!(
            parse_remote_branches(text),
            vec!["origin/build".to_string(), "origin/master".to_string()]
        );
    }

    #[test]
    fn parse_remote_branches_round_trips_real_clone_refs() {
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join(format!("augur-git-refs-{}-{id}", std::process::id()));
        let source = root.join("source");
        let clone = root.join("clone");
        fs::create_dir_all(&source).expect("test directory");

        let git = |args: &[&str]| {
            let output = git_command()
                .args(args)
                .output()
                .expect("git must be available");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        git(&["init", "-q", source.to_str().unwrap()]);
        git(&[
            "-C",
            source.to_str().unwrap(),
            "config",
            "user.email",
            "test@example.com",
        ]);
        git(&[
            "-C",
            source.to_str().unwrap(),
            "config",
            "user.name",
            "Test User",
        ]);
        git(&[
            "-C",
            source.to_str().unwrap(),
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ]);
        git(&[
            "clone",
            "-q",
            source.to_str().unwrap(),
            clone.to_str().unwrap(),
        ]);
        let output = git(&[
            "-C",
            clone.to_str().unwrap(),
            "for-each-ref",
            "refs/remotes",
            "--format=%(refname:short)%09%(symref)",
        ]);
        let parsed = parse_remote_branches(&output);
        assert!(!parsed.is_empty(), "clone should have remote refs");
        for name in &parsed {
            assert!(
                name.starts_with("origin/"),
                "unexpected bare entry {name:?}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

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
    fn parse_stashes_preserves_references_and_descriptions() {
        let text = "stash@{0}: WIP on main: ab1234 x\nstash@{1}: On dev: fix\n";
        assert_eq!(
            parse_stashes(text),
            vec![
                StashInfo {
                    reference: "stash@{0}".to_string(),
                    description: "WIP on main: ab1234 x".to_string(),
                },
                StashInfo {
                    reference: "stash@{1}".to_string(),
                    description: "On dev: fix".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_stashes_skips_malformed_references() {
        let text = "stash@{x}: invalid\nnot-a-stash: invalid\nstash@{2} missing delimiter\n";
        assert!(parse_stashes(text).is_empty());
    }

    #[test]
    fn parse_stashes_keeps_colons_in_descriptions() {
        let stashes = parse_stashes("stash@{3}: message: with: colons\n");
        assert_eq!(stashes[0].description, "message: with: colons");
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
