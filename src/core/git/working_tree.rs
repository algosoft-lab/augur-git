//! Working-tree Git operations and path-safe pathspec helpers.

use std::fs;
use std::io::Write;
use std::path::{Component, Path};
use std::process::Stdio;
use std::sync::mpsc::Sender;

use crate::core::diff::is_binary_patch;

use super::{
    FileStatus, GitEvent, GitRepo, MAX_BLOB_SIZE, RepoLocation,
    WorkingTreeAction, WorkingTreeDiffKind, WorkingTreeScope, read_blob_spec,
};

/// Build the regular working-tree diff command for one status entry.
pub(super) fn working_tree_diff_args(
    repo_path: &str,
    kind: WorkingTreeDiffKind,
    file: &FileStatus,
) -> Vec<String> {
    let mut args = vec![
        "--literal-pathspecs".to_string(),
        "--no-pager".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
    ];
    if matches!(kind, WorkingTreeDiffKind::Staged) {
        args.push("--cached".to_string());
    }
    args.push("--".to_string());
    if let Some(old_path) = file.old_path.as_deref() {
        args.push(old_path.to_string());
    }
    args.push(file.path.clone());
    args
}

/// Build the synthetic diff command used to render an untracked file.
pub(super) fn untracked_diff_args(repo_path: &str, path: &str) -> Vec<String> {
    vec![
        "--literal-pathspecs".to_string(),
        "--no-pager".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff".to_string(),
        "--no-index".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--".to_string(),
        "/dev/null".to_string(),
        path.to_string(),
    ]
}

/// Query a staged or unstaged working-tree file without blocking the UI.
pub(super) fn run_file_diff(
    repo: &GitRepo,
    request_id: u64,
    kind: WorkingTreeDiffKind,
    file: &FileStatus,
    event_tx: &Sender<GitEvent>,
) {
    let untracked =
        matches!(kind, WorkingTreeDiffKind::Unstaged) && file.is_untracked();
    let args = if untracked {
        untracked_diff_args(repo.path(), &file.path)
    } else {
        working_tree_diff_args(repo.path(), kind, file)
    };
    let output = repo.command().args(&args).output();
    match output {
        Ok(output)
            if output.status.success()
                || (untracked && output.status.code() == Some(1)) =>
        {
            let patch = String::from_utf8_lossy(&output.stdout).into_owned();
            let (old_source, new_source) = match kind {
                WorkingTreeDiffKind::Staged => {
                    let old_path =
                        file.old_path.as_deref().unwrap_or(&file.path);
                    let old_source = if file.index == 'A' {
                        None
                    } else {
                        read_blob_spec(repo, &format!("HEAD:{old_path}"))
                    };
                    let new_source = if file.index == 'D' {
                        None
                    } else {
                        read_blob_spec(repo, &format!(":{}", file.path))
                    };
                    (old_source, new_source)
                }
                WorkingTreeDiffKind::Unstaged => {
                    let old_path = if file.index == ' ' {
                        file.old_path.as_deref().unwrap_or(&file.path)
                    } else {
                        &file.path
                    };
                    let old_source = if untracked || file.worktree == 'A' {
                        None
                    } else {
                        read_blob_spec(repo, &format!(":{old_path}"))
                    };
                    let new_source = if file.worktree == 'D' {
                        None
                    } else {
                        read_worktree_source(repo, &file.path)
                    };
                    (old_source, new_source)
                }
            };
            log::debug!(
                "[git_diff] loaded working-tree file diff: request_id={}, kind={kind:?}, binary={}, patch_bytes={}",
                request_id,
                is_binary_patch(&patch),
                patch.len()
            );
            let _ = event_tx.send(GitEvent::WorkingTreeFileDiff {
                request_id,
                kind,
                file: file.clone(),
                patch,
                old_source,
                new_source,
            });
        }
        Ok(output) => {
            let detail = diff_error_detail(&output);
            log::warn!(
                "[git_diff] working-tree file diff failed: request_id={}, kind={kind:?}, status={:?}",
                request_id,
                output.status.code()
            );
            let _ = event_tx.send(GitEvent::WorkingTreeFileDiffError {
                request_id,
                kind,
                file: file.clone(),
                detail,
            });
        }
        Err(error) => {
            log::warn!(
                "[git_diff] working-tree file diff failed to run: request_id={}, kind={kind:?}",
                request_id
            );
            let _ = event_tx.send(GitEvent::WorkingTreeFileDiffError {
                request_id,
                kind,
                file: file.clone(),
                detail: error.to_string(),
            });
        }
    }
}

