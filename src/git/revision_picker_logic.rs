//! Pure revision-picker classification and filtering helpers.

use std::collections::HashSet;

use crate::core::git::{CompareRevision, CompareRevisionKind};

/// A revision candidate displayed by the picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RevisionPickerOption {
    pub value: CompareRevision,
    pub label: String,
}

impl RevisionPickerOption {
    pub(crate) fn new(
        value: CompareRevision,
        label: impl Into<String>,
    ) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }

    pub(crate) fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.label.to_lowercase().contains(&query)
            || self.value.name.to_lowercase().contains(&query)
            || self.value.full_name.to_lowercase().contains(&query)
    }
}

/// The current interpretation of a picker's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RevisionPickerInput {
    Empty,
    Selected(CompareRevision),
    ManualSha(CompareRevision),
    Invalid(String),
}

impl RevisionPickerInput {
    pub(crate) fn revision(&self) -> Option<CompareRevision> {
        match self {
            Self::Selected(revision) | Self::ManualSha(revision) => {
                Some(revision.clone())
            }
            Self::Empty | Self::Invalid(_) => None,
        }
    }

    pub(crate) fn is_valid(&self) -> bool {
        !matches!(self, Self::Empty | Self::Invalid(_))
    }
}

/// Classify text against the current catalog without consulting Git.
pub(crate) fn classify_input(
    input: &str,
    selected: Option<&CompareRevision>,
    options: &[RevisionPickerOption],
) -> RevisionPickerInput {
    let text = input.trim();
    if text.is_empty() {
        return RevisionPickerInput::Empty;
    }

    if let Some(selected) = selected.filter(|selected| {
        selected.name.eq_ignore_ascii_case(text)
            || selected.full_name.eq_ignore_ascii_case(text)
    }) {
        return RevisionPickerInput::Selected(selected.clone());
    }

    if let Some(option) = options.iter().find(|option| {
        option.value.name.eq_ignore_ascii_case(text)
            || option.value.full_name.eq_ignore_ascii_case(text)
            || option.label.eq_ignore_ascii_case(text)
    }) {
        return RevisionPickerInput::Selected(option.value.clone());
    }

    if let Some(revision) = CompareRevision::from_commit_id(text) {
        return RevisionPickerInput::ManualSha(revision);
    }

    RevisionPickerInput::Invalid(text.to_string())
}

/// Return whether a valid SHA already has an exact catalog candidate.
pub(crate) fn has_exact_option(
    query: &str,
    options: &[RevisionPickerOption],
) -> bool {
    options.iter().any(|option| {
        option.value.name.eq_ignore_ascii_case(query)
            || option.value.full_name.eq_ignore_ascii_case(query)
    })
}

pub(crate) fn section_for_kind(kind: CompareRevisionKind) -> usize {
    match kind {
        CompareRevisionKind::Local | CompareRevisionKind::Remote => 0,
        CompareRevisionKind::Tag => 1,
        CompareRevisionKind::Commit => 2,
    }
}

/// Partition and deduplicate candidates in their display order.
pub(crate) fn grouped_options(
    options: impl IntoIterator<Item = RevisionPickerOption>,
) -> [Vec<RevisionPickerOption>; 3] {
    let mut groups = std::array::from_fn(|_| Vec::new());
    let mut seen = HashSet::new();
    for option in options {
        if !seen.insert(option.value.full_name.clone()) {
            continue;
        }
        groups[section_for_kind(option.value.kind)].push(option);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(
        kind: CompareRevisionKind,
        name: &str,
        full_name: &str,
    ) -> RevisionPickerOption {
        RevisionPickerOption::new(
            CompareRevision {
                name: name.to_string(),
                full_name: full_name.to_string(),
                kind,
            },
            name,
        )
    }

    #[test]
    fn classify_known_revisions_and_manual_sha() {
        let options = vec![
            option(CompareRevisionKind::Local, "main", "refs/heads/main"),
            option(CompareRevisionKind::Tag, "v1.0", "refs/tags/v1.0"),
            option(CompareRevisionKind::Commit, "abcdef1", &"a".repeat(40)),
        ];
        assert!(matches!(
            classify_input("main", None, &options),
            RevisionPickerInput::Selected(CompareRevision {
                kind: CompareRevisionKind::Local,
                ..
            })
        ));
        assert!(matches!(
            classify_input("refs/tags/v1.0", None, &options),
            RevisionPickerInput::Selected(CompareRevision {
                kind: CompareRevisionKind::Tag,
                ..
            })
        ));
        assert!(matches!(
            classify_input(&"a".repeat(40), None, &options),
            RevisionPickerInput::Selected(CompareRevision {
                kind: CompareRevisionKind::Commit,
                ..
            })
        ));
        assert!(matches!(
            classify_input("b".repeat(7).as_str(), None, &options),
            RevisionPickerInput::ManualSha(_)
        ));
        assert!(matches!(
            classify_input(&"c".repeat(64), None, &options),
            RevisionPickerInput::ManualSha(_)
        ));
    }

    #[test]
    fn classify_empty_and_invalid_text() {
        let options = Vec::new();
        assert_eq!(
            classify_input("  ", None, &options),
            RevisionPickerInput::Empty
        );
        assert!(matches!(
            classify_input("abcdef", None, &options),
            RevisionPickerInput::Invalid(_)
        ));
        assert!(matches!(
            classify_input("zzzzzzz", None, &options),
            RevisionPickerInput::Invalid(_)
        ));
        assert!(matches!(
            classify_input(&"a".repeat(65), None, &options),
            RevisionPickerInput::Invalid(_)
        ));
    }

    #[test]
    fn manual_sha_is_not_added_when_catalog_has_exact_commit() {
        let options = vec![option(
            CompareRevisionKind::Commit,
            "abcdef1",
            &"a".repeat(40),
        )];
        assert!(has_exact_option(&"a".repeat(40), &options));
        assert!(!has_exact_option(&"b".repeat(7), &options));
    }

    #[test]
    fn filter_matches_names_refs_and_subjects() {
        let mut option =
            option(CompareRevisionKind::Commit, "abc1234", &"a".repeat(40));
        option.label = "commit · abc1234 · Fix Unicode 路径".into();
        assert!(option.matches("unicode"));
        assert!(option.matches("路径"));
        assert!(option.matches("ABC1234"));
    }

    #[test]
    fn grouping_keeps_branches_tags_and_commits_separate() {
        let groups = grouped_options([
            option(CompareRevisionKind::Commit, "c", "c".repeat(40).as_str()),
            option(CompareRevisionKind::Tag, "v1", "refs/tags/v1"),
            option(
                CompareRevisionKind::Remote,
                "origin/main",
                "refs/remotes/origin/main",
            ),
            option(CompareRevisionKind::Local, "main", "refs/heads/main"),
        ]);
        assert_eq!(groups[0].len(), 2);
        assert_eq!(groups[1].len(), 1);
        assert_eq!(groups[2].len(), 1);
    }
}
