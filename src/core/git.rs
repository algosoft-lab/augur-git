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

#[allow(unused_imports)]
pub use crate::core::diff::{
    DiffLine, DiffLineKind, FileChange, FileChangeStatus, parse_diff,
    stat_blocks,
};
use crate::core::diff::{merge_numstat, parse_numstat, parse_raw_records};
use crate::core::graph::LogRow;

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
}

/// 后台 → UI 事件
pub enum GitEvent {
    /// 状态结果（分支 + 变更文件 + 本地分支列表 + ahead/behind）
    Status {
        branch: String,
        files: Vec<FileStatus>,
        /// 本地分支列表（(名字, 是否当前分支)）
        branches: Vec<BranchInfo>,
        /// 领先上游提交数
        ahead: usize,
        /// 落后上游提交数
        behind: usize,
    },
    /// 提交日志（含 parents，供 compute_graph 布局）
    Log { rows: Vec<LogRow> },
    /// 引用快照（侧栏 remotes/远程分支/标签/stash 分区）
    Refs(RefsInfo),
    /// 提交的逐文件增删统计（git show --numstat，选中提交时查询）
    CommitFiles { oid: String, files: Vec<FileChange> },
    /// Structured single-file commit diff payload.
    CommitFileDiff {
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
    /// 命令执行出错（key 为 i18n 键，展示侧本地化）
    Error(GitError),
}

/// 单个文件变更（git status --porcelain 解析）
#[derive(Clone, Debug)]
pub struct FileStatus {
    /// 索引状态字符（M/A/D/R/C/U，空格 = 无）
    pub index: char,
    /// 工作区状态字符
    pub worktree: char,
    /// 文件路径
    pub path: String,
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

    /// 是否已暂存（索引区有变更）
    pub fn is_staged(&self) -> bool {
        self.index != ' '
    }
}

/// 本地分支信息
#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    /// 是否当前 HEAD 分支
    pub is_head: bool,
}

/// UI → 后台指令
pub enum GitCommand {
    /// 刷新仓库快照（status → branch → log 顺序执行，各发事件）
    Refresh,
    /// 执行任意 git 命令（label 供 UI 显示；args 不含 "git" 本身）
    Run { label: String, args: Vec<String> },
    /// 查询提交的逐文件增删统计（git show --numstat）
    CommitNumstat { oid: String },
    /// Query a structured single-file commit diff.
    CommitFileDiff { oid: String, file: FileChange },
    /// 关闭工作线程
    Close,
}

/// 工作线程句柄（UI 侧持有）
pub struct GitHandle {
    cmd_tx: Sender<GitCommand>,
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

    /// 查询提交的逐文件增删统计
    pub fn commit_numstat(&self, oid: String) {
        let _ = self.cmd_tx.send(GitCommand::CommitNumstat { oid });
    }

    /// Query a structured single-file commit diff.
    pub fn commit_file_diff(&self, oid: String, file: FileChange) {
        let _ = self.cmd_tx.send(GitCommand::CommitFileDiff { oid, file });
    }

