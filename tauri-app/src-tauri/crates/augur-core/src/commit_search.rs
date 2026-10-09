//! Pure commit-search matching used by the commit graph search bar.
//!
//! Messages match fuzzily: after ignoring case, whitespace, underscores, and
//! dashes, every query character must appear in order (a subsequence), so
//! `fxlgn` finds `Fix Login` while a scrambled `ngolxfi` does not.
//!
//! Queries that are at least four hex characters additionally match commits
//! whose full or short hash starts with the query, the way Git abbreviates
//! hashes. The two modes are combined, so a hex-looking word such as `dead`
//! still fuzzy-matches messages and also prefix-matches hashes.

use crate::graph::LogRow;

/// Minimum normalized query length before hash-prefix matching applies.
const MIN_HASH_QUERY_LENGTH: usize = 4;

/// Which part of a commit is searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitSearchField {
    Subject,
    FullMessage,
}

/// Filter commits while preserving their existing Git order.
pub fn search_log_rows(commits: &[LogRow], query: &str, field: CommitSearchField) -> Vec<LogRow> {
    if normalize_loose(query).is_empty() {
        return commits.to_vec();
    }

    commits
        .iter()
        .filter(|commit| commit_matches(commit, query, field))
        .cloned()
        .collect()
}

/// Return whether a commit matches the requested field or its hashes.
pub fn commit_matches(commit: &LogRow, query: &str, field: CommitSearchField) -> bool {
    let haystack = match field {
        CommitSearchField::Subject => &commit.subject,
        CommitSearchField::FullMessage => &commit.message,
    };

    let needle = normalize_loose(query);
    if needle.is_empty() {
        return true;
    }

    subsequence(&normalize_loose(haystack), &needle)
        || (is_hash_query(&needle)
            && (commit.oid.to_lowercase().starts_with(&needle)
                || commit.short.to_lowercase().starts_with(&needle)))
}

/// True when every needle character appears in the haystack in order.
fn subsequence(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    for character in needle.chars() {
        let Some(found) = haystack[from..].find(character) else {
            return false;
        };
        from += found + character.len_utf8();
    }
    true
}

fn is_hash_query(needle: &str) -> bool {
    needle.len() >= MIN_HASH_QUERY_LENGTH
        && needle.chars().all(|character| character.is_ascii_hexdigit())
}

fn normalize_loose(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '_' && *character != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(subject: &str, message: &str) -> LogRow {
        row_with_hashes(subject, message, &"a".repeat(40), &"a".repeat(7))
    }

    fn row_with_hashes(subject: &str, message: &str, oid: &str, short: &str) -> LogRow {
        LogRow {
            oid: oid.into(),
            short: short.into(),
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
    fn out_of_order_queries_do_not_match() {
        let commit = row("Fix Login", "Fix Login");

        assert!(!commit_matches(
            &commit,
            "ngolxfi",
            CommitSearchField::Subject
        ));
    }

    #[test]
    fn message_subsequences_match() {
        let commit = row("Fix Login", "Fix Login");

        assert!(commit_matches(&commit, "fxlgn", CommitSearchField::Subject));
        assert!(commit_matches(&commit, "fx", CommitSearchField::Subject));
        assert!(!commit_matches(&commit, "fixz", CommitSearchField::Subject));
    }

    #[test]
    fn full_message_mode_matches_body_only_terms() {
        let commit = row("Release", "Release\n\nEnable SSO login");

        assert!(!commit_matches(&commit, "sso", CommitSearchField::Subject));
        assert!(commit_matches(&commit, "sso", CommitSearchField::FullMessage));
    }

    #[test]
    fn short_hash_prefix_matches() {
        let commit = row_with_hashes(
            "Bump version",
            "Bump version",
            &format!("{}{}", "a1b2c3d", "0".repeat(33)),
            "a1b2c3d",
        );

        assert!(commit_matches(&commit, "a1b2c3d", CommitSearchField::Subject));
        assert!(commit_matches(&commit, "A1B2C3D", CommitSearchField::Subject));
    }

    #[test]
    fn full_hash_prefix_matches_beyond_short_length() {
        let commit = row_with_hashes(
            "Bump version",
            "Bump version",
            &format!("{}{}", "a1b2c3d4e5", "0".repeat(30)),
            "a1b2c3d",
        );

        assert!(commit_matches(
            &commit,
            "a1b2c3d4e5",
            CommitSearchField::Subject
        ));
        assert!(!commit_matches(
            &commit,
            "a1b2c3d4e5f",
            CommitSearchField::Subject
        ));
    }

    #[test]
    fn hash_matching_requires_four_hex_characters() {
        let commit = row_with_hashes(
            "Unrelated message",
            "Unrelated message",
            &format!("{}{}", "abc", "0".repeat(37)),
            "abc0000",
        );

        assert!(!commit_matches(&commit, "abc", CommitSearchField::Subject));
    }

    #[test]
    fn non_hex_queries_never_match_hashes() {
        let commit = row_with_hashes(
            "Unrelated message",
            "Unrelated message",
            &format!("{}{}", "xyzz", "0".repeat(36)),
            "xyzz0000",
        );

        assert!(!commit_matches(&commit, "xyzz", CommitSearchField::Subject));
    }

    #[test]
    fn hex_looking_words_match_messages_and_hashes() {
        let by_message = row("Handle deadlock safely", "Handle deadlock safely");
        let by_hash = row_with_hashes(
            "Unrelated message",
            "Unrelated message",
            &format!("{}{}", "dead", "0".repeat(36)),
            "dead000",
        );

        assert!(commit_matches(&by_message, "dead", CommitSearchField::Subject));
        assert!(commit_matches(&by_hash, "dead", CommitSearchField::Subject));
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
