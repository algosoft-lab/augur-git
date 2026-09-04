/// Branch-name validation failure reasons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NameError {
    Empty,
    Invalid,
    Exists,
}

/// Validate a branch name against git-ref rules and the existing local
/// branches (including the current one). `allow` exempts one name from the
/// exists check, used when renaming (the old name is still listed). Pure so
/// it can be unit tested.
pub(super) fn validate_branch_name(
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
        || name.chars().any(|c| {
            matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
                || c.is_control()
        })
    {
        return Some(NameError::Invalid);
    }
    if allow != Some(name) && existing.iter().any(|branch| branch == name) {
        return Some(NameError::Exists);
    }
    None
}

/// Command label and arguments for renaming a local branch. Pure so it can
/// be unit tested.
pub(super) fn rename_args(old: &str, new: &str) -> (&'static str, Vec<String>) {
    (
        "branch -m",
        vec!["branch".into(), "-m".into(), old.into(), new.into()],
    )
}

/// Command label and arguments for deleting a local branch or a tag. Pure
/// so it can be unit tested.
pub(super) fn delete_args(
    name: &str,
    force: bool,
    is_tag: bool,
) -> (&'static str, Vec<String>) {
    if is_tag {
        ("tag -d", vec!["tag".into(), "-d".into(), name.into()])
    } else if force {
        ("branch -D", vec!["branch".into(), "-D".into(), name.into()])
    } else {
        ("branch -d", vec!["branch".into(), "-d".into(), name.into()])
    }
}

/// Command label and arguments for merging `source` into the current
/// branch. Pure so it can be unit tested.
pub(super) fn merge_args(
    source: &str,
    no_ff: bool,
) -> (&'static str, Vec<String>) {
    if no_ff {
        (
            "merge --no-ff",
            vec!["merge".into(), source.into(), "--no-ff".into()],
        )
    } else {
        ("merge", vec!["merge".into(), source.into()])
    }
}

/// Build arguments for popping the latest stash or one explicit stash entry.
pub(super) fn stash_pop_args(stash_ref: Option<&str>) -> Vec<String> {
    let mut args = vec!["stash".into(), "pop".into()];
    if let Some(stash_ref) = stash_ref {
        args.push(stash_ref.into());
    }
    args
}

/// Build arguments for permanently dropping one explicit stash entry.
pub(super) fn stash_drop_args(stash_ref: &str) -> Vec<String> {
    vec!["stash".into(), "drop".into(), stash_ref.into()]
}

/// Command label and arguments for applying a patch file to the current
/// branch's working tree. Plain `git apply` is atomic — it applies every hunk
/// or leaves the tree untouched — and leaves the results unstaged. The path
/// rides as one structured argument so it can never become command syntax.
/// Pure so it can be unit tested.
pub(super) fn apply_patch_args(path: &str) -> (&'static str, Vec<String>) {
    ("apply", vec!["apply".into(), path.into()])
}

/// Command label and arguments for renaming a remote branch. Git has no
/// native remote rename, so a single push creates the new branch and
/// deletes the old ref. The source of the create refspec is the local
/// remote-tracking ref `refs/remotes/<remote>/<old>` — a plain
/// `refs/heads/<old>` source would require the branch to exist locally
/// ("src refspec does not match any" otherwise), while the tracking ref is
/// maintained by fetch and backs the very sidebar entry this action was
/// launched from. If the new name already exists on the remote, the push is
/// rejected as a non-fast-forward update. Pure so it can be unit tested.
pub(super) fn rename_remote_args(
    remote: &str,
    old: &str,
    new: &str,
) -> (&'static str, Vec<String>) {
    (
        "push --rename",
        vec![
            "push".into(),
            remote.into(),
            format!("refs/remotes/{remote}/{old}:refs/heads/{new}"),
            format!(":refs/heads/{old}"),
        ],
    )
}

