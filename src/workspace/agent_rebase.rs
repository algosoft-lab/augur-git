//! Rebase-specific Agent coordination and pure outcome classification.

use crate::core::git::agent_operation::AgentRebaseProbe;

/// The kind of rebase work an interactive Agent session is allowed to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AgentRebaseMode {
    /// Start a new rebase using the immutable upstream commit.
    Start { upstream_oid: String },
    /// Continue an existing rebase, such as one left by `pull --rebase`.
    Resolve {
        upstream_oid: Option<String>,
        rebase_head_oid: Option<String>,
    },
}

impl AgentRebaseMode {
    pub(super) fn upstream_oid(&self) -> Option<&str> {
        match self {
            Self::Start { upstream_oid } => Some(upstream_oid),
            Self::Resolve { upstream_oid, .. } => upstream_oid.as_deref(),
        }
    }
}

/// Result of a Rebase by AI session after checking repository state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AgentRebaseOutcome {
    Rebased { oid: String },
    AlreadyUpToDate,
    Conflict,
    Failed,
    Cancelled,
    ExitedUnverified { code: Option<i32> },
}

/// Classify a post-marker probe without inspecting provider output.
///
/// A branch rebase with a known upstream requires that upstream to be an
/// ancestor of the resulting HEAD. A pull --rebase recovery has no stable
/// target OID after its fetch, so it relies on the rebase state disappearing,
/// a clean tree, and a changed HEAD.
pub(super) fn classify_rebase_probe(
    mode: &AgentRebaseMode,
    baseline_head: Option<&str>,
    probe: &AgentRebaseProbe,
) -> Option<AgentRebaseOutcome> {
    if probe.rebase_in_progress || probe.has_conflicts {
        return Some(AgentRebaseOutcome::Conflict);
    }
    if probe.has_changes {
        return None;
    }
    if mode.upstream_oid().is_some() && !probe.target_is_ancestor_of_head {
        return None;
    }
    if baseline_head == probe.head.as_deref() {
        return match mode {
            AgentRebaseMode::Start { .. } => {
                Some(AgentRebaseOutcome::AlreadyUpToDate)
            }
            AgentRebaseMode::Resolve { .. } => None,
        };
    }
    probe
        .head
        .clone()
        .map(|oid| AgentRebaseOutcome::Rebased { oid })
}

#[cfg(test)]
mod tests {
    use super::{AgentRebaseMode, AgentRebaseOutcome, classify_rebase_probe};
    use crate::core::git::agent_operation::AgentRebaseProbe;

    fn probe(
        head: Option<&str>,
        rebase_head: Option<&str>,
        in_progress: bool,
        changes: bool,
        conflicts: bool,
        ancestor: bool,
    ) -> AgentRebaseProbe {
        AgentRebaseProbe {
            head: head.map(str::to_owned),
            rebase_head: rebase_head.map(str::to_owned),
            rebase_in_progress: in_progress,
            has_changes: changes,
            has_conflicts: conflicts,
            target_is_ancestor_of_head: ancestor,
        }
    }

    #[test]
    fn changed_head_and_ancestor_upstream_prove_rebase() {
        assert_eq!(
            classify_rebase_probe(
                &AgentRebaseMode::Start {
                    upstream_oid: "upstream".into(),
                },
                Some("base"),
                &probe(Some("rebased"), None, false, false, false, true),
            ),
            Some(AgentRebaseOutcome::Rebased {
                oid: "rebased".into(),
            })
        );
    }

    #[test]
    fn unchanged_head_is_already_up_to_date_only_for_new_rebase() {
        let state = probe(Some("base"), None, false, false, false, true);
        assert_eq!(
            classify_rebase_probe(
                &AgentRebaseMode::Start {
                    upstream_oid: "upstream".into(),
                },
                Some("base"),
                &state,
            ),
            Some(AgentRebaseOutcome::AlreadyUpToDate)
        );
        assert_eq!(
            classify_rebase_probe(
                &AgentRebaseMode::Resolve {
                    upstream_oid: None,
                    rebase_head_oid: Some("current".into()),
                },
                Some("base"),
                &state,
            ),
            None
        );
    }

    #[test]
    fn conflicts_or_unfinished_rebase_never_report_success() {
        assert_eq!(
            classify_rebase_probe(
                &AgentRebaseMode::Start {
                    upstream_oid: "upstream".into(),
                },
                Some("base"),
                &probe(Some("base"), Some("pick"), true, true, true, false),
            ),
            Some(AgentRebaseOutcome::Conflict)
        );
        assert_eq!(
            classify_rebase_probe(
                &AgentRebaseMode::Start {
                    upstream_oid: "upstream".into(),
                },
                Some("base"),
                &probe(Some("other"), None, false, false, false, false),
            ),
            None
        );
    }

    #[test]
    fn pull_rebase_resolution_accepts_changed_clean_head_without_target() {
        assert_eq!(
            classify_rebase_probe(
                &AgentRebaseMode::Resolve {
                    upstream_oid: None,
                    rebase_head_oid: None,
                },
                Some("before"),
                &probe(Some("after"), None, false, false, false, false),
            ),
            Some(AgentRebaseOutcome::Rebased {
                oid: "after".into(),
            })
        );
    }
}
