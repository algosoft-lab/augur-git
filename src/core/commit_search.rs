//! Pure commit-message matching used by the commit graph search bar.

use crate::core::graph::LogRow;

/// Which part of a commit is searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitSearchField {
    Subject,
    FullMessage,
}

/// Filter commits while preserving their existing Git order.
pub fn search_log_rows(
    commits: &[LogRow],
    query: &str,
    field: CommitSearchField,
) -> Vec<LogRow> {
    if query.is_empty() {
        return commits.to_vec();
    }

    commits
        .iter()
        .filter(|commit| commit_matches(commit, query, field))
        .cloned()
        .collect()
}

/// Return whether a commit matches the requested field.
pub fn commit_matches(
    commit: &LogRow,
    query: &str,
    field: CommitSearchField,
) -> bool {
    let haystack = match field {
        CommitSearchField::Subject => &commit.subject,
        CommitSearchField::FullMessage => &commit.message,
    };

    let needle = normalize_loose(query);
    if needle.is_empty() {
        return true;
    }
    normalize_loose(haystack).contains(&needle)
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
            CommitSearchField::Subject
        ));
        assert!(commit_matches(
            &commit,
            "FIX-LOGIN",
            CommitSearchField::Subject
        ));
    }

    #[test]
    fn search_does_not_correct_spelling() {
        let commit = row("Fix Login", "Fix Login");

        assert!(!commit_matches(
            &commit,
            "fix_lgoin",
            CommitSearchField::Subject
        ));
    }

    #[test]
    fn full_message_mode_matches_body_only_terms() {
        let commit = row("Release", "Release\n\nEnable SSO login");

        assert!(!commit_matches(
            &commit,
            "sso_login",
            CommitSearchField::Subject
        ));
        assert!(commit_matches(
            &commit,
            "sso_login",
            CommitSearchField::FullMessage
        ));
    }

    #[test]
    fn empty_queries_do_not_filter() {
        let commits = vec![row("One", "One"), row("Two", "Two")];

        assert_eq!(
            search_log_rows(&commits, "", CommitSearchField::Subject).len(),
            2
        );
        assert_eq!(
            search_log_rows(&commits, "___", CommitSearchField::Subject).len(),
            2
        );
    }
}
