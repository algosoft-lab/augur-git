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

use crate::core::graph::LogRow;

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
    /// 通用命令执行结果（fetch/pull/push/commit/show…）
    CommandDone {
        label: String,
        success: bool,
        message: String,
    },
    /// 命令执行出错（含 stderr）
    Error(String),
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

    /// 请求关闭工作线程
    pub fn close(&self) {
        let _ = self.cmd_tx.send(GitCommand::Close);
    }
}

/// 打开仓库并启动工作线程
///
/// 注意：路径校验在 UI 线程同步执行（毫秒级，可接受），失败即时返回错误；
/// 之后所有 git 命令都在后台线程跑，UI 线程只经通道通信。
pub fn spawn_open(repo_path: String, event_tx: Sender<GitEvent>) -> Result<GitHandle, String> {
    let path = Path::new(&repo_path);
    if !path.is_dir() {
        return Err(format!("路径不存在: {repo_path}"));
    }
    // TODO: 子模块/worktree 的 .git 可能是文件而非目录，后续里程碑补判
    if !path.join(".git").exists() {
        return Err(format!("不是 Git 仓库: {repo_path}"));
    }

    let (cmd_tx, cmd_rx) = mpsc::channel::<GitCommand>();
    thread::spawn(move || worker_loop(repo_path, cmd_rx, event_tx));

    Ok(GitHandle { cmd_tx })
}

/// 工作线程：命令处理（git 子进程阻塞执行）
fn worker_loop(repo_path: String, cmd_rx: Receiver<GitCommand>, event_tx: Sender<GitEvent>) {
    // 打开即刷新一次
    refresh_all(&repo_path, &event_tx);

    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(GitCommand::Refresh) => refresh_all(&repo_path, &event_tx),
            Ok(GitCommand::Run { label, args }) => {
                run_git(&repo_path, &label, &args, &event_tx);
            }
            Ok(GitCommand::Close) | Err(RecvTimeoutError::Disconnected) => break,
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
}

/// 执行 git 命令（子进程阻塞，只在工作线程跑）
fn run_git(repo_path: &str, label: &str, args: &[String], event_tx: &Sender<GitEvent>) {
    let output = Command::new("git").arg("-C").arg(repo_path).args(args).output();
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
        Err(e) => GitEvent::Error(format!("git 执行失败: {e}")),
    };
    let _ = event_tx.send(event);
}

/// 执行 git status，返回 (分支, 变更文件, ahead, behind)
fn run_status(repo_path: &str) -> Option<(String, Vec<FileStatus>, usize, usize)> {
    let output = Command::new("git")
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
                let num = tail.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("");
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
    let Ok(output) = Command::new("git")
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

/// 执行 git log（提交图数据：oid/short/author/date/subject/装饰/parents）
fn run_log(repo_path: &str, event_tx: &Sender<GitEvent>) {
    let output = Command::new("git")
        .args([
            "-C",
            repo_path,
            "log",
            "--all",
            "--graph",
            "--max-count=200",
            "--date=format:%Y-%m-%d %H:%M",
            "--pretty=format:%H%x00%h%x00%an%x00%ai%x00%s%x00%D%x00%P",
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
            let _ = event_tx.send(GitEvent::Error(format!("git log 失败: {msg}")));
        }
        Err(e) => {
            let _ = event_tx.send(GitEvent::Error(format!("git 执行失败: {e}")));
        }
    }
}

/// 解析 `git log --graph --pretty=format:%H%x00%h%x00%an%x00%ai%x00%s%x00%D%x00%P` 输出
///
/// 每行结构：`<graph 区><40-hex oid>\0<short>\0<author>\0<date>\0<subject>\0<decorations>\0<parents>`
/// graph 区字符集为 `| * / \ _ . -` + 空格（不含 a-f 等 hex 字符），
/// 跳过行首 graph 字符后剩下的必以 40-hex 开头。
fn parse_log(text: &str) -> Vec<LogRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let graph_end = line
            .find(|c: char| !matches!(c, '|' | '*' | '/' | '\\' | '_' | '.' | '-' | ' '))
            .unwrap_or(line.len());
        let rest = &line[graph_end..];
        if rest.len() < 40 || !rest[..40].bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let fields: Vec<&str> = rest.split('\0').collect();
        if fields.len() < 6 {
            continue;
        }
        let parents = fields
            .get(6)
            .copied()
            .unwrap_or("")
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        rows.push(LogRow {
            graph: line[..graph_end].to_string(),
            oid: fields[0].to_string(),
            short: fields[1].to_string(),
            author: fields[2].to_string(),
            date: fields[3].to_string(),
            subject: fields[4].to_string(),
            decorations: fields[5].to_string(),
            parents,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let text = "0123456789abcdef0123456789abcdef01234567\0short\0Lionel Fung\02026-08-13 20:00\0M0 框架\0HEAD -> main\0\n";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].oid, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(rows[0].subject, "M0 框架");
        assert_eq!(rows[0].decorations, "HEAD -> main");
        assert!(rows[0].parents.is_empty());
    }

    #[test]
    fn parse_log_with_parents() {
        let text = "0123456789abcdef0123456789abcdef01234567\0s\0a\0d\0merge 分支\0HEAD -> main\089abcdef0123456789abcdef0123456789abcdef ffff0123456789abcdef0123456789abcdef01\n";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].oid, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(rows[0].subject, "merge 分支");
        assert_eq!(rows[0].parents.len(), 2);
        assert_eq!(rows[0].parents[0], "89abcdef0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn parse_log_skips_bad_lines() {
        let text = "garbage\n0123456789abcdef0123456789abcdef01234567\0s\0a\0d\0提交\0\n";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
    }
}
