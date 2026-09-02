//! Pure coordination types for Commit by AI sessions.
//!
//! The visible PTY window owns process and rendering state. This module keeps
//! the Git-operation outcome and probe classification independent of GPUI so
//! completion races can be tested without opening a window.

use crate::core::git::agent_operation::AgentCommitProbe;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AgentCommitOutcome {
    Committed { oid: String },
    NoChanges,
    Conflict,
    Failed,
    Cancelled,
    ExitedUnverified { code: Option<i32> },
}

/// Classify a probe after an Agent reports completion.
///
/// A changed HEAD is the only positive proof of a new commit. Conflicts and a
/// clean unchanged tree are terminal negative outcomes; a dirty unchanged tree
/// remains indeterminate until the Agent exits or emits a marker again.
pub(super) fn classify_probe(
    baseline: &AgentCommitProbe,
    probe: &AgentCommitProbe,
) -> Option<AgentCommitOutcome> {
    if baseline.head != probe.head {
        return probe
            .head
            .clone()
            .map(|oid| AgentCommitOutcome::Committed { oid });
    }
    if probe.has_conflicts {
        Some(AgentCommitOutcome::Conflict)
    } else if !probe.has_changes {
        Some(AgentCommitOutcome::NoChanges)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentCommitOutcome, classify_probe};
    use crate::core::git::agent_operation::AgentCommitProbe;

    fn probe(
        head: Option<&str>,
        changes: bool,
        conflicts: bool,
    ) -> AgentCommitProbe {
        AgentCommitProbe {
            head: head.map(str::to_owned),
            has_changes: changes,
            has_conflicts: conflicts,
        }
    }

    #[test]
    fn changed_head_is_the_only_verified_commit() {
        assert_eq!(
            classify_probe(
                &probe(Some("before"), true, false),
                &probe(Some("after"), true, false),
            ),
            Some(AgentCommitOutcome::Committed {
                oid: "after".to_string(),
            })
        );
    }

    #[test]
    fn unborn_head_becoming_a_commit_is_verified() {
        assert_eq!(
            classify_probe(
                &probe(None, true, false),
                &probe(Some("new"), false, false),
            ),
            Some(AgentCommitOutcome::Committed {
                oid: "new".to_string(),
            })
        );
    }

    #[test]
    fn conflicts_and_clean_tree_are_terminal_failures() {
        assert_eq!(
            classify_probe(
                &probe(Some("same"), true, false),
                &probe(Some("same"), true, true),
            ),
            Some(AgentCommitOutcome::Conflict)
        );
        assert_eq!(
            classify_probe(
                &probe(Some("same"), true, false),
                &probe(Some("same"), false, false),
            ),
            Some(AgentCommitOutcome::NoChanges)
        );
    }

    #[test]
    fn dirty_unchanged_tree_stays_unverified() {
        assert_eq!(
            classify_probe(
                &probe(Some("same"), true, false),
                &probe(Some("same"), true, false),
            ),
            None
        );
    }
}
