//! Focused, single-repository automatic refresh monitoring.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use augur_core::git::{GitHandle, GitRepo};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

const WORKTREE_CHANGE: u8 = 1;
const METADATA_CHANGE: u8 = 2;
const QUIET_PERIOD: Duration = Duration::from_millis(800);
const MAX_EVENT_WAIT: Duration = Duration::from_secs(3);
const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const SAFETY_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const FALLBACK_STATUS_INTERVAL: Duration = Duration::from_secs(5);
const FALLBACK_FULL_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct RefreshTarget {
    pub id: u64,
    pub repo: GitRepo,
    pub handle: GitHandle,
}

enum Message {
    Target(Option<RefreshTarget>),
    Enabled(bool),
    Focused(bool),
    WatchWake(u64),
    WatchFailed(u64, String),
    Shutdown,
}

#[derive(Default)]
struct WatchSignal {
    dirty: AtomicU8,
    wake_pending: AtomicBool,
}

struct ActiveWatch {
    generation: u64,
    signal: Arc<WatchSignal>,
    _watcher: RecommendedWatcher,
}

pub struct AutoRefreshController {
    sender: Sender<Message>,
}

impl AutoRefreshController {
    pub fn new(enabled: bool) -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker_sender = sender.clone();
        thread::Builder::new()
            .name("augur-auto-refresh".to_string())
            .spawn(move || run(receiver, worker_sender, enabled))
            .expect("auto-refresh controller thread");
        Self { sender }
    }

    pub fn set_target(&self, target: Option<RefreshTarget>) {
        let _ = self.sender.send(Message::Target(target));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.sender.send(Message::Enabled(enabled));
    }

    pub fn set_focused(&self, focused: bool) {
        let _ = self.sender.send(Message::Focused(focused));
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(Message::Shutdown);
    }
}

struct Runtime {
    target: Option<RefreshTarget>,
    enabled: bool,
    focused: bool,
    active: bool,
    generation: u64,
    watcher: Option<ActiveWatch>,
    fallback: bool,
    first_change: Option<Instant>,
    quiet_deadline: Option<Instant>,
    full_change: bool,
    last_refresh: Option<Instant>,
    next_status_poll: Option<Instant>,
    next_full_poll: Option<Instant>,
}

impl Runtime {
    fn new(enabled: bool) -> Self {
        Self {
            target: None,
            enabled,
            focused: false,
            active: false,
            generation: 0,
            watcher: None,
            fallback: false,
            first_change: None,
            quiet_deadline: None,
            full_change: false,
            last_refresh: None,
            next_status_poll: None,
            next_full_poll: None,
        }
    }

    fn is_allowed(&self) -> bool {
        self.enabled && self.focused && self.target.is_some()
    }

    fn set_target(&mut self, target: Option<RefreshTarget>, sender: &Sender<Message>) {
        if self.target.as_ref().map(|target| target.id) == target.as_ref().map(|target| target.id) {
            self.target = target;
            return;
        }
        self.deactivate();
        self.target = target;
        self.activate_if_allowed(sender, false);
    }

