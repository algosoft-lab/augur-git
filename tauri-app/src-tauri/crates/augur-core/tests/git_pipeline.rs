//! End-to-end checks against a real Git repository.
//!
//! The unit tests inside the crate cover the parsers with recorded output, which
//! is fast and precise but cannot notice that the command being run no longer
//! produces the shape the parser expects. These tests create a throwaway
//! repository, drive the real `git` binary through the real worker, and assert
//! on the events that reach the interface.
//!
//! Nothing here reaches into a private parser: everything goes through
//! `spawn_open` and the handle, which is exactly the path the application uses.
//! That is deliberate, because a test of the parser alone would not catch a
//! wrong argument vector.
//!
//! Every test returns early when `git` is not on the path, so the suite still
//! runs in an environment that has none.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use augur_core::diff::{DiffDocument, FileChangeStatus};
use augur_core::git::{
    CompareRevision, CompareRevisionKind, FileStatus, GitEvent, GitHandle, GitRepo, LogScope,
    WorkingTreeDiffKind,
};
use augur_core::graph::compute_graph;

/// How long a test waits for one event before giving up.
const PATIENCE: Duration = Duration::from_secs(20);

/// A throwaway repository that removes itself.
struct Sandbox {
    path: PathBuf,
    events: Receiver<GitEvent>,
    handle: Option<GitHandle>,
}

impl Sandbox {
    /// Open a repository with one commit, a branch, and a second branch.
    ///
    /// Returns `None` when `git` is unavailable or refuses the setup, which is
    /// the caller's cue to skip.
    fn new() -> Option<Self> {
        let path = std::env::temp_dir().join(format!(
            "augur-pipeline-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).ok()?;
        let sandbox = Sandbox {
            path,
            events: std::sync::mpsc::channel().1,
            handle: None,
        };
        sandbox.git(&["init", "--initial-branch=main"])?;
        sandbox.git(&["config", "user.email", "test@example.com"])?;
        sandbox.git(&["config", "user.name", "Test"])?;
        // A commit would otherwise fail on a machine with no identity and a
        // default branch that does not exist.
        sandbox.git(&["config", "commit.gpgsign", "false"])?;
        sandbox.write("src/main.rs", "fn main() {\n    let count = 0;\n}\n")?;
        sandbox.write("README.md", "# Title\n\nBody.\n")?;
        sandbox.git(&["add", "."])?;
        sandbox.git(&["commit", "-m", "Add the project"])?;
        sandbox.git(&["branch", "topic"])?;
        sandbox.git(&["tag", "v1.0.0"])?;
        Some(sandbox)
    }

    /// Start the real worker against this repository.
    fn open(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.events = rx;
        self.handle = Some(
            augur_core::git::spawn_open(
                GitRepo::local(self.path.to_string_lossy().into_owned()),
                tx,
            )
            .expect("a repository on disk opens"),
        );
    }

    fn handle(&self) -> &GitHandle {
        self.handle.as_ref().expect("the worker is running")
    }

    /// Run a command, returning its standard output on success.
    fn git(&self, args: &[&str]) -> Option<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            // An ambient configuration must not change what is being tested.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn git_ok(&self, args: &[&str]) -> String {
        self.git(args)
            .unwrap_or_else(|| panic!("git {args:?} failed in {}", self.path.display()))
    }

    fn write(&self, relative: &str, contents: &str) -> Option<()> {
        let target = self.path.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        std::fs::write(target, contents).ok()
    }

    /// Wait for the first event the predicate accepts, skipping the others.
    fn wait_for(&self, accept: impl Fn(&GitEvent) -> bool) -> Option<GitEvent> {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(remaining) {
                Ok(event) if accept(&event) => return Some(event),
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                    return None;
                }
            }
        }
        None
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.close();
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A distinct suffix per sandbox, so parallel tests do not collide.
fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Skip the test body when there is no `git`.
macro_rules! require_git {
    () => {
        if !git_available() {
            return;
        }
    };
}

#[test]
fn a_refresh_reports_the_real_repository() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox
        .write("src/main.rs", "fn main() {\n    let count = 1;\n}\n")
        .unwrap();
    sandbox.write("notes.md", "notes\n").unwrap();
    sandbox.open();

    sandbox.handle().refresh();

