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

use super::{GitRepo, RepoLocation};

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
    let reference = format!("refs/heads/{branch}^{{commit}}");
    let output = git_command_in_repo(repo)
        .args(["rev-parse", "--verify"])
        .arg(reference)
        .output()
        .map_err(|error| format!("failed to resolve merge target: {error}"))?;
    if !output.status.success() {
        return Err(command_error(&output, "git rev-parse"));
    }
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if oid.is_empty() {
        Err("merge target resolved to an empty object id".to_string())
    } else {
        Ok(oid)
    }
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

        let staged = parse_status(b"# branch.oid aaaa\01 M. N... 100644 100644 100644 aaa bbb R100\0src/x.rs\0");
        assert!(staged.has_changes);
        assert!(!staged.has_conflicts);

        let untracked = parse_status(b"# branch.oid aaaa\0? src/new.rs\0");
        assert!(untracked.has_changes);
        assert!(!untracked.has_conflicts);

        let conflicted = parse_status(b"# branch.oid aaaa\0u UU N... 100644 100644 100644 100644 aaa bbb ccc src/x.rs\0");
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
}
