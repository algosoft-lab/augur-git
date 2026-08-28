//! Commit diff context and merge-specific Git argument builders.

use crate::core::diff::{FileChange, FileChangeStatus};

/// Additional context needed to query a commit diff.
///
/// `None` keeps the existing command path for normal commits. `Some` contains
/// the first parent of a merge commit and makes the diff compare that parent
/// with the merge result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitDiffContext {
    pub merge_parent: Option<String>,
}

/// Parse the first line of `git rev-list --parents -n 1 <oid>`.
pub(crate) fn parse_parent_line(
    text: &str,
) -> Result<CommitDiffContext, String> {
    let line = text
        .lines()
        .next()
        .filter(|line| !line.trim().is_empty())
        .ok_or_else(|| {
            "Git returned no commit parent information".to_string()
        })?;
    let ids: Vec<&str> = line.split_whitespace().collect();
    if ids.is_empty() || ids.iter().any(|id| !is_object_id(id)) {
        return Err(
            "Git returned malformed commit parent information".to_string()
        );
    }

    Ok(CommitDiffContext {
        merge_parent: (ids.len() > 2).then(|| ids[1].to_string()),
    })
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Build the parent query used to determine whether a commit is a merge.
pub(crate) fn parent_query_args(repo_path: &str, oid: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "rev-list".to_string(),
        "--parents".to_string(),
        "-n".to_string(),
        "1".to_string(),
        oid.to_string(),
    ]
}

/// Build the raw file metadata query for a merge's first-parent diff.
pub(crate) fn merge_raw_args(
    repo_path: &str,
    parent: &str,
    oid: &str,
) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff-tree".to_string(),
        "--no-commit-id".to_string(),
        "-r".to_string(),
        "-M".to_string(),
        "--raw".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "-z".to_string(),
        parent.to_string(),
        oid.to_string(),
    ]
}

/// Build the numstat query for a merge's first-parent diff.
pub(crate) fn merge_numstat_args(
    repo_path: &str,
    parent: &str,
    oid: &str,
) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff".to_string(),
        "--numstat".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
        parent.to_string(),
        oid.to_string(),
    ]
}

/// Build a single-file patch query for a merge's first-parent diff.
pub(crate) fn merge_patch_args(
    repo_path: &str,
    parent: &str,
    oid: &str,
    file: &FileChange,
) -> Vec<String> {
    let path = if matches!(file.status, FileChangeStatus::Deleted) {
        file.old_path.as_deref().unwrap_or(&file.new_path)
    } else {
        &file.new_path
    };
    let mut args = vec![
        "--no-pager".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
        parent.to_string(),
        oid.to_string(),
        "--".to_string(),
    ];
    if file.status.is_rename() {
        if let Some(old_path) = file.old_path.as_deref() {
            args.push(old_path.to_string());
        }
    }
    args.push(path.to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    const FIRST_PARENT: &str = "1123456789abcdef0123456789abcdef01234567";
    const SECOND_PARENT: &str = "2123456789abcdef0123456789abcdef01234567";
    const THIRD_PARENT: &str = "3123456789abcdef0123456789abcdef01234567";

    #[test]
    fn parse_parent_line_distinguishes_root_normal_and_merge_commits() {
        assert_eq!(
            parse_parent_line(&format!("{COMMIT}\n")),
            Ok(CommitDiffContext { merge_parent: None })
        );
        assert_eq!(
            parse_parent_line(&format!("{COMMIT} {FIRST_PARENT}\n")),
            Ok(CommitDiffContext { merge_parent: None })
        );
        assert_eq!(
            parse_parent_line(&format!(
                "{COMMIT} {FIRST_PARENT} {SECOND_PARENT}\n"
            )),
            Ok(CommitDiffContext {
                merge_parent: Some(FIRST_PARENT.to_string())
            })
        );
        assert_eq!(
            parse_parent_line(&format!(
                "{COMMIT} {FIRST_PARENT} {SECOND_PARENT} {THIRD_PARENT}\n"
            )),
            Ok(CommitDiffContext {
                merge_parent: Some(FIRST_PARENT.to_string())
            })
        );
    }

    #[test]
    fn parse_parent_line_rejects_empty_and_malformed_output() {
        assert!(parse_parent_line("").is_err());
        assert!(parse_parent_line("not-a-valid-commit\n").is_err());
        assert!(parse_parent_line(&format!("{COMMIT} invalid\n")).is_err());
    }

    #[test]
    fn merge_raw_and_numstat_args_compare_first_parent_to_merge() {
        let raw = merge_raw_args("repo", FIRST_PARENT, COMMIT);
        let numstat = merge_numstat_args("repo", FIRST_PARENT, COMMIT);
        for args in [raw, numstat] {
            assert!(
                args.windows(2).any(|pair| {
                    pair[0] == FIRST_PARENT && pair[1] == COMMIT
                })
            );
            assert!(!args.iter().any(|arg| arg == "-m"));
        }
    }

    #[test]
    fn merge_patch_args_preserve_structured_rename_paths() {
        let file = FileChange {
            path: "old.rs => new.rs".to_string(),
            old_path: Some("old.rs".to_string()),
            new_path: "new.rs".to_string(),
            status: FileChangeStatus::Renamed,
            old_blob: None,
            new_blob: None,
            added: Some(1),
            deleted: Some(0),
        };
        let args = merge_patch_args("repo", FIRST_PARENT, COMMIT, &file);
        let separator = args.iter().position(|arg| arg == "--");
        assert!(separator.is_some());
        let separator = separator.unwrap_or_default();
        assert_eq!(&args[separator + 1..], ["old.rs", "new.rs"]);
        assert!(
            args.windows(2)
                .any(|pair| { pair[0] == FIRST_PARENT && pair[1] == COMMIT })
        );
    }
}