    let status = sandbox
        .wait_for(|event| matches!(event, GitEvent::Status { .. }))
        .expect("a status snapshot");
    let GitEvent::Status {
        branch,
        head,
        upstream,
        files,
        branches,
        ahead,
        behind,
    } = status
    else {
        unreachable!()
    };

    assert_eq!(branch, "main");
    assert!(head.is_some(), "the branch has a commit");
    assert_eq!(upstream, None, "a local branch tracks nothing");
    assert_eq!(ahead, 0);
    assert_eq!(behind, 0);

    let modified = files
        .iter()
        .find(|file: &&FileStatus| file.path == "src/main.rs")
        .expect("the modified file");
    assert!(modified.has_worktree_changes());
    assert!(!modified.has_staged_changes());

    let untracked = files
        .iter()
        .find(|file: &&FileStatus| file.path == "notes.md")
        .expect("the untracked file");
    assert!(untracked.is_untracked());

    let main = branches
        .iter()
        .find(|entry| entry.name == "main")
        .expect("the checked-out branch is listed");
    assert!(main.is_head, "the checked-out branch is marked");
    assert!(
        branches.iter().any(|entry| entry.name == "topic"),
        "the second branch is listed"
    );
}

#[test]
fn a_log_query_produces_a_layable_graph() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox.write("second.txt", "second\n").unwrap();
    sandbox.git_ok(&["add", "second.txt"]);
    sandbox.git_ok(&["commit", "-m", "Add a second file\n\nWith a body."]);
    sandbox.open();

    sandbox.handle().log_query(LogScope::AllBranches);

    let page = sandbox
        .wait_for(|event| matches!(event, GitEvent::LogPage { .. }))
        .expect("a log page");
    let GitEvent::LogPage {
        rows,
        replace,
        has_more,
    } = page
    else {
        unreachable!()
    };

    assert!(replace, "the first page replaces rather than appends");
    assert!(!has_more, "two commits fit in one page");
    assert_eq!(rows.len(), 2);

    assert_eq!(rows[0].subject, "Add a second file");
    assert!(
        rows[0].message.contains("With a body."),
        "the body survives: {}",
        rows[0].message
    );
    assert_eq!(rows[0].parents.len(), 1);
    assert!(
        rows[0].decorations.contains("main"),
        "the branch is in the decoration: {}",
        rows[0].decorations
    );
    assert_eq!(rows[1].subject, "Add the project");
    assert!(rows[1].parents.is_empty(), "the first commit has no parent");

    // The layout the viewer draws has to be derivable from those rows, which is
    // the one thing the frontend depends on.
    let graph = compute_graph(&rows);
    assert_eq!(graph.len(), rows.len());
    assert!(graph[0].is_head, "the newest commit is the head");
    for row in &graph {
        assert!(row.lane_count >= 1);
    }
}

#[test]
fn selecting_a_commit_yields_its_files_and_their_diff() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox
        .write("src/main.rs", "fn main() {\n    let count = 1;\n}\n")
        .unwrap();
    sandbox.write("extra.txt", "extra\n").unwrap();
    sandbox.git_ok(&["add", "."]);
    sandbox.git_ok(&["commit", "-m", "Change two files"]);
    let oid = sandbox.git_ok(&["rev-parse", "HEAD"]);
    let oid = oid.trim().to_string();
    sandbox.open();

    // File metadata first, then the diff of the chosen file: the same order the
    // interface asks for them in.
    sandbox.handle().commit_numstat(42, oid.clone());
    let files = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommitFiles { request_id: 42, .. }))
        .expect("the file list");
    let GitEvent::CommitFiles {
        request_id, files, ..
    } = files
    else {
        unreachable!()
    };
    assert_eq!(request_id, 42);
    assert_eq!(files.len(), 2, "two files changed");

    let main = files
        .iter()
        .find(|file| file.path == "src/main.rs")
        .expect("the modified file");
    assert_eq!(main.status, FileChangeStatus::Modified);
    assert_eq!(main.added, Some(1));
    assert_eq!(main.deleted, Some(1));

    let extra = files
        .iter()
        .find(|file| file.path == "extra.txt")
        .expect("the added file");
    assert_eq!(extra.status, FileChangeStatus::Added);
    assert_eq!(extra.added, Some(1));
    assert_eq!(extra.deleted, Some(0));

    sandbox
        .handle()
        .commit_file_diff(42, oid, None, main.clone());
    let diff = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommitFileDiff { .. }))
        .expect("the file diff");
    let GitEvent::CommitFileDiff {
        request_id,
        patch,
        old_source,
        new_source,
        ..
    } = diff
    else {
        unreachable!()
    };
    assert_eq!(request_id, 42);
    let document =
        DiffDocument::from_patch("src/main.rs".to_string(), &patch, old_source, new_source);

    assert!(!document.binary);
    assert_eq!(document.language.as_deref(), Some("rust"));
    assert!(
        document.rows.iter().any(|row| row
            .new_text
            .as_deref()
            .is_some_and(|text| text.contains("let count = 1;"))),
        "the new line is present"
    );
    assert!(!document.aligned_rows().is_empty());
    // The clipboard form is rebuilt from the parsed rows rather than copied from
    // the patch, so it carries the change itself.
    let copied = document.copy_text();
    assert!(copied.contains("@@"), "a hunk header: {copied}");
    assert!(
        copied.contains("+    let count = 1;"),
        "the addition: {copied}"
    );
    assert!(
        copied.contains("-    let count = 0;"),
        "the deletion: {copied}"
    );
}

