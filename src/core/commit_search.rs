//! Pure commit-message matching used by the commit graph search bar.

use crate::core::graph::LogRow;

/// Which part of a commit is searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitSearchField {
    Subject,
    FullMessage,
}

/// Matching semantics for commit-message searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitSearchMode {
    Loose,
    Strict,
}

/// Filter commits while preserving their existing Git order.
pub fn search_log_rows(
    commits: &[LogRow],
    query: &str,
    field: CommitSearchField,
    mode: CommitSearchMode,
) -> Vec<LogRow> {
    if query.is_empty() {
        return commits.to_vec();
    }

    commits
        .iter()
        .filter(|commit| commit_matches(commit, query, field, mode))
        .cloned()
        .collect()
}

/// Return whether a commit matches the requested field and mode.
pub fn commit_matches(
    commit: &LogRow,
    query: &str,
    field: CommitSearchField,
    mode: CommitSearchMode,
) -> bool {
    let haystack = match field {
        CommitSearchField::Subject => &commit.subject,
        CommitSearchField::FullMessage => &commit.message,
    };

    match mode {
        CommitSearchMode::Strict => haystack.contains(query),
        CommitSearchMode::Loose => {
            let needle = normalize_loose(query);
            if needle.is_empty() {
                return true;
            }
            normalize_loose(haystack).contains(&needle)
        }
    }
}

fn normalize_loose(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_whitespace() && *character != '_' && *character != '-'
        })
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(subject: &str, message: &str) -> LogRow {
        LogRow {
            oid: "a".repeat(40),
            short: "a".repeat(7),
            author: "Author".into(),
            date: "2026-01-01 00:00".into(),
            timestamp: 0,
            subject: subject.into(),
            message: message.into(),
            decorations: String::new(),
            parents: Vec::new(),
        }
    }

    #[test]
    fn loose_mode_ignores_case_and_common_separators() {
        let commit = row("Fix Login", "Fix Login\n\nAllow SSO");

        assert!(commit_matches(
            &commit,
            "fix_login",
            CommitSearchField::Subject,
            CommitSearchMode::Loose
        ));
        assert!(commit_matches(
            &commit,
            "FIX-LOGIN",
            CommitSearchField::Subject,
            CommitSearchMode::Loose
        ));
    }

    #[test]
    fn loose_mode_does_not_correct_spelling() {
        let commit = row("Fix Login", "Fix Login");

        assert!(!commit_matches(
            &commit,
            "fix_lgoin",
            CommitSearchField::Subject,
            CommitSearchMode::Loose
        ));
    }

    #[test]
    fn strict_mode_preserves_case_and_separators() {
        let commit = row("Fix_Login", "Fix_Login");

        assert!(commit_matches(
            &commit,
            "Fix_Login",
            CommitSearchField::Subject,
            CommitSearchMode::Strict
        ));
        assert!(!commit_matches(
            &commit,
            "fix_login",
            CommitSearchField::Subject,
            CommitSearchMode::Strict
        ));
        assert!(!commit_matches(
            &commit,
            "Fix Login",
            CommitSearchField::Subject,
            CommitSearchMode::Strict
        ));
    }

    #[test]
    fn full_message_mode_matches_body_only_terms() {
        let commit = row("Release", "Release\n\nEnable SSO login");

        assert!(!commit_matches(
            &commit,
            "sso_login",
            CommitSearchField::Subject,
            CommitSearchMode::Loose
        ));
        assert!(commit_matches(
            &commit,
            "sso_login",
            CommitSearchField::FullMessage,
            CommitSearchMode::Loose
        ));
    }

    #[test]
    fn empty_queries_do_not_filter() {
        let commits = vec![row("One", "One"), row("Two", "Two")];

        assert_eq!(
            search_log_rows(
                &commits,
                "",
                CommitSearchField::Subject,
                CommitSearchMode::Loose
            )
            .len(),
            2
        );
        assert_eq!(
            search_log_rows(
                &commits,
                "___",
                CommitSearchField::Subject,
                CommitSearchMode::Loose
            )
            .len(),
            2
        );
    }
}
