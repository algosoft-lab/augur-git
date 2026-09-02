//! Structured Git operations used by extension automation.
//!
//! This module deliberately sits on the Git worker side of the architecture.
//! Extensions only exchange JSON-shaped values with it; repository paths and
//! arguments are always passed to `Command` as separate arguments.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{FileStatus, GitError, git_command, run_status};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Result of one structured Git invocation.
#[derive(Clone, Debug, Serialize)]
pub struct CommandResult {
    pub ok: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub cancelled: bool,
    pub timed_out: bool,
    pub summary: String,
}

impl CommandResult {
    fn failure(summary: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: None,
            stdout: String::new(),
            stderr: String::new(),
            cancelled: false,
            timed_out: false,
            summary: summary.into(),
        }
    }

    fn with_summary(mut self) -> Self {
        self.summary = summarize_output(&self.stdout, &self.stderr, self.code);
        self
    }
}

/// Repository state captured immediately before an extension operation.
#[derive(Clone, Debug, Serialize)]
pub struct RepositoryState {
    pub branch: String,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub dirty: bool,
    pub conflicts: bool,
    pub busy: bool,
    pub operation: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub remotes: Vec<String>,
}

/// Capture Git state without mutating the repository.
pub fn capture(path: &Path) -> Result<RepositoryState, String> {
    let path_text = path.to_string_lossy().into_owned();
    let (branch, upstream, files, ahead, behind) =
        run_status(&path_text).map_err(git_error_detail)?;
    let head = read_line(path, &["rev-parse", "HEAD"]);
    let remotes = read_lines(path, &["remote"]);
    let operation = detect_operation(path);
    let conflicts = files.iter().any(FileStatus::is_conflicted);
    Ok(RepositoryState {
        branch,
        head,
        upstream,
        dirty: !files.is_empty(),
        conflicts,
        busy: operation.is_some(),
        operation,
        ahead,
        behind,
        remotes,
    })
}

/// Run arbitrary, structured Git arguments for a trusted extension.
pub fn run(
    path: &Path,
    args: &[String],
    timeout: Option<Duration>,
    cancelled: &AtomicBool,
) -> CommandResult {
    if args.is_empty() {
        return CommandResult::failure(
            "git command requires at least one argument",
        );
    }
    run_command(path, args, timeout.unwrap_or(DEFAULT_TIMEOUT), cancelled)
}

