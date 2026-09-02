//! Read-only Git probes used to coordinate external Agent operations.

use std::path::Path;
use std::process::Output;

use super::git_command;

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
    /// Whether the requested target is reachable from the current HEAD.
    pub target_is_ancestor_of_head: bool,
}

/// Read the repository state without mutating the worktree or index.
pub fn probe_agent_commit(
    repo_path: &Path,
) -> Result<AgentCommitProbe, String> {
    let output = git_command()
        .args([
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ])
        .current_dir(repo_path)
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
    repo_path: &Path,
    target_oid: &str,
) -> Result<AgentMergeProbe, String> {
    let commit = probe_agent_commit(repo_path)?;
    let merge_head = read_merge_head(repo_path)?;
    let target_is_ancestor_of_head =
        match (target_oid.is_empty(), commit.head.as_deref()) {
            (true, _) | (false, None) => false,
            (false, Some(_)) => is_ancestor(repo_path, target_oid)?,
        };
    Ok(AgentMergeProbe {
        head: commit.head,
        merge_head,
        has_changes: commit.has_changes,
        has_conflicts: commit.has_conflicts,
        target_is_ancestor_of_head,
    })
}

/// Return whether Git has another stateful operation in progress.
///
/// The marker paths are resolved through Git so linked worktrees and custom
/// git directories are handled correctly. This check is intentionally kept
/// separate from [`AgentMergeProbe`]: callers that are already handling an
/// existing merge can still inspect `MERGE_HEAD` without treating it as an
/// unrelated operation.
pub fn has_other_git_operation(repo_path: &Path) -> Result<bool, String> {
    for marker in [
        "REBASE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ] {
        let path = git_path(repo_path, marker)?;
        if path.exists() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Read merge state without checking ancestry. This is used after an ordinary
/// merge command fails, before the user chooses how to recover.
pub fn probe_merge_state(repo_path: &Path) -> Result<AgentMergeProbe, String> {
    let commit = probe_agent_commit(repo_path)?;
    Ok(AgentMergeProbe {
        head: commit.head,
        merge_head: read_merge_head(repo_path)?,
        has_changes: commit.has_changes,
        has_conflicts: commit.has_conflicts,
        target_is_ancestor_of_head: false,
    })
}

/// Resolve a local branch to an immutable commit object id before putting it
/// in an Agent prompt. The branch name is passed as one structured argument.
pub fn resolve_agent_merge_target(
    repo_path: &Path,
    branch: &str,
) -> Result<String, String> {
    let reference = format!("refs/heads/{branch}^{{commit}}");
    let output = git_command()
        .args(["rev-parse", "--verify"])
        .arg(reference)
        .current_dir(repo_path)
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

fn read_merge_head(repo_path: &Path) -> Result<Option<String>, String> {
    let output = git_command()
        .args(["rev-parse", "--verify", "--quiet", "MERGE_HEAD"])
        .current_dir(repo_path)
        .output()
        .map_err(|error| format!("failed to inspect merge state: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn git_path(
    repo_path: &Path,
    marker: &str,
) -> Result<std::path::PathBuf, String> {
    let output = git_command()
        .args(["rev-parse", "--git-path"])
        .arg(marker)
        .current_dir(repo_path)
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
    let path = std::path::PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        repo_path.join(path)
    })
}

fn is_ancestor(repo_path: &Path, target_oid: &str) -> Result<bool, String> {
    let output = git_command()
        .args(["merge-base", "--is-ancestor"])
        .arg(target_oid)
        .arg("HEAD")
        .current_dir(repo_path)
        .output()
        .map_err(|error| format!("failed to verify merge ancestry: {error}"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(command_error(&output, "git merge-base")),
    }
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
    use super::{AgentCommitProbe, parse_agent_commit_status};

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
}