    fn set_enabled(&mut self, enabled: bool, sender: &Sender<Message>) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if enabled {
            self.activate_if_allowed(sender, true);
        } else {
            self.deactivate();
        }
    }

    fn set_focused(&mut self, focused: bool, sender: &Sender<Message>) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if focused {
            self.activate_if_allowed(sender, true);
        } else {
            self.deactivate();
        }
    }

    fn activate_if_allowed(&mut self, sender: &Sender<Message>, refresh_now: bool) {
        if !self.is_allowed() || self.active {
            return;
        }
        self.active = true;
        self.generation = self.generation.wrapping_add(1);
        let now = Instant::now();
        self.last_refresh = Some(now);
        self.next_status_poll = None;
        self.next_full_poll = None;
        self.first_change = None;
        self.quiet_deadline = None;
        self.full_change = false;

        let target = self.target.as_ref().expect("allowed target").clone();
        self.fallback = target.repo.is_wsl();
        if !self.fallback {
            match install_watchers(&target, self.generation, sender) {
                Ok(watcher) => {
                    self.watcher = Some(watcher);
                    self.next_full_poll = Some(now + SAFETY_REFRESH_INTERVAL);
                }
                Err(error) => {
                    log::warn!(
                        "[auto_refresh] watcher setup failed for repo {}: {error}",
                        target.id
                    );
                    self.fallback = true;
                }
            }
        }
        if self.fallback {
            self.next_status_poll = Some(now + FALLBACK_STATUS_INTERVAL);
            self.next_full_poll = Some(now + FALLBACK_FULL_INTERVAL);
        }
        if refresh_now {
            target.handle.refresh_automatically(true);
            self.last_refresh = Some(now);
            self.reset_poll_deadlines(now);
        }
        log::debug!(
            "[auto_refresh] monitoring repo {} (fallback={})",
            target.id,
            self.fallback
        );
    }

    fn deactivate(&mut self) {
        if !self.active {
            self.watcher = None;
            self.first_change = None;
            self.quiet_deadline = None;
            self.next_status_poll = None;
            self.next_full_poll = None;
            return;
        }
        if let Some(target) = &self.target {
            log::debug!("[auto_refresh] paused repo {}", target.id);
        }
        self.active = false;
        self.watcher = None;
        self.first_change = None;
        self.quiet_deadline = None;
        self.full_change = false;
        self.next_status_poll = None;
        self.next_full_poll = None;
    }

    fn accept_change(&mut self, bits: u8) {
        if bits == 0 || !self.active {
            return;
        }
        let now = Instant::now();
        if self.first_change.is_none() {
            self.first_change = Some(now);
        }
        self.quiet_deadline = Some(now + QUIET_PERIOD);
        self.full_change |= bits & METADATA_CHANGE != 0;
    }

    fn next_deadline(&self) -> Option<Instant> {
        if !self.active {
            return None;
        }
        let mut next = self.next_status_poll.min(self.next_full_poll);
        if let (Some(quiet), Some(first)) = (self.quiet_deadline, self.first_change) {
            let deadline = quiet.min(first + MAX_EVENT_WAIT);
            let min_ready = self
                .last_refresh
                .map(|last| last + MIN_REFRESH_INTERVAL)
                .unwrap_or(deadline);
            let deadline = deadline.max(min_ready);
            next = Some(next.map(|value| value.min(deadline)).unwrap_or(deadline));
        }
        next
    }

    fn run_due(&mut self) {
        if !self.active {
            return;
        }
        let now = Instant::now();
        if self.first_change.is_some() {
            let quiet = self.quiet_deadline.unwrap_or(now);
            let max_wait = self.first_change.unwrap_or(now) + MAX_EVENT_WAIT;
            let min_ready = self
                .last_refresh
                .map(|last| last + MIN_REFRESH_INTERVAL)
                .unwrap_or(now);
            if now >= quiet.min(max_wait).max(min_ready) {
                let full = self.full_change;
                self.request_refresh(full, now);
                self.schedule_after_refresh(full, now);
                self.first_change = None;
                self.quiet_deadline = None;
                self.full_change = false;
                return;
            }
        }

        if self.next_full_poll.is_some_and(|deadline| now >= deadline) {
            self.request_refresh(true, now);
            self.schedule_after_refresh(true, now);
        } else if self
            .next_status_poll
            .is_some_and(|deadline| now >= deadline)
        {
            self.request_refresh(false, now);
            self.schedule_after_refresh(false, now);
        }
    }

    fn request_refresh(&mut self, full: bool, now: Instant) {
        if let Some(target) = &self.target {
            target.handle.refresh_automatically(full);
            self.last_refresh = Some(now);
            log::debug!(
                "[auto_refresh] requested {} refresh for repo {}",
                if full { "full" } else { "status" },
                target.id
            );
        }
    }

    fn reset_poll_deadlines(&mut self, now: Instant) {
        if self.fallback {
            self.next_status_poll = Some(now + FALLBACK_STATUS_INTERVAL);
            self.next_full_poll = Some(now + FALLBACK_FULL_INTERVAL);
        } else {
            self.next_status_poll = None;
            self.next_full_poll = Some(now + SAFETY_REFRESH_INTERVAL);
        }
    }

    fn schedule_after_refresh(&mut self, full: bool, now: Instant) {
        if self.fallback {
            self.next_status_poll = Some(now + FALLBACK_STATUS_INTERVAL);
        }
        if full {
            self.next_full_poll = Some(
                now + if self.fallback {
                    FALLBACK_FULL_INTERVAL
                } else {
                    SAFETY_REFRESH_INTERVAL
                },
            );
        } else if let Some(deadline) = self.next_full_poll {
            self.next_full_poll = Some(deadline.max(now + MIN_REFRESH_INTERVAL));
        }
    }

    fn watcher_wake(&mut self, generation: u64, sender: &Sender<Message>) {
        let Some((watch_generation, signal)) = self
            .watcher
            .as_ref()
            .map(|watcher| (watcher.generation, watcher.signal.clone()))
        else {
            return;
        };
        if watch_generation != generation {
            return;
        }
        let bits = signal.dirty.swap(0, Ordering::AcqRel);
        signal.wake_pending.store(false, Ordering::Release);
        self.accept_change(bits);
        let remaining = signal.dirty.load(Ordering::Acquire);
        if remaining != 0 && !signal.wake_pending.swap(true, Ordering::AcqRel) {
            let _ = sender.send(Message::WatchWake(generation));
        }
    }
}

