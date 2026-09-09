//! Read-only Git probes used to coordinate external Agent operations.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::{GitRepo, RepoLocation};

/// A read-only snapshot of the Git state relevant to an Agent commit.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentCommitProbe {
    /// The current commit object id, or `None` for an unborn HEAD.
    pub head: Option<String>,
    /// Whether Git reports any staged, worktree, or untracked changes.
    pub has_changes: bool,
    /// Whether Git reports an unmerged index entry.
    pub has_conflicts: bool,
}

/// Read-only repository state used by Merge by AI and merge-conflict recovery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentMergeProbe {
    /// The current commit object id, or `None` for an unborn HEAD.
    pub head: Option<String>,
    /// The commit recorded in `.git/MERGE_HEAD`, when a merge is in progress.
    pub merge_head: Option<String>,
    /// Whether Git reports staged, worktree, or untracked changes.
    pub has_changes: bool,
    /// Whether Git reports an unmerged index entry.
    pub has_conflicts: bool,
    /// Whether Git has an in-progress rebase state directory.
    pub rebase_in_progress: bool,
    /// Whether the requested target is reachable from the current HEAD.
    pub target_is_ancestor_of_head: bool,
}

/// Read-only repository state used by Rebase by AI and rebase-conflict
/// recovery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentRebaseProbe {
    /// The current commit object id, or `None` for an unborn HEAD.
    pub head: Option<String>,
    /// The commit currently being replayed, when Git exposes `REBASE_HEAD`.
    pub rebase_head: Option<String>,
    /// Whether a rebase state directory is present.
    pub rebase_in_progress: bool,
    /// Whether Git reports staged, worktree, or untracked changes.
    pub has_changes: bool,
    /// Whether Git reports an unmerged index entry.
    pub has_conflicts: bool,
    /// Whether the requested upstream is reachable from the current HEAD.
    pub target_is_ancestor_of_head: bool,
}

/// Marker files that represent a stateful operation other than merge or
/// rebase. `REBASE_HEAD` is intentionally absent: Git may leave that file
/// behind after a rebase has completed or been aborted, while the authoritative
/// in-progress state is represented by `rebase-merge` or `rebase-apply`.
const NON_MERGE_OPERATION_MARKERS: &[&str] =
    &["CHERRY_PICK_HEAD", "REVERT_HEAD", "BISECT_LOG", "sequencer"];

/// Read the repository state without mutating the worktree or index.
pub fn probe_agent_commit(repo: &GitRepo) -> Result<AgentCommitProbe, String> {
    let output = git_command_in_repo(repo)
        .args([
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ])
        .output()
        .map_err(|error| {
            format!("failed to inspect repository status: {error}")
        })?;
    if !output.status.success() {
        return Err(status_error(&output));
    }
    Ok(parse_agent_commit_status(&output.stdout))
}

/// Read the repository state required before or after an Agent merge.
pub fn probe_agent_merge(
    repo: &GitRepo,
    target_oid: &str,
) -> Result<AgentMergeProbe, String> {
    let commit = probe_agent_commit(repo)?;
    let merge_head = read_merge_head(repo)?;
    let target_is_ancestor_of_head =
        match (target_oid.is_empty(), commit.head.as_deref()) {
            (true, _) | (false, None) => false,
            (false, Some(_)) => is_ancestor(repo, target_oid)?,
        };
    Ok(AgentMergeProbe {
        head: commit.head,
        merge_head,
        rebase_in_progress: rebase_state_exists(repo)?,
        has_changes: commit.has_changes,
        has_conflicts: commit.has_conflicts,
        target_is_ancestor_of_head,
    })
}

/// Read the repository state required before or after an Agent rebase.
/// `target_oid` is present for a branch rebase and omitted for pull --rebase,
/// whose fetched upstream can change while Git is running.
pub fn probe_agent_rebase(
    repo: &GitRepo,
    target_oid: Option<&str>,
) -> Result<AgentRebaseProbe, String> {
    let commit = probe_agent_commit(repo)?;
    let rebase_head = read_rebase_head(repo)?;
    let rebase_in_progress = rebase_state_exists(repo)?;
    let target_is_ancestor_of_head = match (target_oid, commit.head.as_deref())
    {
        (Some(target), Some(_)) if !target.is_empty() => {
            is_ancestor(repo, target)?
        }
        _ => false,
    };
    Ok(AgentRebaseProbe {
        head: commit.head,
        rebase_head,
        rebase_in_progress,
        has_changes: commit.has_changes,
        has_conflicts: commit.has_conflicts,
        target_is_ancestor_of_head,
    })
}