/// Execute `git pull --rebase` after checking that no unsupported operation is
/// already in progress. A conflict is returned as a normal business failure;
/// callers can then invoke an AI recovery operation.
pub fn pull_rebase(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<CommandResult, String> {
    let state = capture(path)?;
    if let Some(operation) = state.operation {
        return Ok(CommandResult::failure(format!(
            "repository has an active {operation} operation"
        )));
    }
    if state.conflicts {
        return Ok(CommandResult::failure(
            "repository has unresolved conflicts",
        ));
    }
    let result = run_command(
        path,
        &["pull".into(), "--rebase".into()],
        DEFAULT_TIMEOUT,
        cancelled,
    );
    if result.ok {
        let after = capture(path)?;
        if after.operation.is_some() || after.conflicts {
            return Ok(CommandResult {
                ok: false,
                summary: "pull completed with unresolved repository state"
                    .into(),
                ..result
            });
        }
    }
    Ok(result)
}

/// Push the captured branch. Existing upstreams use ordinary `git push`; a
/// missing upstream uses `--set-upstream` with the explicitly selected remote.
pub fn push(
    path: &Path,
    remote: Option<&str>,
    branch: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<CommandResult, String> {
    let before = capture(path)?;
    if before.operation.is_some() || before.conflicts {
        return Ok(CommandResult::failure(
            "repository has unresolved Git operation or conflicts",
        ));
    }
    let selected_branch = branch
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(before.branch.as_str());
    validate_argument("branch", selected_branch)?;
    if selected_branch != before.branch {
        return Ok(CommandResult::failure(
            "captured branch no longer matches the current branch",
        ));
    }

    let result = if before.upstream.is_some() {
        run_command(path, &["push".into()], DEFAULT_TIMEOUT, cancelled)
    } else {
        let remote = remote
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("origin");
        validate_argument("remote", remote)?;
        if !before.remotes.iter().any(|name| name == remote) {
            return Ok(CommandResult::failure(format!(
                "remote does not exist: {remote}"
            )));
        }
        run_command(
            path,
            &[
                "push".into(),
                "--set-upstream".into(),
                remote.to_string(),
                selected_branch.to_string(),
            ],
            DEFAULT_TIMEOUT,
            cancelled,
        )
    };
    if !result.ok {
        return Ok(result);
    }
    let after = capture(path)?;
    if after.branch != selected_branch
        || before.head != after.head
        || (before.upstream.is_some() && after.upstream.is_none())
        || after.upstream.is_none() && before.upstream.is_none()
        || after.operation.is_some()
        || after.conflicts
        || after.ahead != 0
    {
        return Ok(CommandResult {
            ok: false,
            summary: "push completed but repository verification failed (branch, HEAD, upstream, or ahead state changed)".into(),
            ..result
        });
    }
    Ok(result)
}

/// Resolve a path under `.git` for marker checks. `git rev-parse --git-path`
/// handles worktrees and repositories whose `.git` is a file.
fn detect_operation(path: &Path) -> Option<String> {
    let git_dir = read_line(path, &["rev-parse", "--git-dir"])?;
    let git_dir = PathBuf::from(git_dir);
    let marker = |name: &str| {
        let candidate = if git_dir.is_absolute() {
            git_dir.join(name)
        } else {
            path.join(&git_dir).join(name)
        };
        candidate.exists()
    };
    if marker("MERGE_HEAD") {
        return Some("merge".into());
    }
    if marker("rebase-merge") || marker("rebase-apply") {
        return Some("rebase".into());
    }
    if marker("CHERRY_PICK_HEAD") {
        return Some("cherry-pick".into());
    }
    if marker("REVERT_HEAD") {
        return Some("revert".into());
    }
    if marker("BISECT_LOG") {
        return Some("bisect".into());
    }
    if marker("sequencer") {
        return Some("sequencer".into());
    }
    None
}

fn read_line(path: &Path, args: &[&str]) -> Option<String> {
    let output = git_command().arg("-C").arg(path).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn read_lines(path: &Path, args: &[&str]) -> Vec<String> {
    let output = git_command().arg("-C").arg(path).args(args).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn run_command(
    path: &Path,
    args: &[String],
    timeout: Duration,
    cancelled: &AtomicBool,
) -> CommandResult {
    let mut command = git_command();
    command
        .arg("-C")
        .arg(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            log::warn!("[extension_sync] git spawn failed: {error}");
            return CommandResult::failure(error.to_string());
        }
    };
    let started = Instant::now();
    let mut was_cancelled = false;
    let mut timed_out = false;
    loop {
        if cancelled.load(Ordering::Acquire) {
            was_cancelled = true;
            let _ = child.kill();
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                return CommandResult::failure(error.to_string());
            }
        }
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => return CommandResult::failure(error.to_string()),
    };
    let stdout = bounded_text(&output.stdout);
    let stderr = bounded_text(&output.stderr);
    let result = CommandResult {
        ok: output.status.success() && !was_cancelled && !timed_out,
        code: output.status.code(),
        stdout,
        stderr,
        cancelled: was_cancelled,
        timed_out,
        summary: String::new(),
    }
    .with_summary();
    log::info!(
        "[git_command] extension command finished: ok={}, code={:?}, cancelled={}, timed_out={}",
        result.ok,
        result.code,
        result.cancelled,
        result.timed_out
    );
    result
}

fn bounded_text(bytes: &[u8]) -> String {
    let bytes = &bytes[..bytes.len().min(MAX_OUTPUT_BYTES)];
    String::from_utf8_lossy(bytes).into_owned()
}

fn summarize_output(stdout: &str, stderr: &str, code: Option<i32>) -> String {
    let summary = if !stderr.trim().is_empty() {
        stderr.trim()
    } else if !stdout.trim().is_empty() {
        stdout.trim()
    } else {
        return code
            .map(|code| format!("git exited with status {code}"))
            .unwrap_or_else(|| "git command terminated unexpectedly".into());
    };
    summary.chars().take(2000).collect()
}

fn validate_argument(kind: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.starts_with('-')
        || value.contains('\0')
        || value.chars().any(char::is_whitespace)
    {
        return Err(format!("invalid {kind}"));
    }
    Ok(())
}

fn git_error_detail(error: GitError) -> String {
    error.detail
}

#[cfg(test)]
mod tests {
    use super::validate_argument;

    #[test]
    fn rejects_option_injection_for_refs() {
        assert!(validate_argument("branch", "--force").is_err());
        assert!(validate_argument("remote", "origin/main").is_ok());
    }

    #[test]
    fn rejects_whitespace_and_empty_refs() {
        assert!(validate_argument("branch", "").is_err());
        assert!(validate_argument("branch", "feature name").is_err());
    }
}