#[test]
fn commit_diff_failures_are_request_specific_and_do_not_stop_the_worker() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox
        .write("src/main.rs", "fn main() {\n    let count = 1;\n}\n")
        .unwrap();
    sandbox.git_ok(&["add", "."]);
    sandbox.git_ok(&["commit", "-m", "Change main"]);
    let oid = sandbox.git_ok(&["rev-parse", "HEAD"]).trim().to_string();
    let missing_oid = "0000000000000000000000000000000000000000";
    sandbox.open();

    sandbox.handle().commit_numstat(51, missing_oid.to_string());
    let error = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommitFilesError { request_id: 51, .. }))
        .expect("the file-list failure");
    let GitEvent::CommitFilesError {
        request_id, error, ..
    } = error
    else {
        unreachable!()
    };
    assert_eq!(request_id, 51);
    assert_eq!(error.key, "err-numstat");

    sandbox.handle().commit_numstat(52, oid.clone());
    let files = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommitFiles { request_id: 52, .. }))
        .expect("the valid file list after the failure");
    let GitEvent::CommitFiles { files, .. } = files else {
        unreachable!()
    };
    let main = files
        .into_iter()
        .find(|file| file.new_path == "src/main.rs")
        .expect("the modified file");

    sandbox
        .handle()
        .commit_file_diff(52, missing_oid.to_string(), None, main);
    let error = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommitFileDiffError { request_id: 52, .. }))
        .expect("the file-diff failure");
    let GitEvent::CommitFileDiffError {
        request_id, error, ..
    } = error
    else {
        unreachable!()
    };
    assert_eq!(request_id, 52);
    assert_eq!(error.key, "err-file-diff");

    sandbox.handle().commit_numstat(53, oid);
    assert!(
        sandbox
            .wait_for(|event| matches!(event, GitEvent::CommitFiles { request_id: 53, .. }))
            .is_some()
    );
}

#[test]
fn a_new_commit_selection_skips_queued_diffs_from_the_previous_selection() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    let oid = sandbox.git_ok(&["rev-parse", "HEAD"]).trim().to_string();
    sandbox.open();

    sandbox.handle().commit_numstat(61, oid.clone());
    let files = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommitFiles { request_id: 61, .. }))
        .expect("the initial file list");
    let GitEvent::CommitFiles { files, .. } = files else {
        unreachable!()
    };
    let file = files.into_iter().next().expect("a changed file");

    for _ in 0..256 {
        sandbox
            .handle()
            .commit_file_diff(61, oid.clone(), None, file.clone());
    }
    sandbox.handle().commit_numstat(62, oid);

    let files = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommitFiles { request_id: 62, .. }))
        .expect("the newer file list");
    assert!(matches!(
        files,
        GitEvent::CommitFiles { request_id: 62, .. }
    ));
    while let Ok(event) = sandbox.events.try_recv() {
        assert!(!matches!(
            event,
            GitEvent::CommitFileDiff { request_id: 61, .. }
                | GitEvent::CommitFileDiffError { request_id: 61, .. }
        ));
    }
}

