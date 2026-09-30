//! Read-only Git state probes used to coordinate integration operations.
//!
//! These probes exist because `git status` alone is not enough to decide
//! whether an integration action is safe. A merge that has been fully resolved
//! in the index still leaves `MERGE_HEAD` behind while Git waits for the merge
//! commit, and a completed or aborted rebase can leave `REBASE_HEAD` on disk.
//! The authoritative state therefore comes from the marker files, resolved
//! through Git so linked worktrees and custom git directories work.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use std::process::{Command, Output};

use super::{CompareRevision, GitRepo, RebaseTarget, RepoLocation, ResetTarget};

/// Read-only details used to confirm a reset against the exact repository snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResetPreview {
    pub head_oid: String,
    pub target_oid: String,
    pub target_subject: String,
    pub moved_commits: usize,
    pub has_changes: bool,
}

/// Marker files that represent a stateful operation other than merge or
/// rebase. `REBASE_HEAD` is intentionally absent: Git may leave that file
/// behind after a rebase completes or is aborted, while the authoritative
/// in-progress state is `rebase-merge` or `rebase-apply`.
const NON_MERGE_OPERATION_MARKERS: &[&str] =
    &["CHERRY_PICK_HEAD", "REVERT_HEAD", "BISECT_LOG", "sequencer"];

/// Marker files that mean a merge is still in progress.
const MERGE_OPERATION_MARKERS: &[&str] = &["MERGE_HEAD"];

/// Minimal repository state every probe starts from.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepoState {
    /// The current commit object id, or `None` for an unborn HEAD.
    pub head: Option<String>,
    /// Whether Git reports any staged, worktree, or untracked changes.
    pub has_changes: bool,
    /// Whether Git reports an unmerged index entry.
    pub has_conflicts: bool,
}

/// Repository state used to preflight and diagnose a merge.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergeState {
    pub head: Option<String>,
    /// The commit recorded in `.git/MERGE_HEAD`, when a merge is in progress.
    pub merge_head: Option<String>,
    /// Whether Git has an in-progress rebase state directory.
    pub rebase_in_progress: bool,
    pub has_changes: bool,
    pub has_conflicts: bool,
}

impl MergeState {
    /// Whether the repository is still waiting on a merge commit. Actions that
    /// would discard or replace the merge must stay blocked while this is true.
    pub fn blocks_integration(&self) -> bool {
        self.has_conflicts || self.merge_head.is_some() || self.rebase_in_progress
    }
}

/// Repository state used to preflight and diagnose a rebase.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RebaseState {
    pub head: Option<String>,
    /// The commit currently being replayed, when Git exposes it.
    pub rebase_head: Option<String>,
    pub rebase_in_progress: bool,
    pub has_changes: bool,
    pub has_conflicts: bool,
}

impl RebaseState {
    pub fn blocks_integration(&self) -> bool {
        self.has_conflicts || self.rebase_in_progress || self.rebase_head.is_some()
    }
}

/// Read the repository state without mutating the worktree or index.
pub fn probe_repo_state(repo: &GitRepo) -> Result<RepoState, String> {
    let output = git_command_in_repo(repo)
        .args([
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ])
        .output()
        .map_err(|error| format!("failed to inspect repository status: {error}"))?;
    if !output.status.success() {
        return Err(command_error(&output, "git status"));
    }
    Ok(parse_status(&output.stdout))
}

/// Read merge state.
pub fn probe_merge_state(repo: &GitRepo) -> Result<MergeState, String> {
    let state = probe_repo_state(repo)?;
    Ok(MergeState {
        head: state.head,
        merge_head: read_marker_ref(repo, "MERGE_HEAD")?,
        rebase_in_progress: rebase_state_exists(repo)?,
        has_changes: state.has_changes,
        has_conflicts: state.has_conflicts,
    })
}

/// Read rebase state.
pub fn probe_rebase_state(repo: &GitRepo) -> Result<RebaseState, String> {
    let state = probe_repo_state(repo)?;
    Ok(RebaseState {
        head: state.head,
        rebase_head: read_marker_ref(repo, "REBASE_HEAD")?,
        rebase_in_progress: rebase_state_exists(repo)?,
        has_changes: state.has_changes,
        has_conflicts: state.has_conflicts,
    })
}

