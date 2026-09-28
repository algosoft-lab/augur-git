//! Git argument builders for every user-visible operation.
//!
//! The frontend never supplies raw Git arguments. It names an operation, and
//! this module turns that operation into a fixed argument vector. Keeping the
//! mapping here means the frontend cannot be talked into running an arbitrary
//! command, and every path-like value travels as one structured argument
//! instead of shell text.
//!
//! All builders are pure so the exact argv of each operation is unit tested.

use serde::{Deserialize, Serialize};

use augur_core::git::{CheckoutTarget, CompareRevision};

/// A repository operation the interface can request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum GitAction {
    Fetch,
    PullMerge,
    PullRebase,
    Push,
    /// Force push. The interface confirms first; the backend cannot know that.
    PushForce,
    /// Publish a branch that has no upstream yet.
    PushSetUpstream {
        remote: String,
        branch: String,
    },
    /// Create the new remote branch and delete the old one in one push.
    PushRenameRemote {
        remote: String,
        old: String,
        new: String,
    },
    /// Delete a branch on its remote.
    PushDeleteRemote {
        remote: String,
        branch: String,
    },
    Stash {
        message: String,
    },
    /// Pop the newest stash, or one explicit entry.
    StashPop {
        stash_ref: Option<String>,
    },
    StashDrop {
        stash_ref: String,
    },
    /// Apply a patch file. Plain `git apply` is atomic and leaves the result
    /// unstaged.
    ApplyPatch {
        path: String,
    },
    Checkout {
        target: CheckoutTarget,
    },
    CreateBranch {
        name: String,
    },
    RenameBranch {
        old: String,
        new: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    DeleteTag {
        name: String,
    },
    Merge {
        source: String,
        no_ff: bool,
    },
    Rebase {
        source: String,
    },
    AbortMerge,
    AbortRebase,
    /// Reset the worktree after a conflicted stash pop. Git keeps the stash
    /// entry when the pop fails, so the reset only discards the applied
    /// state and the change can be retried later.
    AbortStashApply,
    Commit {
        message: String,
        amend: bool,
    },
    /// Read a commit message for the clipboard. Never mutates the repository.
    CopyCommitMessage {
        oid: String,
    },
}

impl GitAction {
    /// Operations whose success requires a fresh repository snapshot.
    pub fn refreshes_after_success(&self) -> bool {
        matches!(
            self,
            GitAction::Commit { .. }
                | GitAction::Checkout { .. }
                | GitAction::ApplyPatch { .. }
                | GitAction::Fetch
                | GitAction::PullMerge
                | GitAction::PullRebase
                | GitAction::Push
                | GitAction::PushForce
                | GitAction::PushSetUpstream { .. }
                | GitAction::PushRenameRemote { .. }
                | GitAction::PushDeleteRemote { .. }
                | GitAction::CreateBranch { .. }
                | GitAction::RenameBranch { .. }
                | GitAction::DeleteBranch { .. }
                | GitAction::DeleteTag { .. }
                | GitAction::Stash { .. }
                | GitAction::StashPop { .. }
                | GitAction::StashDrop { .. }
                | GitAction::Merge { .. }
                | GitAction::Rebase { .. }
                | GitAction::AbortMerge
                | GitAction::AbortRebase
                | GitAction::AbortStashApply
        )
    }