    /// 请求关闭工作线程
    pub fn close(&self) {
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
    thread::spawn(move || worker_loop(repo_path, cmd_rx, event_tx));

    Ok(GitHandle { cmd_tx })
}

/// 工作线程：命令处理（git 子进程阻塞执行）
fn worker_loop(
    repo_path: String,
    cmd_rx: Receiver<GitCommand>,
    event_tx: Sender<GitEvent>,
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
            Ok(GitCommand::CommitFileDiff { oid, file }) => {
                run_file_diff(&repo_path, &oid, &file, &event_tx);
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
    let status = run_status(repo_path);
    let branches = run_branches(repo_path);
    if let Some((branch, files, ahead, behind)) = status {
        let _ = event_tx.send(GitEvent::Status {
            branch,
            files,
            branches,
            ahead,
            behind,
        });
    }
    run_log(repo_path, event_tx);
    run_refs(repo_path, event_tx);
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

/// Query structured file metadata for a commit.
fn run_numstat(repo_path: &str, oid: &str, event_tx: &Sender<GitEvent>) {
    let raw = git_command()
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
        .output();
    let stats = git_command()
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
        .output();

    match (raw, stats) {
        (Ok(raw), Ok(stats))
            if raw.status.success() && stats.status.success() =>
        {
            let raw_files = parse_raw_records(&raw.stdout);
            let stat_files =
                parse_numstat(&String::from_utf8_lossy(&stats.stdout));
            let files = if is_merge_commit(repo_path, oid) {
                Vec::new()
            } else if raw_files.is_empty() {
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

fn is_merge_commit(repo_path: &str, oid: &str) -> bool {
    let output = git_command()
        .args([
            "--no-pager",
            "-C",
            repo_path,
            "rev-list",
            "--parents",
            "-n",
            "1",
            oid,
        ])
        .output();
    output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            is_merge_commit_parent_line(&String::from_utf8_lossy(
                &output.stdout,
            ))
        })
        .unwrap_or(false)
}

fn is_merge_commit_parent_line(text: &str) -> bool {
    text.lines()
        .next()
        .is_some_and(|line| line.split_whitespace().count() > 2)
}

const MAX_BLOB_SIZE: usize = 10 * 1024 * 1024;

/// Query a single file patch and, when possible, its complete old/new blobs.
fn run_file_diff(
    repo_path: &str,
    oid: &str,
    file: &FileChange,
    event_tx: &Sender<GitEvent>,
) {
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
    let oid = oid?;
    let output = git_command()
        .args(["--no-pager", "-C", repo_path, "cat-file", "blob", oid])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > MAX_BLOB_SIZE {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// 执行 git status，返回 (分支, 变更文件, ahead, behind)
fn run_status(
    repo_path: &str,
) -> Option<(String, Vec<FileStatus>, usize, usize)> {
    let output = git_command()
        .args(["-C", repo_path, "status", "--porcelain", "-b"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Some(parse_status(&text))
}

/// 解析 `git status --porcelain -b` 输出
///
/// 格式：
/// ```text
/// ## main...origin/main [ahead 1, behind 2]
///  M src/foo.rs
/// ?? new.txt
/// R  old -> new
/// ```
///
/// 已知限制：porcelain v1 对非 ASCII 文件名做引号+八进制转义，
/// 且路径含 ` -> ` 时重命名识别取箭头后部分——M4 里程碑再处理引号反转义。
fn parse_status(text: &str) -> (String, Vec<FileStatus>, usize, usize) {
    let mut branch = String::new();
    let mut ahead = 0;
    let mut behind = 0;
    let mut files = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // "main...origin/main [ahead 1, behind 2]" → 取 "..." 前分支名
            branch = rest.split("...").next().unwrap_or("").to_string();
            // 提取 ahead/behind（无上游时整段缺失）
            if let Some(a) = rest.find("[ahead ") {
                let tail = &rest[a + 7..];
                let num = tail
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("");
                ahead = num.parse().unwrap_or(0);
                if let Some(b) = tail.find("behind ") {
                    let num2 = tail[b + 7..]
                        .split(|c: char| !c.is_ascii_digit())
                        .next()
                        .unwrap_or("");
                    behind = num2.parse().unwrap_or(0);
                }
            }
        } else if line.len() >= 3 {
            let bytes = line.as_bytes();
            let index = bytes[0] as char;
            let worktree = bytes[1] as char;
            // 重命名格式 "R  old -> new"：取箭头后的新路径
            let path = if let Some(idx) = line.find(" -> ") {
                line[idx + 4..].to_string()
            } else {
                line[3..].to_string()
            };
            files.push(FileStatus {
                index,
                worktree,
                path,
            });
        }
    }
    (branch, files, ahead, behind)
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
    let refs = RefsInfo {
        remotes: non_empty_lines(&out(&["remote"])),
        remote_branches: non_empty_lines(&out(&[
            "branch",
            "-r",
            "--format=%(refname:short)",
        ])),
        tags: non_empty_lines(&out(&["tag", "--sort=-creatordate"])),
        stashes: parse_stashes(&out(&["stash", "list"])),
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

/// 执行 git log（提交图数据：oid/short/author/date/subject/装饰/parents）
fn run_log(repo_path: &str, event_tx: &Sender<GitEvent>) {
    let output = git_command()
        .args([
            "-C",
            repo_path,
            "log",
            "--all",
            "--graph",
            "--max-count=200",
            "--date=format:%Y-%m-%d %H:%M",
            "--pretty=format:%H%x00%h%x00%an%x00%ai%x00%at%x00%s%x00%D%x00%P",
        ])
        .output();
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

/// 解析 `git log --graph --pretty=format:%H%x00%h%x00%an%x00%ai%x00%at%x00%s%x00%D%x00%P` 输出
///
/// 每行结构：`<graph 区><40-hex oid>\0<short>\0<author>\0<date>\0<timestamp>\0<subject>\0<decorations>\0<parents>`
/// graph 区字符集为 `| * / \ _ . -` + 空格（不含 a-f 等 hex 字符），
/// 跳过行首 graph 字符后剩下的必以 40-hex 开头。
fn parse_log(text: &str) -> Vec<LogRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let graph_end = line
            .find(|c: char| {
                !matches!(c, '|' | '*' | '/' | '\\' | '_' | '.' | '-' | ' ')
            })
            .unwrap_or(line.len());
        let rest = &line[graph_end..];
        if rest.len() < 40 || !rest[..40].bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        let fields: Vec<&str> = rest.split('\0').collect();
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
        // %at = 作者时间戳（unix 秒，相对时间显示用）
        let timestamp = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        rows.push(LogRow {
            graph: line[..graph_end].to_string(),
            oid: fields[0].to_string(),
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
    fn merge_parent_line_detection_preserves_merge_empty_diff() {
        assert!(!is_merge_commit_parent_line("abc123\n"));
        assert!(is_merge_commit_parent_line(
            "abc123 parent-one parent-two\n"
        ));
        assert!(!is_merge_commit_parent_line(""));
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
        let text = "## main...origin/main [ahead 1, behind 2]\n M src/a.rs\nA  new.rs\n?? untracked.txt\n";
        let (branch, files, ahead, behind) = parse_status(text);
        assert_eq!(branch, "main");
        assert_eq!(ahead, 1);
        assert_eq!(behind, 2);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].index, ' ');
        assert_eq!(files[0].worktree, 'M');
        assert_eq!(files[0].path, "src/a.rs");
        assert!(files[0].is_staged() == false);
        assert!(files[1].is_staged());
        assert_eq!(files[2].code(), '?');
    }

    #[test]
    fn parse_status_no_upstream() {
        let text = "## main\n M a.rs\n";
        let (branch, _files, ahead, behind) = parse_status(text);
        assert_eq!(branch, "main");
        assert_eq!(ahead, 0);
        assert_eq!(behind, 0);
    }

    #[test]
    fn parse_status_rename_and_detached() {
        let text = "## HEAD (no branch)\nR  old.txt -> new.txt\n";
        let (branch, files, _, _) = parse_status(text);
        assert_eq!(branch, "HEAD (no branch)");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].index, 'R');
        assert_eq!(files[0].path, "new.txt");
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
}