fn diff_error_detail(output: &std::process::Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        output
            .status
            .code()
            .map(|code| format!("git diff exited with status {code}"))
            .unwrap_or_else(|| "git diff terminated unexpectedly".to_string())
    } else {
        detail
    }
}

/// Read the current content of a working-tree file for the diff side panel.
/// The path comes from a status snapshot and is validated as repo-relative
/// before any access. Reading is best-effort: unreadable, oversized, or
/// non-UTF-8 content degrades to `None`, exactly as before.
fn read_worktree_source(repo: &GitRepo, path: &str) -> Option<String> {
    let relative_path = Path::new(path);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let bytes = match repo.location() {
        RepoLocation::Local => {
            fs::read(Path::new(repo.path()).join(relative_path)).ok()?
        }
        RepoLocation::Wsl { .. } => read_remote_worktree_bytes(repo, path)?,
    };
    if bytes.len() > MAX_BLOB_SIZE {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Read one untracked working-tree file from a non-local repository through
/// the location itself (`cat -- <path>` inside the distro). Paths containing
/// control characters are skipped: the preview is best-effort and such paths
/// would be awkward to present anyway.
fn read_remote_worktree_bytes(repo: &GitRepo, path: &str) -> Option<Vec<u8>> {
    if path.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
        log::debug!(
            "[git_diff] skipping untracked preview: path contains control characters"
        );
        return None;
    }
    let output = repo
        .command_in_location("cat")
        .args(["--", path])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(output.stdout)
}

/// Execute one staged/working-tree operation against the captured status
/// snapshot. The caller is responsible for emitting the completion event and
/// refreshing the repository status afterwards.
pub(super) fn apply_operation(
    repo: &GitRepo,
    action: WorkingTreeAction,
    scope: &WorkingTreeScope,
) -> Result<(), String> {
    let files = scope.files();
    for file in &files {
        validate_status_path(&file.path)?;
        if let Some(old_path) = file.old_path.as_deref() {
            validate_status_path(old_path)?;
        }
        if file.is_conflicted() {
            return Err(format!(
                "cannot {} conflicted file",
                action.description()
            ));
        }
    }

    if files.is_empty() {
        return Ok(());
    }

    log::info!(
        "[git_worktree] applying action={}, scope={:?}, files={}, repo={}",
        action.description(),
        scope.kind(),
        files.len(),
        repo.label()
    );

    match action {
        WorkingTreeAction::Stage => stage(repo, &files),
        WorkingTreeAction::Unstage => unstage(repo, &files),
        WorkingTreeAction::Discard => discard(repo, &files),
    }
}

fn stage(repo: &GitRepo, files: &[&FileStatus]) -> Result<(), String> {
    let paths = operation_paths(files, true);
    run_pathspec_command(
        repo,
        &[
            "add",
            "--all",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ],
        &paths,
    )
}

fn unstage(repo: &GitRepo, files: &[&FileStatus]) -> Result<(), String> {
    let paths = operation_paths(files, true);
    if has_head(repo)? {
        run_pathspec_command(
            repo,
            &[
                "reset",
                "-q",
                "HEAD",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            &paths,
        )
    } else {
        run_pathspec_command(
            repo,
            &[
                "rm",
                "--cached",
                "--quiet",
                "--force",
                "--ignore-unmatch",
                "-r",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            &paths,
        )
    }
}

fn discard(repo: &GitRepo, files: &[&FileStatus]) -> Result<(), String> {
    let mut restore_paths = Vec::new();
    let mut clean_paths = Vec::new();

    for file in files {
        if file.is_untracked() {
            clean_paths.push(file.path.clone());
            continue;
        }

        match file.worktree {
            // An unstaged rename/copy leaves the old tracked path in the
            // index and exposes the new path as a worktree path.
            'R' => {
                if let Some(old_path) = &file.old_path {
                    restore_paths.push(old_path.clone());
                }
                clean_paths.push(file.path.clone());
            }
            'C' => clean_paths.push(file.path.clone()),
            _ => restore_paths.push(file.path.clone()),
        }
    }

    if !restore_paths.is_empty() {
        run_pathspec_command(
            repo,
            &[
                "checkout",
                "--quiet",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            &deduplicate(restore_paths),
        )?;
    }

    for chunk in clean_paths.chunks(MAX_CLEAN_PATHS_PER_COMMAND) {
        run_clean_command(repo, chunk)?;
    }

    Ok(())
}

const MAX_CLEAN_PATHS_PER_COMMAND: usize = 128;

/// Verify that captured untracked paths are still plain files before running
/// the destructive `git clean`. Local repositories stat the path directly; WSL
/// repositories ask Git whether the path has become a directory. Any probe
/// failure refuses the operation: a destructive action must not proceed on a
/// failed safety check.
fn run_clean_command(repo: &GitRepo, paths: &[String]) -> Result<(), String> {
    for path in paths {
        if clean_path_is_directory(repo, path)? {
            return Err(format!(
                "refusing to remove a directory at a captured file path: {path}"
            ));
        }
    }

    let mut command = repo.command();
    command
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(repo.path())
        .arg("clean")
        .arg("-f")
        .arg("-d")
        .arg("--");
    for path in paths {
        command.arg(path);
    }

    let output = command
        .output()
        .map_err(|error| format!("failed to run git clean: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error("git clean", &output))
    }
}

/// Whether a captured path now denotes a directory. A missing path is not a
/// directory; the subsequent clean reports the mismatch like it does locally.
fn clean_path_is_directory(repo: &GitRepo, path: &str) -> Result<bool, String> {
    match repo.location() {
        RepoLocation::Local => {
            let full_path = Path::new(repo.path()).join(path);
            match fs::symlink_metadata(&full_path) {
                Ok(metadata) => Ok(metadata.file_type().is_dir()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(false)
                }
                Err(error) => Err(format!(
                    "failed to inspect captured path before git clean: {error}"
                )),
            }
        }
        RepoLocation::Wsl { .. } => {
            let output = repo
                .command()
                .arg("--literal-pathspecs")
                .args([
                    "-C",
                    repo.path(),
                    "ls-files",
                    "--others",
                    "--directory",
                    "-z",
                    "--",
                    path,
                ])
                .output()
                .map_err(|error| {
                    format!(
                        "failed to probe captured path before git clean: {error}"
                    )
                })?;
            if !output.status.success() {
                return Err(format!(
                    "failed to probe captured path before git clean: {}",
                    command_error("git ls-files", &output)
                ));
            }
            Ok(clean_probe_is_directory(&output.stdout, path))
        }
    }
}

/// Decide from `git ls-files --others --directory -z` output (anchored to one
/// pathspec) whether the captured path denotes a directory. Git reports a
/// collapsed directory with a trailing slash; a plain record is the file
/// itself; no record means the path is gone.
fn clean_probe_is_directory(records: &[u8], path: &str) -> bool {
    let directory_record = format!("{path}/").into_bytes();
    records
        .split(|byte| *byte == 0)
        .any(|record| record == directory_record.as_slice())
}

fn run_pathspec_command(
    repo: &GitRepo,
    args: &[&str],
    paths: &[String],
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut command = repo.command();
    command
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(repo.path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to run git: {error}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("git command did not expose stdin".to_string());
    };
    for path in paths {
        let result = stdin
            .write_all(path.as_bytes())
            .and_then(|_| stdin.write_all(&[0]));
        if let Err(error) = result {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("failed to provide Git paths: {error}"));
        }
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for git: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error("git operation", &output))
    }
}

fn command_error(label: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        output
            .status
            .code()
            .map(|code| format!("{label} exited with status {code}"))
            .unwrap_or_else(|| format!("{label} terminated unexpectedly"))
    } else {
        stderr
    }
}

fn operation_paths(
    files: &[&FileStatus],
    include_old_paths: bool,
) -> Vec<String> {
    let mut paths = Vec::new();
    for file in files {
        if include_old_paths {
            if let Some(old_path) = &file.old_path {
                paths.push(old_path.clone());
            }
        }
        paths.push(file.path.clone());
    }
    deduplicate(paths)
}

fn deduplicate(paths: Vec<String>) -> Vec<String> {
    let mut unique = Vec::with_capacity(paths.len());
    for path in paths {
        if !unique.iter().any(|existing| existing == &path) {
            unique.push(path);
        }
    }
    unique
}

fn validate_status_path(path: &str) -> Result<(), String> {
    let relative = Path::new(path);
    if path.is_empty() || relative.is_absolute() {
        return Err(
            "Git reported an invalid absolute or empty path".to_string()
        );
    }
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("Git reported an unsafe path: {path}"));
    }
    Ok(())
}