    /// The label shown in the status bar while the operation runs and in the
    /// completion message. The frontend routes merge and rebase results by
    /// label, so these strings are part of the protocol.
    pub fn label(&self) -> &'static str {
        match self {
            GitAction::Fetch => "fetch --all --prune",
            GitAction::PullMerge => "pull",
            GitAction::PullRebase => "pull --rebase",
            GitAction::Push => "push",
            GitAction::PushForce => "push --force",
            GitAction::PushSetUpstream { .. } => "push --set-upstream",
            GitAction::PushRenameRemote { .. } => "push --rename",
            GitAction::PushDeleteRemote { .. } => "push --delete",
            GitAction::Stash { .. } => "stash",
            GitAction::StashPop { .. } => "stash pop",
            GitAction::StashDrop { .. } => "stash drop",
            GitAction::ApplyPatch { .. } => "apply",
            GitAction::Checkout { .. } => "checkout",
            GitAction::CreateBranch { .. } => "branch",
            GitAction::RenameBranch { .. } => "branch -m",
            GitAction::DeleteBranch { force: true, .. } => "branch -D",
            GitAction::DeleteBranch { .. } => "branch -d",
            GitAction::DeleteTag { .. } => "tag -d",
            GitAction::Merge { no_ff: true, .. } => "merge --no-ff",
            GitAction::Merge { .. } => "merge",
            GitAction::Rebase { .. } => "rebase",
            GitAction::AbortMerge => "merge --abort",
            GitAction::AbortRebase => "rebase --abort",
            GitAction::AbortStashApply => "stash pop abort",
            GitAction::Commit { amend: true, .. } => "commit --amend",
            GitAction::Commit { .. } => "commit",
            GitAction::CopyCommitMessage { .. } => "copy-commit-message",
        }
    }

    /// Whether the frontend must run a preflight probe before this action.
    pub fn needs_merge_preflight(&self) -> bool {
        matches!(
            self,
            GitAction::Merge { .. } | GitAction::AbortMerge | GitAction::PushSetUpstream { .. }
        )
    }

    /// Whether the frontend must run a rebase preflight before this action.
    pub fn needs_rebase_preflight(&self) -> bool {
        matches!(
            self,
            GitAction::Rebase { .. } | GitAction::PullRebase | GitAction::AbortRebase
        )
    }

    /// Build the argument vector, or a user-facing reason the action is
    /// malformed.
    pub fn args(&self) -> Result<Vec<String>, String> {
        Ok(match self {
            GitAction::Fetch => strs(&["fetch", "--all", "--prune"]),
            GitAction::PullMerge => strs(&["pull"]),
            GitAction::PullRebase => strs(&["pull", "--rebase"]),
            GitAction::Push => strs(&["push"]),
            GitAction::PushForce => strs(&["push", "--force"]),
            GitAction::PushSetUpstream { remote, branch } => {
                strs(&["push", "--set-upstream", remote, branch])
            }
            GitAction::PushRenameRemote { remote, old, new } => strs(&[
                "push",
                remote,
                // The source is the local remote-tracking ref, which fetch
                // maintains. A `refs/heads/<old>` source would fail with
                // "src refspec does not match any" because the branch need not
                // exist locally.
                &format!("refs/remotes/{remote}/{old}:refs/heads/{new}"),
                &format!(":refs/heads/{old}"),
            ]),
            GitAction::PushDeleteRemote { remote, branch } => {
                strs(&["push", remote, "--delete", branch])
            }
            GitAction::Stash { message } => strs(&["stash", "push", "-m", message]),
            GitAction::StashPop { stash_ref: None } => strs(&["stash", "pop"]),
            GitAction::StashPop {
                stash_ref: Some(reference),
            } => strs(&["stash", "pop", reference]),
            GitAction::StashDrop { stash_ref } => strs(&["stash", "drop", stash_ref]),
            GitAction::ApplyPatch { path } => strs(&["apply", path]),
            GitAction::Checkout { target } => checkout_args(target),
            // `switch -c` creates and checks out, which is what the dialog
            // promises. Plain `branch` would leave the reader on the old
            // branch with a new one they are not on.
            GitAction::CreateBranch { name } => strs(&["switch", "-c", name]),
            GitAction::RenameBranch { old, new } => strs(&["branch", "-m", old, new]),
            GitAction::DeleteBranch { name, force } => {
                strs(&["branch", if *force { "-D" } else { "-d" }, name])
            }
            GitAction::DeleteTag { name } => strs(&["tag", "-d", name]),
            GitAction::Merge { source, no_ff } => {
                if *no_ff {
                    strs(&["merge", source, "--no-ff"])
                } else {
                    strs(&["merge", source])
                }
            }
            GitAction::Rebase { source } => strs(&["rebase", source]),
            GitAction::AbortMerge => strs(&["merge", "--abort"]),
            GitAction::AbortRebase => strs(&["rebase", "--abort"]),
            GitAction::AbortStashApply => strs(&["reset", "--hard"]),
            GitAction::Commit { message, amend } => commit_args(message, *amend),
            // The message only, with no diff, notes, colour, or external
            // diff driver, because it goes straight to the clipboard.
            GitAction::CopyCommitMessage { oid } => strs(&[
                "show",
                "--no-patch",
                "--format=%B",
                "--no-color",
                "--no-ext-diff",
                "--no-notes",
                oid,
            ]),
        })
    }
}

fn strs(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

pub fn checkout_args(target: &CheckoutTarget) -> Vec<String> {
    match target {
        CheckoutTarget::LocalBranch { local_branch } => strs(&["switch", local_branch]),
        CheckoutTarget::RemoteBranch { remote_branch } => {
            strs(&["switch", "--track", remote_branch])
        }
        CheckoutTarget::Tag { tag } => strs(&["switch", "--detach", tag]),
        CheckoutTarget::Commit { commit } => strs(&["switch", "--detach", commit]),
    }
}

pub fn commit_args(message: &str, amend: bool) -> Vec<String> {
    let mut args = vec!["commit".to_string(), "-m".to_string(), message.to_string()];
    if amend {
        args.push("--amend".to_string());
    }
    args
}

/// First line of a multi-line Git message, for one-line status text.
pub fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// Whether a push failed because the branch has no upstream. Git's wording
/// changed between versions, so both known messages are recognized.
pub fn push_error_missing_upstream(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("no upstream")
        || (message.contains("current branch")
            && (message.contains("has no upstream branch") || message.contains("upstream branch")))
        || message.contains("please set a upstream")
}

/// Why a branch name was rejected before Git saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    Invalid,
    Exists,
}