#[test]
fn a_working_tree_file_diff_names_the_side_it_read() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox
        .write("src/main.rs", "fn main() {\n    let count = 7;\n}\n")
        .unwrap();
    let file = FileStatus {
        index: ' ',
        worktree: 'M',
        path: "src/main.rs".to_string(),
        old_path: None,
    };
    sandbox.open();

    sandbox
        .handle()
        .working_tree_file_diff(1, WorkingTreeDiffKind::Unstaged, file);

    let diff = sandbox
        .wait_for(|event| matches!(event, GitEvent::WorkingTreeFileDiff { .. }))
        .expect("the working-tree diff");
    let GitEvent::WorkingTreeFileDiff {
        request_id,
        kind,
        patch,
        old_source,
        new_source,
        ..
    } = diff
    else {
        unreachable!()
    };
    let document =
        DiffDocument::from_patch("src/main.rs".to_string(), &patch, old_source, new_source);
    assert_eq!(request_id, 1, "the request id is echoed back");
    assert!(matches!(kind, WorkingTreeDiffKind::Unstaged));
    assert!(
        document.rows.iter().any(|row| row
            .new_text
            .as_deref()
            .is_some_and(|text| text.contains("let count = 7;"))),
        "the working-tree content is what is shown"
    );
}

#[test]
fn sustained_automatic_refresh_does_not_starve_a_working_tree_diff() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox
        .write("src/main.rs", "fn main() {\n    let count = 7;\n}\n")
        .unwrap();
    sandbox.open();
    sandbox
        .wait_for(|event| matches!(event, GitEvent::Status { .. }))
        .expect("the initial status snapshot");

    let handle = sandbox.handle().clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let refresh_handle = handle.clone();
    let refresh_stop = stop.clone();
    let refresher = std::thread::spawn(move || {
        while !refresh_stop.load(std::sync::atomic::Ordering::Relaxed) {
            refresh_handle.refresh_automatically(false);
            std::thread::yield_now();
        }
    });
    sandbox
        .wait_for(|event| matches!(event, GitEvent::Status { .. }))
        .expect("an automatic status refresh while the refresher is active");

    handle.working_tree_file_diff(
        42,
        WorkingTreeDiffKind::Unstaged,
        FileStatus {
            index: ' ',
            worktree: 'M',
            path: "src/main.rs".to_string(),
            old_path: None,
        },
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut request_id = None;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match sandbox.events.recv_timeout(remaining) {
            Ok(GitEvent::WorkingTreeFileDiff { request_id: id, .. }) => {
                request_id = Some(id);
                break;
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    refresher.join().expect("refresh request thread exits");
    assert_eq!(
        request_id,
        Some(42),
        "the queued diff is eventually delivered"
    );
}

#[test]
fn refs_report_branches_tags_and_an_empty_stash_list() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox.open();

    sandbox.handle().refresh();

    let refs = sandbox
        .wait_for(|event| matches!(event, GitEvent::Refs(_)))
        .expect("a refs snapshot");
    let GitEvent::Refs(refs) = refs else {
        unreachable!()
    };

    assert!(
        refs.tags.iter().any(|tag| tag == "v1.0.0"),
        "the tag is listed: {:?}",
        refs.tags
    );
    assert!(refs.stashes.is_empty(), "nothing has been stashed");
    assert!(
        refs.comparison_revisions
            .iter()
            .any(|revision| revision.name == "main" && revision.kind == CompareRevisionKind::Local),
        "the branch is offered for comparison: {:?}",
        refs.comparison_revisions
    );
}

#[test]
fn a_stash_appears_in_the_refs_snapshot() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox
        .write("src/main.rs", "fn main() {\n    /* stashed */\n}\n")
        .unwrap();
    sandbox.git_ok(&["stash", "push", "-m", "work in progress"]);
    sandbox.open();

    sandbox.handle().refresh();

    let refs = sandbox
        .wait_for(|event| matches!(event, GitEvent::Refs(_)))
        .expect("a refs snapshot");
    let GitEvent::Refs(refs) = refs else {
        unreachable!()
    };

    assert_eq!(refs.stashes.len(), 1, "one stash: {:?}", refs.stashes);
    assert_eq!(refs.stashes[0].reference, "stash@{0}");
    assert!(
        refs.stashes[0].description.contains("work in progress"),
        "the message survives: {}",
        refs.stashes[0].description
    );
}

