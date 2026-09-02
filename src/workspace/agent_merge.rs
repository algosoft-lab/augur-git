//! Merge-specific Agent coordination and pure outcome classification.

use crate::core::git::agent_operation::AgentMergeProbe;

/// The kind of merge work an interactive Agent session is allowed to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AgentMergeMode {
    /// Start a new merge using the immutable target commit.
    Start { target_oid: String },
    /// Finish the merge represented by an existing `MERGE_HEAD`.
    Resolve { merge_head_oid: String },
}

impl AgentMergeMode {
    pub(super) fn target_oid(&self) -> &str {
        match self {
            Self::Start { target_oid }
            | Self::Resolve {
                merge_head_oid: target_oid,
            } => target_oid,
        }
    }
}

/// Result of a Merge by AI session after checking repository state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AgentMergeOutcome {
    Merged { oid: String },
    AlreadyUpToDate,
    Conflict,
    Failed,
    Cancelled,
    ExitedUnverified { code: Option<i32> },
}

/// Classify a post-marker probe without inspecting provider output.
///
/// The target must be an ancestor of the resulting HEAD and the repository
/// must no longer be in a merge state. A changed HEAD proves a merge (or
/// fast-forward); an unchanged HEAD is only successful for a new merge that
/// was already up to date before the Agent started.
pub(super) fn classify_merge_probe(
    mode: &AgentMergeMode,
    baseline_head: Option<&str>,
    probe: &AgentMergeProbe,
) -> Option<AgentMergeOutcome> {
    if probe.has_conflicts || probe.merge_head.is_some() {
        return Some(AgentMergeOutcome::Conflict);
    }
    if !probe.target_is_ancestor_of_head {
        return None;
    }
    if baseline_head == probe.head.as_deref() {
        return match mode {
            AgentMergeMode::Start { .. } => {
                Some(AgentMergeOutcome::AlreadyUpToDate)
            }
            AgentMergeMode::Resolve { .. } => None,
        };
    }
    probe
        .head
        .clone()
        .map(|oid| AgentMergeOutcome::Merged { oid })
}

#[cfg(test)]
mod tests {
    use super::{AgentMergeMode, AgentMergeOutcome, classify_merge_probe};
    use crate::core::git::agent_operation::AgentMergeProbe;

    fn probe(
        head: Option<&str>,
        merge_head: Option<&str>,
        changes: bool,
        conflicts: bool,
        ancestor: bool,
    ) -> AgentMergeProbe {
        AgentMergeProbe {
            head: head.map(str::to_owned),
            merge_head: merge_head.map(str::to_owned),
            has_changes: changes,
            has_conflicts: conflicts,
            target_is_ancestor_of_head: ancestor,
        }
    }

    #[test]
    fn changed_head_proves_a_new_merge() {
        assert_eq!(
            classify_merge_probe(
                &AgentMergeMode::Start {
                    target_oid: "target".into(),
                },
                Some("base"),
                &probe(Some("merged"), None, false, false, true),
            ),
            Some(AgentMergeOutcome::Merged {
                oid: "merged".into(),
            })
        );
    }

    #[test]
    fn unchanged_head_is_already_up_to_date_only_for_new_merge() {
        let state = probe(Some("base"), None, false, false, true);
        assert_eq!(
            classify_merge_probe(
                &AgentMergeMode::Start {
                    target_oid: "target".into(),
                },
                Some("base"),
                &state,
            ),
            Some(AgentMergeOutcome::AlreadyUpToDate)
        );
        assert_eq!(
            classify_merge_probe(
                &AgentMergeMode::Resolve {
                    merge_head_oid: "target".into(),
                },
                Some("base"),
                &state,
            ),
            None
        );
    }

    #[test]
    fn merge_state_or_non_ancestor_never_reports_success() {
        assert_eq!(
            classify_merge_probe(
                &AgentMergeMode::Start {
                    target_oid: "target".into(),
                },
                Some("base"),
                &probe(Some("base"), Some("target"), true, true, false),
            ),
            Some(AgentMergeOutcome::Conflict)
        );
        assert_eq!(
            classify_merge_probe(
                &AgentMergeMode::Start {
                    target_oid: "target".into(),
                },
                Some("base"),
                &probe(Some("other"), None, false, false, false),
            ),
            None
        );
    }
}
