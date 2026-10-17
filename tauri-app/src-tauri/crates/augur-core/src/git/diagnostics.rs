//! Cancellable repository diagnostics kept off the serial Git command queue.

use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{DiffLineStats, FileStatus, GitRepo, RepoLocation};
use crate::diff::parse_numstat_z;

pub const MAX_DIFF_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
const MAX_STATS_UNTRACKED_FILES: usize = 200;
const MAX_STATS_FILE_BYTES: u64 = 1024 * 1024;
const MAX_STATS_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const STATS_CATEGORY_TIMEOUT: Duration = Duration::from_secs(2);
pub const WORKING_DIFF_TIMEOUT: Duration = Duration::from_secs(10);
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const STATUS_OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
const STDERR_CAPTURE_LIMIT: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffStatsStatus {
    Pending,
    Ready,
    Skipped,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffStatCategory {
    pub status: DiffStatsStatus,
    pub stats: Option<DiffLineStats>,
    pub reason: Option<String>,
}

impl DiffStatCategory {
    pub fn pending() -> Self {
        Self {
            status: DiffStatsStatus::Pending,
            stats: None,
            reason: None,
        }
    }

    fn ready(stats: DiffLineStats) -> Self {
        Self {
            status: DiffStatsStatus::Ready,
            stats: Some(stats),
            reason: None,
        }
    }

    fn skipped(reason: &str) -> Self {
        Self {
            status: DiffStatsStatus::Skipped,
            stats: None,
            reason: Some(reason.to_string()),
        }
    }

    fn error(reason: &str) -> Self {
        Self {
            status: DiffStatsStatus::Error,
            stats: None,
            reason: Some(reason.to_string()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingTreeDiffStatsReport {
    pub staged: DiffStatCategory,
    pub unstaged: DiffStatCategory,
    pub untracked: DiffStatCategory,
}

impl WorkingTreeDiffStatsReport {
    pub fn pending() -> Self {
        Self {
            staged: DiffStatCategory::pending(),
            unstaged: DiffStatCategory::pending(),
            untracked: DiffStatCategory::pending(),
        }
    }
}

/// A worker that runs at most one job and replaces queued work with the newest
/// request. Submitting a new request asks the current process to stop.
pub struct LatestTaskWorker<T: Send + 'static> {
    shared: Arc<Shared<T>>,
}

struct Shared<T> {
    state: Mutex<WorkerState<T>>,
    changed: Condvar,
}

struct WorkerState<T> {
    pending: Option<(T, Arc<AtomicBool>)>,
    running: Option<Arc<AtomicBool>>,
    closed: bool,
}

impl<T: Send + 'static> Clone for LatestTaskWorker<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T: Send + 'static> LatestTaskWorker<T> {
    pub fn new(name: impl Into<String>, execute: impl Fn(T, &AtomicBool) + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(WorkerState {
                pending: None,
                running: None,
                closed: false,
            }),
            changed: Condvar::new(),
        });
        let worker_shared = shared.clone();
        thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                loop {
                    let (job, cancel) = {
                        let mut state =
                            worker_shared.state.lock().expect("diagnostics worker lock");
                        loop {
                            if let Some(job) = state.pending.take() {
                                state.running = Some(job.1.clone());
                                break job;
                            }
                            if state.closed {
                                return;
                            }
                            state = worker_shared
                                .changed
                                .wait(state)
                                .expect("diagnostics worker lock");
                        }
                    };
                    execute(job, &cancel);
                    let mut state = worker_shared.state.lock().expect("diagnostics worker lock");
                    if state
                        .running
                        .as_ref()
                        .is_some_and(|running| Arc::ptr_eq(running, &cancel))
                    {
                        state.running = None;
                    }
                }
            })
            .expect("diagnostics worker thread");
        Self { shared }
    }

    pub fn submit(&self, job: T) {
        let mut state = self.shared.state.lock().expect("diagnostics worker lock");
        if state.closed {
            return;
        }
        if let Some(running) = &state.running {
            running.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        state.pending = Some((job, cancel));
        self.shared.changed.notify_one();
    }

    pub fn cancel(&self) {
        let mut state = self.shared.state.lock().expect("diagnostics worker lock");
        state.pending = None;
        if let Some(running) = &state.running {
            running.store(true, Ordering::Release);
        }
    }

    pub fn shutdown(&self) {
        let mut state = self.shared.state.lock().expect("diagnostics worker lock");
        state.closed = true;
        state.pending = None;
        if let Some(running) = &state.running {
            running.store(true, Ordering::Release);
        }
        self.shared.changed.notify_one();
    }
}

#[derive(Debug)]
pub enum ProcessFailure {
    Cancelled,
    TimedOut,
    OutputTooLarge,
    Spawn(String),
    Read(String),
}

impl std::fmt::Display for ProcessFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Git request was cancelled"),
            Self::TimedOut => formatter.write_str("Git request exceeded its time limit"),
            Self::OutputTooLarge => formatter.write_str("Git output exceeded its size limit"),
            Self::Spawn(detail) | Self::Read(detail) => formatter.write_str(detail),
        }
    }
}