/// Validate a branch name against Git ref rules and the existing local
/// branches. `allow` exempts one name from the duplicate check, which rename
/// needs because the old name is still listed.
pub fn validate_branch_name(
    name: &str,
    existing: &[String],
    allow: Option<&str>,
) -> Option<NameError> {
    if name.is_empty() {
        return Some(NameError::Empty);
    }
    if name.starts_with(['-', '.', '/'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
        || name
            .chars()
            .any(|c| matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\') || c.is_control())
    {
        return Some(NameError::Invalid);
    }
    if allow != Some(name) && existing.iter().any(|branch| branch == name) {
        return Some(NameError::Exists);
    }
    None
}

/// The patch filename suggested when exporting a comparison.
pub fn suggested_patch_filename(base: &CompareRevision, target: &CompareRevision) -> String {
    augur_core::git::suggested_patch_filename(base, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(action: GitAction) -> Vec<String> {
        action.args().expect("valid action")
    }

    #[test]
    fn remote_operations_pin_the_expected_arguments() {
        assert_eq!(args(GitAction::Fetch), ["fetch", "--all", "--prune"]);
        assert_eq!(args(GitAction::PullMerge), ["pull"]);
        assert_eq!(args(GitAction::PullRebase), ["pull", "--rebase"]);
        assert_eq!(args(GitAction::Push), ["push"]);
        assert_eq!(args(GitAction::PushForce), ["push", "--force"]);
        assert_eq!(
            args(GitAction::PushSetUpstream {
                remote: "origin".into(),
                branch: "main".into()
            }),
            ["push", "--set-upstream", "origin", "main"]
        );
        assert_eq!(
            args(GitAction::PushDeleteRemote {
                remote: "origin".into(),
                branch: "old".into()
            }),
            ["push", "origin", "--delete", "old"]
        );
    }

    #[test]
    fn remote_rename_creates_the_new_ref_and_deletes_the_old_one() {
        assert_eq!(
            args(GitAction::PushRenameRemote {
                remote: "origin".into(),
                old: "a".into(),
                new: "b".into()
            }),
            [
                "push",
                "origin",
                "refs/remotes/origin/a:refs/heads/b",
                ":refs/heads/a"
            ]
        );
    }

    #[test]
    fn checkout_picks_the_matching_switch_form() {
        assert_eq!(
            args(GitAction::Checkout {
                target: CheckoutTarget::LocalBranch {
                    local_branch: "main".into()
                }
            }),
            ["switch", "main"]
        );
        assert_eq!(
            args(GitAction::Checkout {
                target: CheckoutTarget::RemoteBranch {
                    remote_branch: "origin/topic".into()
                }
            }),
            ["switch", "--track", "origin/topic"]
        );
        assert_eq!(
            args(GitAction::Checkout {
                target: CheckoutTarget::Tag { tag: "v1".into() }
            }),
            ["switch", "--detach", "v1"]
        );
        assert_eq!(
            args(GitAction::Checkout {
                target: CheckoutTarget::Commit {
                    commit: "abc1234".into()
                }
            }),
            ["switch", "--detach", "abc1234"]
        );
    }

    #[test]
    fn checkout_targets_deserialize_the_frontend_wire_shape() {
        let targets = [
            (
                serde_json::json!({ "kind": "localBranch", "localBranch": "topic/name" }),
                CheckoutTarget::LocalBranch {
                    local_branch: "topic/name".into(),
                },
            ),
            (
                serde_json::json!({ "kind": "remoteBranch", "remoteBranch": "origin/topic" }),
                CheckoutTarget::RemoteBranch {
                    remote_branch: "origin/topic".into(),
                },
            ),
            (
                serde_json::json!({ "kind": "tag", "tag": "v1.2" }),
                CheckoutTarget::Tag { tag: "v1.2".into() },
            ),
            (
                serde_json::json!({ "kind": "commit", "commit": "abc1234" }),
                CheckoutTarget::Commit {
                    commit: "abc1234".into(),
                },
            ),
        ];

        for (wire, expected) in targets {
            assert_eq!(
                serde_json::from_value::<CheckoutTarget>(wire).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn creating_a_branch_also_checks_it_out() {
        // The dialog says "Creates and checks out a new branch", so `branch`
        // alone would leave the reader on the old branch. `switch -c` does both.
        assert_eq!(
            args(GitAction::CreateBranch {
                name: "feature/tauri".into()
            }),
            ["switch", "-c", "feature/tauri"]
        );
    }

    #[test]
    fn branch_deletion_covers_normal_force_and_tag_variants() {
        assert_eq!(
            args(GitAction::DeleteBranch {
                name: "f".into(),
                force: false
            }),
            ["branch", "-d", "f"]
        );
        assert_eq!(
            args(GitAction::DeleteBranch {
                name: "f".into(),
                force: true
            }),
            ["branch", "-D", "f"]
        );
        assert_eq!(
            args(GitAction::DeleteTag { name: "v1".into() }),
            ["tag", "-d", "v1"]
        );
    }

    #[test]
    fn merge_toggles_the_no_ff_flag() {
        assert_eq!(
            args(GitAction::Merge {
                source: "f".into(),
                no_ff: false
            }),
            ["merge", "f"]
        );
        assert_eq!(
            args(GitAction::Merge {
                source: "f".into(),
                no_ff: true
            }),
            ["merge", "f", "--no-ff"]
        );
    }

    #[test]
    fn commit_and_amend_keep_the_message_as_one_argument() {
        assert_eq!(
            args(GitAction::Commit {
                message: "subject\n\nbody".into(),
                amend: false
            }),
            ["commit", "-m", "subject\n\nbody"]
        );
        assert_eq!(
            args(GitAction::Commit {
                message: "fixed".into(),
                amend: true
            }),
            ["commit", "-m", "fixed", "--amend"]
        );
    }

    #[test]
    fn stash_pop_targets_the_newest_or_one_explicit_entry() {
        assert_eq!(
            args(GitAction::StashPop { stash_ref: None }),
            ["stash", "pop"]
        );
        assert_eq!(
            args(GitAction::StashPop {
                stash_ref: Some("stash@{2}".into())
            }),
            ["stash", "pop", "stash@{2}"]
        );
        assert_eq!(
            args(GitAction::StashDrop {
                stash_ref: "stash@{2}".into()
            }),
            ["stash", "drop", "stash@{2}"]
        );
    }

    #[test]
    fn stash_pop_abort_resets_the_worktree_and_keeps_its_label_routable() {
        assert_eq!(args(GitAction::AbortStashApply), ["reset", "--hard"]);
        assert_eq!(GitAction::AbortStashApply.label(), "stash pop abort");
        assert!(GitAction::AbortStashApply.refreshes_after_success());
    }

    #[test]
    fn labels_route_merge_and_rebase_results() {
        assert_eq!(
            GitAction::Merge {
                source: "f".into(),
                no_ff: true
            }
            .label(),
            "merge --no-ff"
        );
        assert_eq!(GitAction::AbortRebase.label(), "rebase --abort");
        assert_eq!(GitAction::PullRebase.label(), "pull --rebase");
        assert!(GitAction::PullRebase.needs_rebase_preflight());
        assert!(!GitAction::PullMerge.needs_rebase_preflight());
        assert!(
            GitAction::Merge {
                source: "f".into(),
                no_ff: false
            }
            .needs_merge_preflight()
        );
    }

    #[test]
    fn only_mutating_actions_trigger_a_refresh() {
        assert!(GitAction::Fetch.refreshes_after_success());
        assert!(
            GitAction::Checkout {
                target: CheckoutTarget::LocalBranch {
                    local_branch: "m".into()
                }
            }
            .refreshes_after_success()
        );
        assert!(!GitAction::CopyCommitMessage { oid: "a".into() }.refreshes_after_success());
    }

    #[test]
    fn branch_name_validation_matches_git_ref_rules() {
        let refs = vec!["main".to_string(), "feature/one".to_string()];
        assert_eq!(validate_branch_name("dev", &refs, None), None);
        assert_eq!(validate_branch_name("topic+.patch", &refs, None), None);
        assert_eq!(
            validate_branch_name("", &refs, None),
            Some(NameError::Empty)
        );
        for name in [
            "-dev", ".hidden", "a..b", "a b", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b",
            "a@{b", "a.lock", "a/", "a.", "/a", "a//b",
        ] {
            assert_eq!(
                validate_branch_name(name, &refs, None),
                Some(NameError::Invalid),
                "expected {name:?} to be rejected"
            );
        }
        assert_eq!(
            validate_branch_name("main", &refs, None),
            Some(NameError::Exists)
        );
        assert_eq!(validate_branch_name("main", &refs, Some("main")), None);
    }

    #[test]
    fn missing_upstream_push_errors_are_recognized() {
        assert!(push_error_missing_upstream(
            "fatal: The current branch main has no upstream branch."
        ));
        assert!(!push_error_missing_upstream(
            "fatal: 'main' does not appear to be a git repository"
        ));
        assert!(!push_error_missing_upstream("permission denied"));
    }
}