fn has_head(repo: &GitRepo) -> Result<bool, String> {
    let output = repo
        .command()
        .arg("-C")
        .arg(repo.path())
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()
        .map_err(|error| {
            format!("failed to inspect repository HEAD: {error}")
        })?;
    if output.status.success() {
        Ok(true)
    } else if output.stderr.is_empty() {
        // An unborn repository has no HEAD yet. --quiet keeps this expected
        // condition separate from other repository inspection failures.
        Ok(false)
    } else {
        Err(command_error("git rev-parse", &output))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        GitRepo, apply_operation, clean_probe_is_directory, deduplicate,
        validate_status_path,
    };
    use crate::core::git::{FileStatus, WorkingTreeAction, WorkingTreeScope};

    /// Plain git for test fixtures; worker commands go through `GitRepo`.
    fn git_command() -> Command {
        Command::new("git")
    }

    #[test]
    fn deduplicate_preserves_path_order() {
        assert_eq!(
            deduplicate(vec!["a".into(), "b".into(), "a".into()]),
            vec!["a", "b"]
        );
    }

    #[test]
    fn validate_status_path_rejects_escape_paths() {
        assert!(validate_status_path("../outside").is_err());
        assert!(validate_status_path("/outside").is_err());
        assert!(validate_status_path("").is_err());
        assert!(validate_status_path("folder/../outside").is_err());
        assert!(validate_status_path("folder/file.rs").is_ok());
    }

    #[test]
    fn clean_probe_treats_only_the_directory_record_as_a_directory() {
        let records = b"captured/\0other-untracked.txt\0";
        assert!(clean_probe_is_directory(records, "captured"));
        assert!(!clean_probe_is_directory(records, "other-untracked.txt"));
        assert!(!clean_probe_is_directory(records, "missing.txt"));
        assert!(!clean_probe_is_directory(b"", "captured"));
        // A plain record is the file itself, not a directory.
        assert!(!clean_probe_is_directory(b"captured\0", "captured"));
    }

    #[test]
    fn local_clean_check_refuses_directories_at_captured_paths() {
        let repo = TempRepo::new();
        let handle = repo.handle();
        fs::create_dir(repo.path.join("became-a-directory"))
            .expect("test directory");
        assert!(
            super::clean_path_is_directory(&handle, "became-a-directory")
                .expect("probe should succeed")
        );
        fs::write(repo.path.join("still-a-file"), "x\n").expect("test file");
        assert!(
            !super::clean_path_is_directory(&handle, "still-a-file")
                .expect("probe should succeed")
        );
        assert!(
            !super::clean_path_is_directory(&handle, "gone.txt")
                .expect("missing paths are simply not directories")
        );
    }

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(1);
            let root = std::env::temp_dir();
            loop {
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                let path = root.join(format!(
                    "augur-git-working-tree-{}-{id}",
                    std::process::id()
                ));
                if fs::create_dir(&path).is_ok() {
                    let repo = Self { path };
                    repo.git(["init", "-q"]);
                    repo.git(["config", "user.email", "test@example.com"]);
                    repo.git(["config", "user.name", "Test User"]);
                    return repo;
                }
            }
        }

        fn handle(&self) -> GitRepo {
            GitRepo::local(self.path.to_string_lossy().into_owned())
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            let output = git_command()
                .arg("-C")
                .arg(&self.path)
                .args(args)
                .output()
                .expect("git must be available for working-tree tests");
            assert!(
                output.status.success(),
                "git command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn git_status<I, S>(&self, args: I) -> bool
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            git_command()
                .arg("-C")
                .arg(&self.path)
                .args(args)
                .output()
                .expect("git must be available for working-tree tests")
                .status
                .success()
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.path.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .expect("test directory should be writable");
            }
            fs::write(path, contents).expect("test file should be writable");
        }

        fn commit_base(&self) {
            self.write("tracked.txt", "base\n");
            self.git(["add", "."]);
            self.git(["commit", "-q", "-m", "base"]);
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn status(index: char, worktree: char, path: &str) -> FileStatus {
        FileStatus {
            index,
            worktree,
            path: path.to_string(),
            old_path: None,
        }
    }

    #[test]
    fn stage_and_unstage_preserve_worktree_content() {
        let repo = TempRepo::new();
        repo.commit_base();
        repo.write("tracked.txt", "staged\n");
        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Stage,
            &WorkingTreeScope::File(status(' ', 'M', "tracked.txt")),
        )
        .expect("stage should succeed");
        assert!(!repo.git_status(["diff", "--cached", "--quiet"]));
        assert!(repo.git_status(["diff", "--quiet"]));

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Unstage,
            &WorkingTreeScope::File(status('M', ' ', "tracked.txt")),
        )
        .expect("unstage should succeed");
        assert!(repo.git_status(["diff", "--cached", "--quiet"]));
        assert!(!repo.git_status(["diff", "--quiet"]));
    }

    #[test]
    fn discard_mixed_changes_keeps_staged_content() {
        let repo = TempRepo::new();
        repo.commit_base();
        repo.write("tracked.txt", "staged\n");
        repo.git(["add", "tracked.txt"]);
        repo.write("tracked.txt", "staged plus worktree\n");

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Discard,
            &WorkingTreeScope::File(status('M', 'M', "tracked.txt")),
        )
        .expect("discard should succeed");
        assert!(!repo.git_status(["diff", "--cached", "--quiet"]));
        assert!(repo.git_status(["diff", "--quiet"]));
        assert_eq!(
            fs::read_to_string(repo.path.join("tracked.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "staged\n"
        );
    }

    #[test]
    fn untracked_file_can_be_staged_then_discarded() {
        let repo = TempRepo::new();
        repo.commit_base();
        repo.write("new file.txt", "new\n");
        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Stage,
            &WorkingTreeScope::File(status('?', '?', "new file.txt")),
        )
        .expect("untracked stage should succeed");
        assert!(!repo.git_status(["diff", "--cached", "--quiet"]));

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Unstage,
            &WorkingTreeScope::File(status('A', ' ', "new file.txt")),
        )
        .expect("untracked file should be unstaged");
        assert!(repo.path.join("new file.txt").exists());

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Discard,
            &WorkingTreeScope::File(status('?', '?', "new file.txt")),
        )
        .expect("untracked discard should succeed");
        assert!(!repo.path.join("new file.txt").exists());
    }

    #[test]
    fn pathspec_operations_handle_literal_paths() {
        let repo = TempRepo::new();
        repo.commit_base();
        let literal_path = "[literal].txt";
        repo.write(literal_path, "literal\n");

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Stage,
            &WorkingTreeScope::File(status('?', '?', literal_path)),
        )
        .expect("special paths should be stageable");
        assert!(!repo.git_status(["diff", "--cached", "--quiet"]));

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Unstage,
            &WorkingTreeScope::File(status('A', ' ', literal_path)),
        )
        .expect("special paths should be unstageable");
        assert!(repo.path.join(literal_path).exists());

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Discard,
            &WorkingTreeScope::File(status('?', '?', literal_path)),
        )
        .expect("special paths should be discardable");
        assert!(!repo.path.join(literal_path).exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn pathspec_operations_handle_newline_paths() {
        let repo = TempRepo::new();
        repo.commit_base();
        let newline_path = "line\nbreak.txt";
        repo.write(newline_path, "newline\n");

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Stage,
            &WorkingTreeScope::File(status('?', '?', newline_path)),
        )
        .expect("newline paths should be stageable");
        assert!(!repo.git_status(["diff", "--cached", "--quiet"]));

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Unstage,
            &WorkingTreeScope::File(status('A', ' ', newline_path)),
        )
        .expect("newline paths should be unstageable");

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Discard,
            &WorkingTreeScope::File(status('?', '?', newline_path)),
        )
        .expect("newline paths should be discardable");
        assert!(!repo.path.join(newline_path).exists());
    }

    #[test]
    fn discard_unstaged_rename_restores_old_path_and_removes_new_path() {
        let repo = TempRepo::new();
        repo.commit_base();
        fs::rename(
            repo.path.join("tracked.txt"),
            repo.path.join("renamed.txt"),
        )
        .expect("test rename should succeed");

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Discard,
            &WorkingTreeScope::File(FileStatus {
                index: ' ',
                worktree: 'R',
                path: "renamed.txt".to_string(),
                old_path: Some("tracked.txt".to_string()),
            }),
        )
        .expect("unstaged rename should be discardable");
        assert_eq!(
            fs::read_to_string(repo.path.join("tracked.txt"))
                .expect("old path should be restored")
                .replace("\r\n", "\n"),
            "base\n"
        );
        assert!(!repo.path.join("renamed.txt").exists());
    }

    #[test]
    fn discard_does_not_remove_ignored_files() {
        let repo = TempRepo::new();
        repo.commit_base();
        repo.write(".gitignore", "ignored.txt\n");
        repo.git(["add", ".gitignore"]);
        repo.git(["commit", "-q", "-m", "ignore"]);
        repo.write("ignored.txt", "keep\n");

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Discard,
            &WorkingTreeScope::File(status('?', '?', "ignored.txt")),
        )
        .expect("ignored file cleanup should be harmless");
        assert!(repo.path.join("ignored.txt").exists());
    }

    #[test]
    fn discard_all_uses_the_captured_snapshot() {
        let repo = TempRepo::new();
        repo.commit_base();
        repo.write("tracked.txt", "changed\n");
        repo.write("captured.txt", "remove\n");
        repo.write("created-after-confirmation.txt", "keep\n");

        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Discard,
            &WorkingTreeScope::All(vec![
                status(' ', 'M', "tracked.txt"),
                status('?', '?', "captured.txt"),
            ]),
        )
        .expect("captured discard should succeed");
        assert!(repo.git_status(["diff", "--quiet"]));
        assert!(!repo.path.join("captured.txt").exists());
        assert!(repo.path.join("created-after-confirmation.txt").exists());
    }

    #[test]
    fn unstage_without_head_removes_only_the_index_entry() {
        let repo = TempRepo::new();
        repo.write("initial.txt", "initial\n");
        repo.git(["add", "initial.txt"]);
        apply_operation(
            &repo.handle(),
            WorkingTreeAction::Unstage,
            &WorkingTreeScope::File(status('A', ' ', "initial.txt")),
        )
        .expect("unborn repository should support unstage");
        assert!(repo.path.join("initial.txt").exists());
        assert!(
            repo.git_status(["ls-files", "--error-unmatch", "initial.txt"])
                == false
        );
    }
}