fn run(receiver: Receiver<Message>, sender: Sender<Message>, enabled: bool) {
    let mut runtime = Runtime::new(enabled);
    loop {
        let timeout = runtime
            .next_deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(24 * 60 * 60));
        match receiver.recv_timeout(timeout) {
            Ok(Message::Target(target)) => runtime.set_target(target, &sender),
            Ok(Message::Enabled(enabled)) => runtime.set_enabled(enabled, &sender),
            Ok(Message::Focused(focused)) => runtime.set_focused(focused, &sender),
            Ok(Message::WatchWake(generation)) => runtime.watcher_wake(generation, &sender),
            Ok(Message::WatchFailed(generation, error)) => {
                if runtime.active && runtime.generation == generation {
                    log::warn!("[auto_refresh] filesystem watcher failed: {error}");
                    runtime.watcher = None;
                    runtime.fallback = true;
                    runtime.first_change = None;
                    runtime.quiet_deadline = None;
                    runtime.reset_poll_deadlines(Instant::now());
                }
            }
            Ok(Message::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => runtime.run_due(),
        }
    }
}

fn install_watchers(
    target: &RefreshTarget,
    generation: u64,
    sender: &Sender<Message>,
) -> notify::Result<ActiveWatch> {
    let root = std::fs::canonicalize(target.repo.path())
        .map_err(|error| notify::Error::generic(&error.to_string()))?;
    let metadata = git_metadata_paths(&target.repo)
        .map_err(|error| notify::Error::generic(&error.to_string()))?;
    let signal = Arc::new(WatchSignal::default());
    let callback_signal = signal.clone();
    let callback_sender = sender.clone();
    let worktree_root = root.clone();
    let metadata_roots = metadata.clone();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) => {
                let changes = event.paths.iter().fold(0, |changes, path| {
                    let worktree = is_worktree_change(&worktree_root, path);
                    let metadata = is_relevant_metadata_change(&metadata_roots, path);
                    changes
                        | if worktree { WORKTREE_CHANGE } else { 0 }
                        | if metadata { METADATA_CHANGE } else { 0 }
                });
                signal_change(&callback_signal, changes, generation, &callback_sender);
            }
            Err(error) => {
                let _ = callback_sender.send(Message::WatchFailed(generation, error.to_string()));
            }
        })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    let mut watched = HashSet::new();
    for git_dir in metadata {
        if !git_dir.starts_with(&root) && git_dir.is_dir() && watched.insert(git_dir.clone()) {
            watcher.watch(&git_dir, RecursiveMode::NonRecursive)?;
        }
        let refs = git_dir.join("refs");
        if !refs.starts_with(&root) && refs.is_dir() && watched.insert(refs.clone()) {
            watcher.watch(&refs, RecursiveMode::Recursive)?;
        }
    }

    Ok(ActiveWatch {
        generation,
        signal,
        _watcher: watcher,
    })
}

fn signal_change(signal: &WatchSignal, change: u8, generation: u64, sender: &Sender<Message>) {
    if change == 0 {
        return;
    }
    signal.dirty.fetch_or(change, Ordering::AcqRel);
    if !signal.wake_pending.swap(true, Ordering::AcqRel) {
        let _ = sender.send(Message::WatchWake(generation));
    }
}

fn git_metadata_paths(repo: &GitRepo) -> std::io::Result<Vec<PathBuf>> {
    let output = repo
        .command()
        .args([
            "-C",
            repo.path(),
            "rev-parse",
            "--git-dir",
            "--git-common-dir",
        ])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let root = std::fs::canonicalize(repo.path())?;
    let paths = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            let path = PathBuf::from(line.trim());
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            std::fs::canonicalize(&path).unwrap_or(path)
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(std::io::Error::other("git returned no metadata path"));
    }
    Ok(paths)
}

fn is_worktree_change(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let mut components = relative.components();
    let Some(first) = components.next() else {
        return false;
    };
    if first.as_os_str() == ".git" {
        return components.next().is_none();
    }
    true
}