/// Command label and arguments for deleting a remote branch on its remote.
/// Pure so it can be unit tested.
pub(super) fn delete_remote_args(
    remote: &str,
    branch: &str,
) -> (&'static str, Vec<String>) {
    (
        "push --delete",
        vec![
            "push".into(),
            remote.into(),
            "--delete".into(),
            branch.into(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{
        NameError, apply_patch_args, delete_args, delete_remote_args,
        merge_args, rename_args, rename_remote_args, stash_drop_args,
        stash_pop_args, validate_branch_name,
    };

    fn existing(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn accepts_common_branch_names() {
        let refs = existing(&["main", "feature/one"]);
        assert_eq!(validate_branch_name("dev", &refs, None), None);
        assert_eq!(validate_branch_name("feature/two", &refs, None), None);
        assert_eq!(validate_branch_name("v1.2.3", &refs, None), None);
        assert_eq!(validate_branch_name("fix_bug-1", &refs, None), None);
        assert_eq!(validate_branch_name("topic+.patch", &refs, None), None);
    }

    #[test]
    fn rejects_empty_names() {
        assert_eq!(validate_branch_name("", &[], None), Some(NameError::Empty));
    }

    #[test]
    fn rejects_invalid_ref_syntax() {
        for name in [
            "-dev", ".hidden", "a..b", "a b", "a~b", "a^b", "a:b", "a?b",
            "a*b", "a[b", "a\\b", "a@{b", "a.lock", "a/", "a.", "/a", "a//b",
        ] {
            assert_eq!(
                validate_branch_name(name, &[], None),
                Some(NameError::Invalid),
                "expected {name:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_existing_branches() {
        let refs = existing(&["main", "feature/one"]);
        assert_eq!(
            validate_branch_name("main", &refs, None),
            Some(NameError::Exists)
        );
        assert_eq!(
            validate_branch_name("feature/one", &refs, None),
            Some(NameError::Exists)
        );
    }

    #[test]
    fn rename_allows_keeping_the_old_name_but_not_other_branches() {
        let refs = existing(&["main", "feature/one"]);
        assert_eq!(
            validate_branch_name("main", &refs, Some("main")),
            None,
            "unchanged old name must stay acceptable"
        );
        assert_eq!(
            validate_branch_name("feature/one", &refs, Some("main")),
            Some(NameError::Exists)
        );
    }

    #[test]
    fn rename_args_keep_old_and_new_as_separate_arguments() {
        let (label, args) = rename_args("old", "new");
        assert_eq!(label, "branch -m");
        assert_eq!(args, vec!["branch", "-m", "old", "new"]);
    }

    #[test]
    fn delete_args_cover_branch_force_and_tag_variants() {
        let (label, args) = delete_args("feature", false, false);
        assert_eq!(label, "branch -d");
        assert_eq!(args, vec!["branch", "-d", "feature"]);

        let (label, args) = delete_args("feature", true, false);
        assert_eq!(label, "branch -D");
        assert_eq!(args, vec!["branch", "-D", "feature"]);

        let (label, args) = delete_args("v1.0", false, true);
        assert_eq!(label, "tag -d");
        assert_eq!(args, vec!["tag", "-d", "v1.0"]);
    }

    #[test]
    fn merge_args_toggle_the_no_ff_flag() {
        let (label, args) = merge_args("feature", false);
        assert_eq!(label, "merge");
        assert_eq!(args, vec!["merge", "feature"]);

        let (label, args) = merge_args("feature", true);
        assert_eq!(label, "merge --no-ff");
        assert_eq!(args, vec!["merge", "feature", "--no-ff"]);
    }

    #[test]
    fn stash_pop_args_target_latest_or_explicit_entry() {
        assert_eq!(stash_pop_args(None), vec!["stash", "pop"]);
        assert_eq!(
            stash_pop_args(Some("stash@{2}")),
            vec!["stash", "pop", "stash@{2}"]
        );
    }

    #[test]
    fn stash_drop_args_target_the_explicit_entry() {
        assert_eq!(
            stash_drop_args("stash@{2}"),
            vec!["stash", "drop", "stash@{2}"]
        );
    }

    #[test]
    fn apply_patch_args_pass_the_patch_path_as_one_argument() {
        let (label, args) =
            apply_patch_args(r"C:\patches\main-to-feature.patch");
        assert_eq!(label, "apply");
        assert_eq!(args, vec!["apply", r"C:\patches\main-to-feature.patch"]);
    }

    #[test]
    fn rename_remote_args_push_new_name_and_delete_old_in_one_push() {
        let (label, args) = rename_remote_args("origin", "old", "new");
        assert_eq!(label, "push --rename");
        assert_eq!(
            args,
            vec![
                "push",
                "origin",
                "refs/remotes/origin/old:refs/heads/new",
                ":refs/heads/old",
            ]
        );
    }

    #[test]
    fn delete_remote_args_target_the_named_remote() {
        let (label, args) = delete_remote_args("upstream", "feature/one");
        assert_eq!(label, "push --delete");
        assert_eq!(args, vec!["push", "upstream", "--delete", "feature/one"]);
    }

    #[test]
    fn remote_branch_names_pass_ref_validation() {
        assert_eq!(validate_branch_name("feature/one", &[], None), None);
        assert_eq!(validate_branch_name("v1.2.3", &[], None), None);
    }
}