#[test]
fn comparing_two_revisions_lists_only_what_differs() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox.write("changed.txt", "one\n").unwrap();
    // Committed up front, so it is identical on both sides of the comparison and
    // must not be reported.
    sandbox.write("untouched.txt", "same\n").unwrap();
    sandbox.git_ok(&["add", "."]);
    sandbox.git_ok(&["commit", "-m", "First"]);
    let base = sandbox.git_ok(&["rev-parse", "HEAD"]);
    sandbox.write("changed.txt", "two\n").unwrap();
    sandbox.git_ok(&["add", "."]);
    sandbox.git_ok(&["commit", "-m", "Second"]);
    let target = sandbox.git_ok(&["rev-parse", "HEAD"]);
    sandbox.open();

    let base = CompareRevision::from_commit_id(base.trim()).expect("a commit id");
    let target = CompareRevision::from_commit_id(target.trim()).expect("a commit id");
    sandbox.handle().branch_compare(7, base, target);

    let files = sandbox
        .wait_for(|event| matches!(event, GitEvent::BranchCompareFiles { .. }))
        .expect("the comparison file list");
    let GitEvent::BranchCompareFiles { request_id, files } = files else {
        unreachable!()
    };
    assert_eq!(request_id, 7, "the request id is echoed back");
    assert_eq!(files.len(), 1, "only the changed file: {files:?}");
    assert_eq!(files[0].path, "changed.txt");

    let finished = sandbox
        .wait_for(|event| matches!(event, GitEvent::BranchCompareFinished { .. }))
        .expect("the comparison finishing");
    assert!(matches!(
        finished,
        GitEvent::BranchCompareFinished { request_id: 7 }
    ));
}

#[test]
fn a_command_reports_its_label_and_its_failure() {
    require_git!();
    let mut sandbox = Sandbox::new().expect("sandbox");
    sandbox.open();

    // A label the interface displays, and arguments that cannot succeed.
    sandbox
        .handle()
        .run("checkout", vec!["no-such-branch".to_string()]);

    let done = sandbox
        .wait_for(|event| matches!(event, GitEvent::CommandDone { success: false, .. }))
        .expect("the command finishing");
    let GitEvent::CommandDone { label, message, .. } = done else {
        unreachable!()
    };
    assert_eq!(label, "checkout", "the label is the one the caller chose");
    assert!(
        !message.is_empty(),
        "a failure carries Git's own explanation"
    );
}

#[test]
fn a_probe_reports_a_clean_tree() {
    require_git!();
    let sandbox = Sandbox::new().expect("sandbox");

    let head = sandbox.git_ok(&["rev-parse", "HEAD"]);
    let repo = GitRepo::local(sandbox.path.to_string_lossy().into_owned());
    let probe = augur_core::git::operation_probe::probe_merge_state(&repo)
        .expect("a probe on a clean repository");

    assert_eq!(probe.head.as_deref(), Some(head.trim()));
    assert!(probe.merge_head.is_none(), "no merge in progress");
    assert!(!probe.rebase_in_progress);
    assert!(!probe.has_changes);
    assert!(!probe.has_conflicts);
}

#[test]
fn opening_a_directory_that_is_not_a_repository_fails_immediately() {
    require_git!();
    let path = std::env::temp_dir().join(format!("augur-not-a-repo-{}", unique()));
    std::fs::create_dir_all(&path).unwrap();

    let (tx, _rx): (Sender<GitEvent>, Receiver<GitEvent>) = std::sync::mpsc::channel();
    let result =
        augur_core::git::spawn_open(GitRepo::local(path.to_string_lossy().into_owned()), tx);

    let error = result.err().expect("a plain directory is not a repository");
    assert_eq!(error.key, "err-not-a-repo");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn opening_a_path_that_does_not_exist_fails_immediately() {
    require_git!();
    let (tx, _rx): (Sender<GitEvent>, Receiver<GitEvent>) = std::sync::mpsc::channel();
    let missing = std::env::temp_dir().join(format!("augur-missing-{}", unique()));
    let result =
        augur_core::git::spawn_open(GitRepo::local(missing.to_string_lossy().into_owned()), tx);

    let error = result.err().expect("a missing path is refused");
    assert_eq!(error.key, "err-path-not-exist");
}
