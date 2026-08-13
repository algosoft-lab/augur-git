//! M1：Git 命令层（镜像 augur-com 的 core/serial.rs 双通道线程模式）
//!
//! 架构：
//! - 专用工作线程跑阻塞式 `git` 子进程，事件经 `std::sync::mpsc` 推给 UI（20ms 轮询 try_recv）
//! - UI → 后台指令：`std::sync::mpsc`（send 无阻塞，即发即返）
//! - 读写全走后台线程，UI 线程零阻塞
//! - 当前调用系统 git 可执行文件（PATH 查找）；后续里程碑可换 git2/libgit2 做对象级访问

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

/// 后台 → UI 事件
pub enum GitEvent {
    /// 状态结果（分支 + 变更文件）
    Status {
        branch: String,
        files: Vec<FileStatus>,
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
}

/// UI → 后台指令
pub enum GitCommand {
    /// 刷新状态（git status --porcelain -b）
    Refresh,
    /// 关闭工作线程
    Close,
}

/// 工作线程句柄（UI 侧持有）
pub struct GitHandle {
    cmd_tx: Sender<GitCommand>,
}

impl GitHandle {
    /// 请求刷新状态（std mpsc 无界通道，send 即返回）
    pub fn refresh(&self) {
        let _ = self.cmd_tx.send(GitCommand::Refresh);
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
) -> Result<GitHandle, String> {
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
    run_status(&repo_path, &event_tx);

    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(GitCommand::Refresh) => run_status(&repo_path, &event_tx),
            Ok(GitCommand::Close) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// 执行 git status（子进程阻塞，只在工作线程跑）
fn run_status(repo_path: &str, event_tx: &Sender<GitEvent>) {
    let output = Command::new("git")
        .args(["-C", repo_path, "status", "--porcelain", "-b"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let (branch, files) = parse_status(&text);
            let _ = event_tx.send(GitEvent::Status { branch, files });
        }
        Ok(output) => {
            let msg = String::from_utf8_lossy(&output.stderr);
            let _ = event_tx.send(GitEvent::Error(format!("git status 失败: {msg}")));
        }
        Err(e) => {
            let _ = event_tx.send(GitEvent::Error(format!("git 执行失败: {e}")));
        }
    }
}

/// 解析 `git status --porcelain -b` 输出
///
/// 格式：
/// ```text
/// ## main...origin/main [ahead 1]
///  M src/foo.rs
/// ?? new.txt
/// R  old -> new
/// ```
///
/// 已知限制：porcelain v1 对非 ASCII 文件名做引号+八进制转义，
/// 且路径含 ` -> ` 时重命名识别取箭头后部分——M4 里程碑再处理引号反转义。
fn parse_status(text: &str) -> (String, Vec<FileStatus>) {
    let mut branch = String::new();
    let mut files = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // "main...origin/main [ahead 1]" → 取 "..." 前分支名
            branch = rest.split("...").next().unwrap_or("").to_string();
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
    (branch, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_normal() {
        let text = "## main...origin/main\n M src/a.rs\nA  new.rs\n?? untracked.txt\n";
        let (branch, files) = parse_status(text);
        assert_eq!(branch, "main");
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].index, ' ');
        assert_eq!(files[0].worktree, 'M');
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(files[1].index, 'A');
        assert_eq!(files[1].path, "new.rs");
        assert_eq!(files[2].code(), '?');
        assert_eq!(files[2].path, "untracked.txt");
    }

    #[test]
    fn parse_status_rename_and_detached() {
        let text = "## HEAD (no branch)\nR  old.txt -> new.txt\n";
        let (branch, files) = parse_status(text);
        assert_eq!(branch, "HEAD (no branch)");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].index, 'R');
        assert_eq!(files[0].path, "new.txt");
    }

    #[test]
    fn parse_status_empty() {
        let text = "## main\n";
        let (branch, files) = parse_status(text);
        assert_eq!(branch, "main");
        assert!(files.is_empty());
    }
}