/// Read rebase state without requiring a target commit. This is used after an
/// ordinary rebase or pull --rebase fails, before showing recovery actions.
pub fn probe_rebase_state(repo: &GitRepo) -> Result<AgentRebaseProbe, String> {
    probe_agent_rebase(repo, None)
}

/// Return whether another Git operation is in progress, excluding a rebase.
/// This lets the rebase resolver attach to an existing rebase while still
/// refusing to operate when merge, cherry-pick, revert, bisect, or sequencer
/// state is present.
pub fn has_other_git_operation_except_rebase(
    repo: &GitRepo,
) -> Result<bool, String> {
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "sequencer",
    ] {
        if git_path_exists(repo, marker)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Return whether Git has another stateful operation in progress.
///
/// The marker paths are resolved through Git so linked worktrees and custom
/// git directories are handled correctly. This check is intentionally kept
/// separate from [`AgentMergeProbe`]: callers that are already handling an
/// existing merge can still inspect `MERGE_HEAD` without treating it as an
/// unrelated operation.
pub fn has_other_git_operation(repo: &GitRepo) -> Result<bool, String> {
    for marker in NON_MERGE_OPERATION_MARKERS
        .iter()
        .copied()
        .chain(["rebase-merge", "rebase-apply"])
    {
        if git_path_exists(repo, marker)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Read merge state without checking ancestry. This is used after an ordinary
/// merge command fails, before the user chooses how to recover.
pub fn probe_merge_state(repo: &GitRepo) -> Result<AgentMergeProbe, String> {
    let commit = probe_agent_commit(repo)?;
    Ok(AgentMergeProbe {
        head: commit.head,
        merge_head: read_merge_head(repo)?,
        rebase_in_progress: rebase_state_exists(repo)?,
        has_changes: commit.has_changes,
        has_conflicts: commit.has_conflicts,
        target_is_ancestor_of_head: false,
    })
}

/// Resolve a local branch to an immutable commit object id before putting it
/// in an Agent prompt. The branch name is passed as one structured argument.
pub fn resolve_agent_merge_target(
    repo: &GitRepo,
    branch: &str,
) -> Result<String, String> {
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

fn read_merge_head(repo: &GitRepo) -> Result<Option<String>, String> {
    let output = git_command_in_repo(repo)
        .args(["rev-parse", "--verify", "--quiet", "MERGE_HEAD"])
        .output()
        .map_err(|error| format!("failed to inspect merge state: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn read_rebase_head(repo: &GitRepo) -> Result<Option<String>, String> {
    let output = git_command_in_repo(repo)
        .args(["rev-parse", "--verify", "--quiet", "REBASE_HEAD"])
        .output()
        .map_err(|error| format!("failed to inspect rebase state: {error}"))?;
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
        .map_err(|error| {
            format!("failed to inspect Git operation state: {error}")
        })?;
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
                .map_err(|error| {
                    format!("failed to inspect Git operation state: {error}")
                })?;
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
/// `\\?\\C:\\...` path; preserve the existing normalization for local
/// repositories while leaving WSL paths in their Linux form.
fn git_command_in_repo(repo: &GitRepo) -> Command {
    let path = match repo.location() {
        RepoLocation::Local => {
            normalize_repository_path(Path::new(repo.path()))
        }
        RepoLocation::Wsl { .. } => PathBuf::from(repo.path()),
    };
    let mut command = repo.command();
    command.arg("-C").arg(path);
    command
}

fn normalize_repository_path(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
            return std::path::PathBuf::from(format!("\\\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix("\\\\?\\")
            && rest.as_bytes().get(1) == Some(&b':')
        {
            return std::path::PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn status_error(output: &Output) -> String {
    command_error(output, "git status")
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
pub fn parse_agent_commit_status(output: &[u8]) -> AgentCommitProbe {
    let mut probe = AgentCommitProbe::default();
    for record in output.split(|byte| *byte == 0) {
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if value != b"(initial)" && !value.is_empty() {
                probe.head = Some(String::from_utf8_lossy(value).into_owned());
            }
            continue;
        }
        if record.starts_with(b"u ") {
            probe.has_changes = true;
            probe.has_conflicts = true;
        } else if record.starts_with(b"1 ")
            || record.starts_with(b"2 ")
            || record.starts_with(b"? ")
        {
            probe.has_changes = true;
        }
    }
    probe
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        AgentCommitProbe, GitRepo, NON_MERGE_OPERATION_MARKERS, RepoLocation,
        has_other_git_operation_except_rebase, parse_agent_commit_status,
        probe_agent_rebase, resolve_agent_merge_target,
    };

    #[test]
    fn stale_rebase_head_is_not_an_other_operation_marker() {
        assert!(!NON_MERGE_OPERATION_MARKERS.contains(&"REBASE_HEAD"));
    }

    #[test]
    fn parses_head_and_mixed_changes() {
        let output = b"# branch.oid abc123\0# branch.head main\0"
            .iter()
            .copied()
            .chain(
                b"1 .M N... 100644 100644 100644 a b file.txt\0? new.txt\0"
                    .iter()
                    .copied(),
            )
            .collect::<Vec<_>>();
        assert_eq!(
            parse_agent_commit_status(&output),
            AgentCommitProbe {
                head: Some("abc123".to_string()),
                has_changes: true,
                has_conflicts: false,
            }
        );
    }

    #[test]
    fn parses_unborn_and_conflict_state() {
        let output = b"# branch.oid (initial)\0# branch.head main\0u UU 100644 100644 100644 100644 a b c d conflict.txt\0";
        assert_eq!(
            parse_agent_commit_status(output),
            AgentCommitProbe {
                head: None,
                has_changes: true,
                has_conflicts: true,
            }
        );
    }

    #[test]
    fn clean_status_has_no_change_records() {
        assert_eq!(
            parse_agent_commit_status(
                b"# branch.oid abc123\0# branch.head main\0"
            ),
            AgentCommitProbe {
                head: Some("abc123".to_string()),
                has_changes: false,
                has_conflicts: false,
            }
        );
    }

    #[test]
    fn parses_detached_and_rename_records_without_path_assumptions() {
        let output = b"# branch.oid 0123456789abcdef\0# branch.head (detached)\02 R. N... 100644 100644 old new R100\0old name\0new name\0";
        let probe = parse_agent_commit_status(output);
        assert_eq!(probe.head.as_deref(), Some("0123456789abcdef"));
        assert!(probe.has_changes);
        assert!(!probe.has_conflicts);
    }

    #[test]
    fn preserves_sha256_object_ids_without_abbreviation() {
        let oid =
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let output = format!("# branch.oid {oid}\0# branch.head main\0");
        let probe = parse_agent_commit_status(output.as_bytes());
        assert_eq!(probe.head.as_deref(), Some(oid));
    }

    #[test]
    fn probes_local_repository_and_resolves_branch_target() {
        let repository = TestRepo::new();
        let repo = repository.handle();
        repository.git(["commit", "--allow-empty", "-m", "initial"]);
        repository.git(["branch", "topic"]);

        let target = resolve_agent_merge_target(&repo, "topic")
            .expect("resolve local branch target");
        let probe = probe_agent_rebase(&repo, Some(&target))
            .expect("probe local rebase state");

        assert!(probe.head.is_some());
        assert!(!probe.has_changes);
        assert!(!probe.rebase_in_progress);
        assert!(probe.target_is_ancestor_of_head);
        assert!(
            !has_other_git_operation_except_rebase(&repo)
                .expect("probe local operation markers")
        );
    }

    #[test]
    fn local_operation_marker_is_detected_without_host_path_assumptions() {
        let repository = TestRepo::new();
        let repo = repository.handle();
        repository.git(["commit", "--allow-empty", "-m", "initial"]);
        fs::write(repository.path.join(".git/CHERRY_PICK_HEAD"), "deadbeef\n")
            .expect("write operation marker");

        assert!(
            has_other_git_operation_except_rebase(&repo)
                .expect("probe local operation marker")
        );
    }

    #[cfg(windows)]
    #[test]
    fn wsl_probe_command_uses_the_distro_git() {
        let repo = GitRepo::new(
            RepoLocation::Wsl {
                distro: "Ubuntu".to_string(),
            },
            "/home/u/repo",
        );
        let command = super::git_command_in_repo(&repo);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(command.get_program(), "wsl.exe");
        assert_eq!(
            args,
            vec![
                "-d",
                "Ubuntu",
                "--cd",
                "/home/u/repo",
                "--exec",
                "git",
                "-C",
                "/home/u/repo",
            ]
        );
    }

    struct TestRepo {
        path: PathBuf,
    }

    impl TestRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!(
                "augur-git-agent-operation-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create temporary repository");
            let repository = Self { path };
            repository.git(["init", "-q"]);
            repository.git(["config", "user.email", "test@example.com"]);
            repository.git(["config", "user.name", "augur-git test"]);
            repository
        }

        fn handle(&self) -> GitRepo {
            GitRepo::local(self.path.to_string_lossy().into_owned())
        }

        fn git<const N: usize>(&self, args: [&str; N]) -> String {
            let output =
                GitRepo::local(self.path.to_string_lossy().into_owned())
                    .command()
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