impl std::error::Error for ProcessFailure {}

/// Place a child in its own process group so Git helpers can be terminated with it.
pub fn configure_process_tree(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

/// Stop a Git process and helpers it spawned before joining their output pipes.
pub fn terminate_process_tree(child: &mut Child) {
    let process_id = child.id();
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(process_id as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // Redirecting output alone does not prevent a Windows console from opening.
        let _ = Command::new("taskkill")
            .creation_flags(super::location::CREATE_NO_WINDOW)
            .args(["/PID", &process_id.to_string(), "/T", "/F"])
            .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run a child while polling cancellation/deadline and draining both pipes.
/// Output is retained only up to the requested caps; excess data is discarded
/// and stdout overflow terminates the child.
pub fn run_cancellable(
    command: &mut Command,
    cancel: &AtomicBool,
    timeout: Duration,
    stdout_limit: usize,
) -> Result<Output, ProcessFailure> {
    run_cancellable_until(command, cancel, Instant::now() + timeout, stdout_limit)
}

pub fn run_cancellable_until(
    command: &mut Command,
    cancel: &AtomicBool,
    deadline: Instant,
    stdout_limit: usize,
) -> Result<Output, ProcessFailure> {
    if cancel.load(Ordering::Acquire) {
        return Err(ProcessFailure::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ProcessFailure::TimedOut);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_process_tree(command);
    let mut child = command
        .spawn()
        .map_err(|error| ProcessFailure::Spawn(error.to_string()))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_exceeded = exceeded.clone();
    let stdout_reader =
        thread::spawn(move || drain_capped(stdout, stdout_limit, Some(stdout_exceeded)));
    let stderr_reader = thread::spawn(move || drain_capped(stderr, STDERR_CAPTURE_LIMIT, None));
    let status = loop {
        if cancel.load(Ordering::Acquire) {
            terminate_process_tree(&mut child);
            join_pipe(stdout_reader)?;
            join_pipe(stderr_reader)?;
            return Err(ProcessFailure::Cancelled);
        }
        if exceeded.load(Ordering::Acquire) {
            terminate_process_tree(&mut child);
            join_pipe(stdout_reader)?;
            join_pipe(stderr_reader)?;
            return Err(ProcessFailure::OutputTooLarge);
        }
        if Instant::now() >= deadline {
            terminate_process_tree(&mut child);
            join_pipe(stdout_reader)?;
            join_pipe(stderr_reader)?;
            return Err(ProcessFailure::TimedOut);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                terminate_process_tree(&mut child);
                join_pipe(stdout_reader)?;
                join_pipe(stderr_reader)?;
                return Err(ProcessFailure::Read(error.to_string()));
            }
        }
    };
    let stdout = join_pipe(stdout_reader)?;
    let stderr = join_pipe(stderr_reader)?;
    if exceeded.load(Ordering::Acquire) {
        return Err(ProcessFailure::OutputTooLarge);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn drain_capped(mut reader: impl Read, limit: usize, overflow: Option<Arc<AtomicBool>>) -> Vec<u8> {
    let mut retained = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let room = limit.saturating_sub(retained.len());
                retained.extend_from_slice(&buffer[..read.min(room)]);
                if read > room {
                    if let Some(overflow) = &overflow {
                        overflow.store(true, Ordering::Release);
                    }
                }
            }
        }
    }
    retained
}

fn join_pipe(reader: thread::JoinHandle<Vec<u8>>) -> Result<Vec<u8>, ProcessFailure> {
    reader
        .join()
        .map_err(|_| ProcessFailure::Read("Git output reader stopped unexpectedly".into()))
}

pub fn read_diff_stats(
    repo: &GitRepo,
    files: &[FileStatus],
    cancel: &AtomicBool,
) -> WorkingTreeDiffStatsReport {
    let mut report = WorkingTreeDiffStatsReport::pending();
    let started = Instant::now();
    report.staged = read_tracked_numstat(repo, true, cancel);
    log_stats_category("staged", files.len(), started, &report.staged);
    if cancel.load(Ordering::Acquire) {
        return report;
    }
    let started = Instant::now();
    report.unstaged = read_tracked_numstat(repo, false, cancel);
    log_stats_category("unstaged", files.len(), started, &report.unstaged);
    if cancel.load(Ordering::Acquire) {
        return report;
    }
    let started = Instant::now();
    report.untracked = read_untracked_stats(repo, files, cancel);
    let untracked_count = files.iter().filter(|file| file.is_untracked()).count();
    log_stats_category("untracked", untracked_count, started, &report.untracked);
    report
}

fn log_stats_category(name: &str, files: usize, started: Instant, category: &DiffStatCategory) {
    log::debug!(
        "[git_worktree] statistics category completed: category={name}, files={files}, status={:?}, reason={:?}, elapsed_ms={}",
        category.status,
        category.reason,
        started.elapsed().as_millis()
    );
}

fn read_tracked_numstat(repo: &GitRepo, staged: bool, cancel: &AtomicBool) -> DiffStatCategory {
    let mut command = repo.command();
    command.args(["--no-optional-locks", "-C", repo.path(), "diff"]);
    if staged {
        command.arg("--cached");
    }
    let result = run_cancellable(
        command.args(["--no-ext-diff", "--find-renames", "--numstat", "-z"]),
        cancel,
        STATS_CATEGORY_TIMEOUT,
        STATUS_OUTPUT_LIMIT,
    );
    match result {
        Ok(output) if output.status.success() => {
            DiffStatCategory::ready(sum_numstat(&output.stdout))
        }
        Err(ProcessFailure::Cancelled) => DiffStatCategory::pending(),
        Err(ProcessFailure::TimedOut) => DiffStatCategory::error("timed-out"),
        _ => DiffStatCategory::error("unavailable"),
    }
}

fn read_untracked_stats(
    repo: &GitRepo,
    files: &[FileStatus],
    cancel: &AtomicBool,
) -> DiffStatCategory {
    let untracked: Vec<_> = files.iter().filter(|file| file.is_untracked()).collect();
    if untracked.is_empty() {
        return DiffStatCategory::ready(DiffLineStats::default());
    }
    if untracked.len() > MAX_STATS_UNTRACKED_FILES {
        return DiffStatCategory::skipped("too-many-files");
    }

    let deadline = Instant::now() + STATS_CATEGORY_TIMEOUT;
    let mut total_bytes = 0_u64;
    for file in &untracked {
        if cancel.load(Ordering::Acquire) {
            return DiffStatCategory::pending();
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return DiffStatCategory::error("timed-out");
        }
        let size = match untracked_file_size(repo, &file.path, cancel, remaining) {
            Ok(size) => size,
            Err(ProcessFailure::Cancelled) => return DiffStatCategory::pending(),
            Err(ProcessFailure::TimedOut) => return DiffStatCategory::error("timed-out"),
            Err(_) => return DiffStatCategory::error("unavailable"),
        };
        if size > MAX_STATS_FILE_BYTES {
            return DiffStatCategory::skipped("file-too-large");
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_STATS_TOTAL_BYTES {
            return DiffStatCategory::skipped("total-too-large");
        }
    }

    let mut total = DiffLineStats::default();
    for file in untracked {
        if cancel.load(Ordering::Acquire) {
            return DiffStatCategory::pending();
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return DiffStatCategory::error("timed-out");
        }
        let result = read_untracked_numstat(repo, &file.path, cancel, remaining);
        match result {
            Ok(stats) => add_stats(&mut total, stats),
            Err(ProcessFailure::Cancelled) => return DiffStatCategory::pending(),
            Err(ProcessFailure::TimedOut) => return DiffStatCategory::error("timed-out"),
            Err(_) => return DiffStatCategory::error("unavailable"),
        }
    }
    DiffStatCategory::ready(total)
}

#[cfg(test)]
mod untracked_stats_tests {
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    use super::{
        DiffStatsStatus, MAX_STATS_FILE_BYTES, MAX_STATS_TOTAL_BYTES, read_untracked_stats,
    };
    use crate::git::{FileStatus, GitRepo};

    fn untracked(path: impl Into<String>) -> FileStatus {
        FileStatus {
            index: '?',
            worktree: '?',
            path: path.into(),
            old_path: None,
            is_lfs: false,
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "augur-diagnostics-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create diagnostic test directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    static TEMP_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[test]
    fn untracked_statistics_skip_before_probing_more_than_two_hundred_files() {
        let repo = GitRepo::local(std::env::temp_dir().to_string_lossy());
        let files = (0..201)
            .map(|index| untracked(format!("missing-{index}")))
            .collect::<Vec<_>>();
        let result = read_untracked_stats(&repo, &files, &AtomicBool::new(false));
        assert_eq!(result.status, DiffStatsStatus::Skipped);
        assert_eq!(result.reason.as_deref(), Some("too-many-files"));
    }

    #[test]
    fn untracked_statistics_skip_a_single_file_over_one_mib() {
        let directory = TempDir::new();
        let path = directory.0.join("oversized");
        File::create(&path)
            .expect("create sparse oversized fixture")
            .set_len(MAX_STATS_FILE_BYTES + 1)
            .expect("size sparse oversized fixture");
        let repo = GitRepo::local(directory.0.to_string_lossy());
        let result =
            read_untracked_stats(&repo, &[untracked("oversized")], &AtomicBool::new(false));
        assert_eq!(result.status, DiffStatsStatus::Skipped);
        assert_eq!(result.reason.as_deref(), Some("file-too-large"));
    }

    #[test]
    fn untracked_statistics_skip_when_their_total_exceeds_sixteen_mib() {
        let directory = TempDir::new();
        let files = (0..17)
            .map(|index| {
                let name = format!("part-{index}");
                File::create(directory.0.join(&name))
                    .expect("create sparse total-size fixture")
                    .set_len(MAX_STATS_FILE_BYTES)
                    .expect("size sparse total-size fixture");
                untracked(name)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            17 * MAX_STATS_FILE_BYTES,
            MAX_STATS_TOTAL_BYTES + MAX_STATS_FILE_BYTES
        );
        let repo = GitRepo::local(directory.0.to_string_lossy());
        let result = read_untracked_stats(&repo, &files, &AtomicBool::new(false));
        assert_eq!(result.status, DiffStatsStatus::Skipped);
        assert_eq!(result.reason.as_deref(), Some("total-too-large"));
    }

    #[test]
    fn a_missing_untracked_file_is_reported_as_unavailable() {
        let directory = TempDir::new();
        let repo = GitRepo::local(directory.0.to_string_lossy());
        let result = read_untracked_stats(&repo, &[untracked("vanished")], &AtomicBool::new(false));
        assert_eq!(result.status, DiffStatsStatus::Error);
        assert_eq!(result.reason.as_deref(), Some("unavailable"));
    }
}

fn untracked_file_size(
    repo: &GitRepo,
    path: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<u64, ProcessFailure> {
    match repo.location() {
        RepoLocation::Local => std::fs::metadata(std::path::Path::new(repo.path()).join(path))
            .map(|metadata| metadata.len())
            .map_err(|error| ProcessFailure::Read(error.to_string())),
        RepoLocation::Wsl { .. } => {
            let output = run_cancellable(
                repo.command_in_location("stat")
                    .args(["-c", "%s", "--", path]),
                cancel,
                timeout,
                64,
            )?;
            if !output.status.success() {
                return Err(ProcessFailure::Read("stat failed".into()));
            }
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .map_err(|_| ProcessFailure::Read("stat returned an invalid file size".into()))
        }
    }
}

fn read_untracked_numstat(
    repo: &GitRepo,
    path: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<DiffLineStats, ProcessFailure> {
    let output = run_cancellable(
        repo.command().args([
            "--no-optional-locks",
            "-C",
            repo.path(),
            "--no-pager",
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--numstat",
            "-z",
            "--",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
            path,
        ]),
        cancel,
        timeout,
        4096,
    )?;
    if output.status.success() || output.status.code() == Some(1) {
        Ok(sum_numstat(&output.stdout))
    } else {
        Err(ProcessFailure::Read("git diff failed".into()))
    }
}

fn sum_numstat(data: &[u8]) -> DiffLineStats {
    parse_numstat_z(data)
        .into_iter()
        .fold(DiffLineStats::default(), |mut total, file| {
            if let (Some(added), Some(deleted)) = (file.added, file.deleted) {
                total.added = total.added.saturating_add(added);
                total.deleted = total.deleted.saturating_add(deleted);
            }
            total
        })
}

fn add_stats(total: &mut DiffLineStats, next: DiffLineStats) {
    total.added = total.added.saturating_add(next.added);
    total.deleted = total.deleted.saturating_add(next.deleted);
}

pub fn run_status_bounded(command: &mut Command) -> Result<Output, ProcessFailure> {
    run_cancellable(
        command,
        &AtomicBool::new(false),
        STATUS_TIMEOUT,
        STATUS_OUTPUT_LIMIT,
    )
}

pub fn run_working_diff(
    command: &mut Command,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Output, ProcessFailure> {
    run_cancellable_until(command, cancel, deadline, MAX_DIFF_OUTPUT_BYTES)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::atomic::AtomicBool;

    use super::{LatestTaskWorker, ProcessFailure, run_cancellable};

    #[test]
    fn child_output_is_bounded_and_oversized_children_are_terminated() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let mut command = Command::new("git");
        command.arg("--version");
        assert!(matches!(
            run_cancellable(
                &mut command,
                &AtomicBool::new(false),
                std::time::Duration::from_secs(2),
                1
            ),
            Err(ProcessFailure::OutputTooLarge)
        ));
    }

    #[test]
    fn a_pre_cancelled_child_is_not_started() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let cancel = AtomicBool::new(true);
        let mut command = Command::new("git");
        command.arg("--version");
        assert!(matches!(
            run_cancellable(&mut command, &cancel, std::time::Duration::from_secs(3), 64),
            Err(ProcessFailure::Cancelled)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn timed_out_children_are_killed_and_reaped() {
        use std::time::{Duration, Instant};

        let started = Instant::now();
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 5"]);
        assert!(matches!(
            run_cancellable(
                &mut command,
                &AtomicBool::new(false),
                Duration::from_millis(40),
                64
            ),
            Err(ProcessFailure::TimedOut)
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_kills_and_reaps_a_running_child() {
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            signal.store(true, std::sync::atomic::Ordering::Release);
        });
        let started = Instant::now();
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 5"]);
        let result = run_cancellable(&mut command, &cancel, Duration::from_secs(3), 64);
        canceller.join().unwrap();
        assert!(matches!(result, Err(ProcessFailure::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(windows)]
    fn windows_process_tree() -> Command {
        use std::os::windows::process::CommandExt;

        let mut command = Command::new("cmd.exe");
        // The ping descendant inherits both pipes; killing only cmd leaves them open.
        command
            .creation_flags(super::super::location::CREATE_NO_WINDOW)
            .args(["/D", "/C", "ping.exe -n 30 127.0.0.1"]);
        command
    }

    #[cfg(windows)]
    #[test]
    fn windows_timeout_terminates_descendants_and_closes_pipes() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (sender, receiver) = mpsc::channel();
        let runner = std::thread::spawn(move || {
            let result = run_cancellable(
                &mut windows_process_tree(),
                &AtomicBool::new(false),
                Duration::from_secs(1),
                1024,
            );
            sender.send(result).unwrap();
        });
        let result = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("timed-out process tree must release its output pipes");
        runner.join().unwrap();
        assert!(matches!(result, Err(ProcessFailure::TimedOut)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_cancellation_terminates_descendants_and_closes_pipes() {
        use std::sync::{Arc, mpsc};
        use std::time::Duration;

        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let runner = std::thread::spawn(move || {
            let result = run_cancellable(
                &mut windows_process_tree(),
                &cancel,
                Duration::from_secs(10),
                1024,
            );
            sender.send(result).unwrap();
        });
        std::thread::sleep(Duration::from_secs(1));
        signal.store(true, std::sync::atomic::Ordering::Release);
        let result = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("cancelled process tree must release its output pipes");
        runner.join().unwrap();
        assert!(matches!(result, Err(ProcessFailure::Cancelled)));
    }

    #[test]
    fn latest_worker_replaces_pending_jobs() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (done, received) = mpsc::channel();
        let worker = LatestTaskWorker::new("test-latest-task", move |job: u8, cancel| {
            if job == 1 {
                for _ in 0..50 {
                    if cancel.load(std::sync::atomic::Ordering::Acquire) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            let _ = done.send(job);
        });
        worker.submit(1);
        std::thread::sleep(Duration::from_millis(5));
        worker.submit(2);
        worker.submit(3);
        assert_eq!(received.recv_timeout(Duration::from_secs(1)).unwrap(), 3);
        assert!(received.recv_timeout(Duration::from_millis(30)).is_err());
        worker.shutdown();
    }
}
