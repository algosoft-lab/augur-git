//! In-progress feedback verbs for generic Git commands.
//!
//! The status bar shows `Fetching.` → `Fetching..` → `Fetching...` while the
//! Git worker executes a command. Verbs are intentionally English-only and
//! are derived from the Git subcommand (`args[0]`), not from the freeform
//! display label, so unknown or non-Git commands fall back to a universal
//! verb instead of showing a wrong action name.

/// English in-progress verb for a Git subcommand, e.g. `push` → `Pushing`.
pub(crate) fn progress_verb(subcommand: &str) -> &'static str {
    match subcommand {
        "fetch" => "Fetching",
        "pull" => "Pulling",
        "push" => "Pushing",
        "commit" => "Committing",
        "merge" => "Merging",
        "rebase" => "Rebasing",
        "checkout" => "Checking out",
        "switch" => "Switching",
        "stash" => "Stashing",
        "apply" => "Applying patch",
        "cherry-pick" => "Cherry-picking",
        "revert" => "Reverting",
        "reset" => "Resetting",
        "restore" => "Restoring",
        "tag" => "Tagging",
        "branch" => "Updating branch",
        // Universal fallback for commands without a dedicated verb.
        _ => "Running",
    }
}

#[cfg(test)]
mod tests {
    use super::progress_verb;

    #[test]
    fn network_commands_map_to_ing_verbs() {
        assert_eq!(progress_verb("fetch"), "Fetching");
        assert_eq!(progress_verb("pull"), "Pulling");
        assert_eq!(progress_verb("push"), "Pushing");
    }

    #[test]
    fn mutating_commands_map_to_ing_verbs() {
        assert_eq!(progress_verb("commit"), "Committing");
        assert_eq!(progress_verb("merge"), "Merging");
        assert_eq!(progress_verb("rebase"), "Rebasing");
        assert_eq!(progress_verb("checkout"), "Checking out");
        assert_eq!(progress_verb("switch"), "Switching");
        assert_eq!(progress_verb("stash"), "Stashing");
        assert_eq!(progress_verb("apply"), "Applying patch");
        assert_eq!(progress_verb("cherry-pick"), "Cherry-picking");
        assert_eq!(progress_verb("revert"), "Reverting");
        assert_eq!(progress_verb("reset"), "Resetting");
        assert_eq!(progress_verb("restore"), "Restoring");
        assert_eq!(progress_verb("tag"), "Tagging");
        assert_eq!(progress_verb("branch"), "Updating branch");
    }

    #[test]
    fn unknown_subcommands_use_the_universal_verb() {
        assert_eq!(progress_verb("copy-commit-message"), "Running");
        assert_eq!(progress_verb("definitely-not-a-subcommand"), "Running");
        assert_eq!(progress_verb(""), "Running");
    }
}