fn is_relevant_metadata_change(metadata_roots: &[PathBuf], path: &Path) -> bool {
    let Some(root) = metadata_roots.iter().find(|root| path.starts_with(root)) else {
        return false;
    };
    let relative = path.strip_prefix(root).unwrap_or(path);
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| component == "objects" || component == "logs")
    {
        return false;
    }
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".lock"))
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use augur_core::git::{FileStatus, GitEvent, spawn_open};
    use std::fs;
    use std::process::Command;
    use std::sync::atomic::AtomicU64;

    static NEXT_REPOSITORY_ID: AtomicU64 = AtomicU64::new(1);

    struct TemporaryRepository(PathBuf);

    impl TemporaryRepository {
        fn new() -> Option<Self> {
            let path = std::env::temp_dir().join(format!(
                "augur-auto-refresh-{}-{}",
                std::process::id(),
                NEXT_REPOSITORY_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).ok()?;
            let repository = Self(path);
            if !repository.git(&["init"]).status.success()
                || !repository
                    .git(&["config", "user.email", "test@example.com"])
                    .status
                    .success()
                || !repository
                    .git(&["config", "user.name", "Test"])
                    .status
                    .success()
            {
                return None;
            }
            fs::write(repository.0.join("tracked.txt"), "initial\n").ok()?;
            if !repository.git(&["add", "tracked.txt"]).status.success()
                || !repository
                    .git(&["commit", "-m", "Initial commit"])
                    .status
                    .success()
            {
                return None;
            }
            Some(repository)
        }

        fn git(&self, args: &[&str]) -> std::process::Output {
            Command::new("git")
                .args(args)
                .current_dir(&self.0)
                .output()
                .expect("git is available for automatic refresh tests")
        }
    }

    impl Drop for TemporaryRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn wait_for_status(
        events: &Receiver<GitEvent>,
        mut matches: impl FnMut(&[FileStatus], &[augur_core::git::BranchInfo]) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "timed out waiting for automatic status refresh"
            );
            match events.recv_timeout(remaining) {
                Ok(GitEvent::Status {
                    files, branches, ..
                }) if matches(&files, &branches) => {
                    return;
                }
                Ok(GitEvent::StatusError(error)) => {
                    panic!("automatic status refresh failed: {}", error.detail);
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {
                    panic!("timed out waiting for automatic status refresh");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("repository worker stopped before automatic status refresh");
                }
            }
        }
    }

    fn drain_events_until_quiet(events: &Receiver<GitEvent>) {
        loop {
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return,
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("repository worker stopped unexpectedly")
                }
            }
        }
    }

    fn assert_no_events(events: &Receiver<GitEvent>) {
        match events.recv_timeout(Duration::from_millis(2500)) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(_) => panic!("automatic refresh ran while it was paused"),
            Err(RecvTimeoutError::Disconnected) => {
                panic!("repository worker stopped unexpectedly")
            }
        }
    }

    #[test]
    fn active_repository_refreshes_external_worktree_and_index_changes() {
        let Some(repository) = TemporaryRepository::new() else {
            return;
        };
        let (event_tx, event_rx) = mpsc::channel();
        let repo = GitRepo::local(repository.0.to_string_lossy().into_owned());
        let handle = spawn_open(repo.clone(), event_tx).expect("temporary repository opens");
        wait_for_status(&event_rx, |_, _| true);
        drain_events_until_quiet(&event_rx);

        let controller = AutoRefreshController::new(true);
        controller.set_target(Some(RefreshTarget {
            id: 1,
            repo,
            handle: handle.clone(),
        }));
        controller.set_focused(true);
        wait_for_status(&event_rx, |_, _| true);
        drain_events_until_quiet(&event_rx);

        fs::write(repository.0.join("external.txt"), "created\n").unwrap();
        wait_for_status(&event_rx, |files, _| {
            files.iter().any(|file| file.path == "external.txt")
        });

        fs::write(repository.0.join("tracked.txt"), "modified\n").unwrap();
        wait_for_status(&event_rx, |files, _| {
            files
                .iter()
                .any(|file| file.path == "tracked.txt" && file.worktree == 'M')
        });

        assert!(repository.git(&["add", "tracked.txt"]).status.success());
        wait_for_status(&event_rx, |files, _| {
            files
                .iter()
                .any(|file| file.path == "tracked.txt" && file.index == 'M' && file.worktree == ' ')
        });

        fs::remove_file(repository.0.join("tracked.txt")).unwrap();
        wait_for_status(&event_rx, |files, _| {
            files
                .iter()
                .any(|file| file.path == "tracked.txt" && file.worktree == 'D')
        });

        assert!(
            repository
                .git(&["branch", "external-branch"])
                .status
                .success()
        );
        wait_for_status(&event_rx, |_, branches| {
            branches
                .iter()
                .any(|branch| branch.name == "external-branch")
        });

        drain_events_until_quiet(&event_rx);
        controller.set_focused(false);
        drain_events_until_quiet(&event_rx);
        fs::write(repository.0.join("created-while-unfocused.txt"), "paused\n").unwrap();
        assert_no_events(&event_rx);

        controller.set_focused(true);
        wait_for_status(&event_rx, |files, _| {
            files
                .iter()
                .any(|file| file.path == "created-while-unfocused.txt")
        });
        drain_events_until_quiet(&event_rx);
        controller.set_enabled(false);
        drain_events_until_quiet(&event_rx);
        fs::write(repository.0.join("created-while-disabled.txt"), "paused\n").unwrap();
        assert_no_events(&event_rx);

        controller.set_enabled(true);
        wait_for_status(&event_rx, |files, _| {
            files
                .iter()
                .any(|file| file.path == "created-while-disabled.txt")
        });

        controller.shutdown();
        handle.close();
    }

    #[test]
    fn linked_worktree_git_file_and_common_refs_are_watched() {
        let Some(repository) = TemporaryRepository::new() else {
            return;
        };
        let linked_path = repository.0.join("linked-worktree");
        assert!(
            repository
                .git(&[
                    "worktree",
                    "add",
                    "--detach",
                    linked_path.to_str().unwrap(),
                    "HEAD"
                ])
                .status
                .success()
        );
        assert!(linked_path.join(".git").is_file());

        let (event_tx, event_rx) = mpsc::channel();
        let repo = GitRepo::local(linked_path.to_string_lossy().into_owned());
        let handle = spawn_open(repo.clone(), event_tx).expect("linked worktree opens");
        let controller = AutoRefreshController::new(true);
        controller.set_target(Some(RefreshTarget {
            id: 2,
            repo,
            handle: handle.clone(),
        }));
        controller.set_focused(true);

        fs::write(linked_path.join("tracked.txt"), "linked modification\n").unwrap();
        wait_for_status(&event_rx, |files, _| {
            files
                .iter()
                .any(|file| file.path == "tracked.txt" && file.worktree == 'M')
        });

        assert!(
            repository
                .git(&["branch", "external-linked-branch"])
                .status
                .success()
        );
        wait_for_status(&event_rx, |_, branches| {
            branches
                .iter()
                .any(|branch| branch.name == "external-linked-branch")
        });

        controller.shutdown();
        handle.close();
    }

    #[test]
    fn metadata_filter_ignores_object_log_and_lock_writes() {
        let root = PathBuf::from("/repo/.git");
        let roots = [root.clone()];
        assert!(is_relevant_metadata_change(&roots, &root.join("index")));
        assert!(is_relevant_metadata_change(
            &roots,
            &root.join("refs/heads/main")
        ));
        assert!(!is_relevant_metadata_change(
            &roots,
            &root.join("objects/ab/cdef")
        ));
        assert!(!is_relevant_metadata_change(
            &roots,
            &root.join("logs/HEAD")
        ));
        assert!(!is_relevant_metadata_change(
            &roots,
            &root.join("index.lock")
        ));
    }

    #[test]
    fn fallback_status_scans_do_not_postpone_full_refreshes() {
        let start = Instant::now();
        let mut runtime = Runtime::new(true);
        runtime.fallback = true;
        runtime.reset_poll_deadlines(start);
        let full_deadline = start + FALLBACK_FULL_INTERVAL;

        runtime.schedule_after_refresh(false, start + FALLBACK_STATUS_INTERVAL);
        assert_eq!(runtime.next_full_poll, Some(full_deadline));
        assert_eq!(
            runtime.next_status_poll,
            Some(start + FALLBACK_STATUS_INTERVAL * 2)
        );

        runtime.schedule_after_refresh(
            false,
            start + FALLBACK_FULL_INTERVAL - Duration::from_secs(1),
        );
        assert_eq!(
            runtime.next_full_poll,
            Some(start + FALLBACK_FULL_INTERVAL + MIN_REFRESH_INTERVAL - Duration::from_secs(1))
        );

        runtime.schedule_after_refresh(true, full_deadline);
        assert_eq!(
            runtime.next_full_poll,
            Some(full_deadline + FALLBACK_FULL_INTERVAL)
        );
    }
}
