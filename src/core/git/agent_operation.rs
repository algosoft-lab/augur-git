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

fn status_error(output: &Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        format!("git status exited with {}", output.status)
    } else {
        format!("git status failed: {detail}")
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