/// Whether an operation other than a rebase is in progress. A rebase resolver
/// attaches to an existing rebase, so it must not treat it as a blocker.
pub fn has_other_git_operation_except_rebase(repo: &GitRepo) -> Result<bool, String> {
    for marker in MERGE_OPERATION_MARKERS
        .iter()
        .copied()
        .chain(NON_MERGE_OPERATION_MARKERS.iter().copied())
    {
        if git_path_exists(repo, marker)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether any stateful Git operation is in progress, rebase included.
pub fn has_other_git_operation(repo: &GitRepo) -> Result<bool, String> {
    for marker in MERGE_OPERATION_MARKERS
        .iter()
        .copied()
        .chain(NON_MERGE_OPERATION_MARKERS.iter().copied())
        .chain(["rebase-merge", "rebase-apply"])
    {
        if git_path_exists(repo, marker)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Resolve a local branch to an immutable commit object id. The branch name is
/// passed as one structured argument, never interpolated into a command line.
pub fn resolve_branch_oid(repo: &GitRepo, branch: &str) -> Result<String, String> {
    let reference = format!("refs/heads/{branch}");
    resolve_commit_id(repo, &reference)
        .map_err(|error| format!("could not resolve branch: {error}"))
}

/// Resolve a typed rebase destination to an immutable commit id.
pub fn resolve_rebase_target(repo: &GitRepo, target: &RebaseTarget) -> Result<String, String> {
    match target {
        RebaseTarget::Branch { name } => resolve_branch_oid(repo, name),
        RebaseTarget::Commit { sha } => resolve_user_commit_id(repo, sha),
    }
}

/// Resolve a reset target, verify it is reachable from HEAD, and collect the
/// information shown before the user confirms the operation.
pub fn preview_reset(repo: &GitRepo, target: &ResetTarget) -> Result<ResetPreview, String> {
    let state = probe_repo_state(repo)?;
    let head_oid = resolve_commit_id(repo, "HEAD")?;
    if has_other_git_operation(repo)? {
        return Err("another Git operation is in progress".to_string());
    }

    let target_oid = match target {
        ResetTarget::HeadAncestor { steps } if *steps > 0 => {
            resolve_commit_id(repo, &format!("HEAD~{steps}"))?
        }
        ResetTarget::HeadAncestor { .. } => {
            return Err("HEAD~ must be followed by a positive number".to_string());
        }
        ResetTarget::Commit { sha } => resolve_user_commit_id(repo, sha)?,
    };
    if !is_ancestor(repo, &target_oid)? {
        return Err("reset target is not an ancestor of the current HEAD".to_string());
    }

    let moved_commits = git_output(
        repo,
        &["rev-list", "--count", &format!("{target_oid}..{head_oid}")],
    )?
    .parse::<usize>()
    .map_err(|_| "Git returned an invalid commit count".to_string())?;
    let target_subject = git_output(repo, &["show", "--no-patch", "--format=%s", &target_oid])?;

    Ok(ResetPreview {
        head_oid,
        target_oid,
        target_subject,
        moved_commits,
        has_changes: state.has_changes,
    })
}

/// Recheck the reset preview immediately before changing HEAD.
pub fn validate_reset(repo: &GitRepo, expected_head: &str, target_oid: &str) -> Result<(), String> {
    if !CompareRevision::is_supported_commit_id(expected_head)
        || !CompareRevision::is_supported_commit_id(target_oid)
    {
        return Err("reset requires resolved commit ids".to_string());
    }
    let current_head = resolve_commit_id(repo, "HEAD")?;
    if current_head != expected_head {
        return Err("HEAD changed after the reset preview; review the target again".to_string());
    }
    if has_other_git_operation(repo)? {
        return Err("another Git operation is in progress".to_string());
    }
    let resolved_target = resolve_user_commit_id(repo, target_oid)?;
    if resolved_target != target_oid || !is_ancestor(repo, target_oid)? {
        return Err("reset target is no longer an ancestor of the current HEAD".to_string());
    }
    Ok(())
}

fn resolve_user_commit_id(repo: &GitRepo, sha: &str) -> Result<String, String> {
    if !CompareRevision::is_supported_commit_id(sha) {
        return Err("enter a 7–64 character hexadecimal commit id".to_string());
    }
    resolve_commit_id(repo, sha)
}

fn resolve_commit_id(repo: &GitRepo, revision: &str) -> Result<String, String> {
    let commit = format!("{revision}^{{commit}}");
    let output = git_command_in_repo(repo)
        .args(["rev-parse", "--verify", "--end-of-options"])
        .arg(commit)
        .output()
        .map_err(|error| format!("failed to resolve commit: {error}"))?;
    if !output.status.success() {
        return Err(command_error(&output, "git rev-parse"));
    }
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if oid.is_empty() {
        Err("commit resolved to an empty object id".to_string())
    } else {
        Ok(oid)
    }
}

fn git_output(repo: &GitRepo, args: &[&str]) -> Result<String, String> {
    let output = git_command_in_repo(repo)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(command_error(&output, &format!("git {}", args.join(" "))));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether `target_oid` is reachable from HEAD.
pub fn is_target_ancestor_of_head(repo: &GitRepo, target_oid: &str) -> Result<bool, String> {
    is_ancestor(repo, target_oid)
}

fn read_marker_ref(repo: &GitRepo, marker: &str) -> Result<Option<String>, String> {
    let output = git_command_in_repo(repo)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(marker)
        .output()
        .map_err(|error| format!("failed to inspect Git operation state: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn rebase_state_exists(repo: &GitRepo) -> Result<bool, String> {
    for marker in ["rebase-merge", "rebase-apply"] {
        if git_path_exists(repo, marker)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn git_path(repo: &GitRepo, marker: &str) -> Result<String, String> {
    let output = git_command_in_repo(repo)
        .args(["rev-parse", "--git-path"])
        .arg(marker)
        .output()
        .map_err(|error| format!("failed to inspect Git operation state: {error}"))?;
    if !output.status.success() {
        return Err(command_error(&output, "git rev-parse --git-path"));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        return Err(format!("Git returned an empty path for {marker}"));
    }
    Ok(value)
}

fn git_path_exists(repo: &GitRepo, marker: &str) -> Result<bool, String> {
    let path = git_path(repo, marker)?;
    match repo.location() {
        RepoLocation::Local => {
            let path = PathBuf::from(path);
            let repo_path = normalize_repository_path(Path::new(repo.path()));
            let path = if path.is_absolute() {
                path
            } else {
                repo_path.join(path)
            };
            Ok(path.exists())
        }
        RepoLocation::Wsl { .. } => {
            let output = repo
                .command_in_location("test")
                .arg("-e")
                .arg(path)
                .output()
                .map_err(|error| format!("failed to inspect Git operation state: {error}"))?;
            match output.status.code() {
                Some(0) => Ok(true),
                Some(1) => Ok(false),
                _ => Err(command_error(&output, "test -e")),
            }
        }
    }
}

fn is_ancestor(repo: &GitRepo, target_oid: &str) -> Result<bool, String> {
    let output = git_command_in_repo(repo)
        .args(["merge-base", "--is-ancestor"])
        .arg(target_oid)
        .arg("HEAD")
        .output()
        .map_err(|error| format!("failed to verify merge ancestry: {error}"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(command_error(&output, "git merge-base")),
    }
}

/// Build a location-aware Git command addressed with the `-C` argument.
/// Setting the process working directory instead would tie the child to the
/// host platform's filesystem view and cannot reach repositories inside other
/// locations. Windows canonicalization can return an extended-length
/// `\\?\C:\...` path; local repositories are normalized while WSL paths keep
/// their Linux form.
fn git_command_in_repo(repo: &GitRepo) -> Command {
    let path = match repo.location() {
        RepoLocation::Local => normalize_repository_path(Path::new(repo.path())),
        RepoLocation::Wsl { .. } => PathBuf::from(repo.path()),
    };
    let mut command = repo.command();
    command.arg("-C").arg(path);
    command
}

fn normalize_repository_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix("\\\\?\\")
            && rest.as_bytes().get(1) == Some(&b':')
        {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn command_error(output: &Output, command: &str) -> String {
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        format!("{command} exited with {}", output.status)
    } else {
        format!("{command} failed: {detail}")
    }
}

/// Parse `git status --porcelain=v2 --branch -z` output.
///
/// Porcelain v2 records are NUL-delimited. Header records begin with `#`,
/// ordinary changes with `1 ` or `2 `, unmerged records with `u `, and
/// untracked records with `? `. Rename path payloads are intentionally not
/// interpreted because only the presence of a change matters here.
pub fn parse_status(output: &[u8]) -> RepoState {
    let mut state = RepoState::default();
    for record in output.split(|byte| *byte == 0) {
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if value != b"(initial)" && !value.is_empty() {
                state.head = Some(String::from_utf8_lossy(value).into_owned());
            }
            continue;
        }
        if record.starts_with(b"u ") {
            state.has_changes = true;
            state.has_conflicts = true;
        } else if record.starts_with(b"1 ")
            || record.starts_with(b"2 ")
            || record.starts_with(b"? ")
        {
            state.has_changes = true;
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn rebase_head_is_not_treated_as_an_active_rebase() {
        assert!(!NON_MERGE_OPERATION_MARKERS.contains(&"REBASE_HEAD"));
        assert!(!NON_MERGE_OPERATION_MARKERS.contains(&"rebase-merge"));
    }

    #[test]
    fn status_parsing_detects_head_changes_and_conflicts() {
        let clean = parse_status(b"# branch.oid aaaa\0");
        assert_eq!(clean.head.as_deref(), Some("aaaa"));
        assert!(!clean.has_changes);

        let unborn = parse_status(b"# branch.oid (initial)\0");
        assert_eq!(unborn.head, None);

        let staged = parse_status(
            b"# branch.oid aaaa\01 M. N... 100644 100644 100644 aaa bbb R100\0src/x.rs\0",
        );
        assert!(staged.has_changes);
        assert!(!staged.has_conflicts);

        let untracked = parse_status(b"# branch.oid aaaa\0? src/new.rs\0");
        assert!(untracked.has_changes);
        assert!(!untracked.has_conflicts);

        let conflicted = parse_status(
            b"# branch.oid aaaa\0u UU N... 100644 100644 100644 100644 aaa bbb ccc src/x.rs\0",
        );
        assert!(conflicted.has_changes);
        assert!(conflicted.has_conflicts);
    }

    #[test]
    fn merge_state_blocks_integration_until_merge_head_is_gone() {
        let pending = MergeState {
            merge_head: Some("bbbb".into()),
            ..MergeState::default()
        };
        assert!(pending.blocks_integration());
        assert!(!MergeState::default().blocks_integration());
    }

    #[test]
    fn rebase_state_treats_a_leftover_rebase_head_as_in_progress() {
        let leftover = RebaseState {
            rebase_head: Some("bbbb".into()),
            ..RebaseState::default()
        };
        assert!(leftover.blocks_integration());
    }

    #[test]
    fn reset_preview_resolves_only_current_history_ancestors() {
        let repo = TestRepo::new();
        repo.write("file.txt", "first\n");
        repo.commit("first");
        let first = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["switch", "-c", "side", &first]);
        repo.write("side.txt", "side\n");
        repo.commit("side");
        let side = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["switch", "main"]);
        repo.write("file.txt", "second\n");
        repo.commit("second");

        let preview = preview_reset(&repo.handle(), &ResetTarget::HeadAncestor { steps: 1 })
            .expect("HEAD~1 should resolve");
        assert_eq!(preview.target_oid, first);
        assert_eq!(preview.moved_commits, 1);
        assert_eq!(preview.target_subject, "first");
        validate_reset(&repo.handle(), &preview.head_oid, &preview.target_oid)
            .expect("the previewed ancestor should still be valid");

        assert!(preview_reset(&repo.handle(), &ResetTarget::Commit { sha: side.clone() }).is_err());
        assert!(
            preview_reset(
                &repo.handle(),
                &ResetTarget::Commit {
                    sha: "invalid".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn reset_rejects_a_changed_head_and_an_active_git_operation() {
        let repo = TestRepo::new();
        repo.write("file.txt", "first\n");
        repo.commit("first");
        let first = repo.git(&["rev-parse", "HEAD"]);
        repo.write("file.txt", "second\n");
        repo.commit("second");
        let preview = preview_reset(&repo.handle(), &ResetTarget::HeadAncestor { steps: 1 })
            .expect("HEAD~1 should resolve");

        repo.write("file.txt", "third\n");
        repo.commit("third");
        assert!(validate_reset(&repo.handle(), &preview.head_oid, &first).is_err());

        fs::write(repo.path.join(".git/CHERRY_PICK_HEAD"), &first)
            .expect("write active-operation marker");
        assert!(preview_reset(&repo.handle(), &ResetTarget::Commit { sha: first }).is_err());
    }

    struct TestRepo {
        path: PathBuf,
    }

    static TEST_REPO_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    impl TestRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let sequence = TEST_REPO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "augur-git-reset-probe-{}-{nonce}-{sequence}",
                std::process::id(),
            ));
            fs::create_dir(&path).expect("create temporary repository");
            let repo = Self { path };
            repo.git(&["init", "-q", "--initial-branch=main"]);
            repo.git(&["config", "user.email", "test@example.com"]);
            repo.git(&["config", "user.name", "augur-git test"]);
            repo
        }

        fn handle(&self) -> GitRepo {
            GitRepo::local(self.path.to_string_lossy())
        }

        fn write(&self, path: &str, contents: &str) {
            fs::write(self.path.join(path), contents).expect("write test file");
        }

        fn commit(&self, message: &str) {
            self.git(&["add", "--all"]);
            self.git(&["commit", "-q", "-m", message]);
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.path)
                .args(args)
                .output()
                .expect("run git test command");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
