//! Pure helpers for organizing repository refs in sidebar presentations.

use std::collections::BTreeMap;

/// Display label for branches that carry no `remote/` prefix segment.
const OTHER_GROUP: &str = "(other)";

/// One remote-tracking branch inside its remote group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteBranchEntry {
    /// Full short name as produced by `git branch -r` (e.g. `origin/main`).
    pub full_name: String,
    /// Display label with the matched remote prefix removed (e.g. `main`).
    pub label: String,
}

/// A configured remote and its remote-tracking branches. Remotes without any
/// fetched branch produce an empty group so they stay visible in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteBranchGroup {
    pub remote: String,
    pub branches: Vec<RemoteBranchEntry>,
}

/// Group `git branch -r` short names under their remote for tree rendering.
///
/// Matching prefers known remote names (longest prefix wins) so remotes whose
/// name contains a slash, such as `foo/bar`, group correctly. Unknown prefixes
/// fall back to splitting at the first slash, and names without a slash land
/// in the catch-all `(other)` group. Symbolic `remote/HEAD` aliases are
/// skipped because they duplicate the branch they point at.
pub fn group_remote_branches(
    remotes: &[String],
    branches: &[String],
) -> Vec<RemoteBranchGroup> {
    let mut groups: BTreeMap<&str, Vec<RemoteBranchEntry>> = BTreeMap::new();
    for remote in remotes {
        groups.entry(remote).or_default();
    }
    for branch in branches {
        if branch.ends_with("/HEAD") {
            continue;
        }
        let (remote, label) = match matched_remote(remotes, branch) {
            Some((remote, rest)) => (remote, rest),
            None => match branch.split_once('/') {
                Some((prefix, rest)) => (prefix, rest),
                None => (OTHER_GROUP, branch.as_str()),
            },
        };
        groups.entry(remote).or_default().push(RemoteBranchEntry {
            full_name: branch.clone(),
            label: label.to_string(),
        });
    }
    groups
        .into_iter()
        .map(|(remote, mut branches)| {
            branches.sort_by(|a, b| a.label.cmp(&b.label));
            RemoteBranchGroup {
                remote: remote.to_string(),
                branches,
            }
        })
        .collect()
}

/// Find the known remote owning `branch`, returning `(remote, remainder)`.
/// The longest matching remote wins so `foo/bar` beats a hypothetical `foo`.
fn matched_remote<'a>(
    remotes: &'a [String],
    branch: &'a str,
) -> Option<(&'a str, &'a str)> {
    remotes
        .iter()
        .filter_map(|remote| {
            let rest =
                branch.strip_prefix(remote.as_str())?.strip_prefix('/')?;
            Some((remote.as_str(), rest))
        })
        .max_by_key(|(remote, _)| remote.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn labels(group: &RemoteBranchGroup) -> Vec<&str> {
        group.branches.iter().map(|e| e.label.as_str()).collect()
    }

    #[test]
    fn groups_branches_by_known_remote_and_sorts() {
        let groups = group_remote_branches(
            &names(&["origin", "upstream"]),
            &names(&[
                "upstream/dev",
                "origin/main",
                "origin/feature/x",
                "origin/a",
            ]),
        );
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].remote, "origin");
        assert_eq!(labels(&groups[0]), ["a", "feature/x", "main"]);
        assert_eq!(groups[1].remote, "upstream");
        assert_eq!(labels(&groups[1]), ["dev"]);
    }

    #[test]
    fn keeps_full_name_for_actions() {
        let groups = group_remote_branches(
            &names(&["origin"]),
            &names(&["origin/main"]),
        );
        let entry = &groups[0].branches[0];
        assert_eq!(entry.full_name, "origin/main");
        assert_eq!(entry.label, "main");
    }

    #[test]
    fn slash_in_remote_name_matches_longest_prefix() {
        let groups = group_remote_branches(
            &names(&["foo/bar"]),
            &names(&["foo/bar/baz"]),
        );
        assert_eq!(groups[0].remote, "foo/bar");
        assert_eq!(labels(&groups[0]), ["baz"]);
    }

    #[test]
    fn unknown_prefix_falls_back_to_first_segment() {
        let groups = group_remote_branches(
            &names(&["origin"]),
            &names(&["stale/topic"]),
        );
        let stale = groups
            .iter()
            .find(|g| g.remote == "stale")
            .expect("stale group");
        assert_eq!(labels(stale), ["topic"]);
    }

    #[test]
    fn branch_without_slash_lands_in_other_group() {
        let groups =
            group_remote_branches(&names(&["origin"]), &names(&["odd"]));
        let other = groups
            .iter()
            .find(|g| g.remote == OTHER_GROUP)
            .expect("other group");
        assert_eq!(labels(other), ["odd"]);
    }

    #[test]
    fn head_alias_is_skipped() {
        let groups = group_remote_branches(
            &names(&["origin"]),
            &names(&["origin/HEAD", "origin/main"]),
        );
        assert_eq!(labels(&groups[0]), ["main"]);
    }

    #[test]
    fn empty_remote_still_produces_group() {
        let groups =
            group_remote_branches(&names(&["origin", "empty"]), &names(&[]));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].remote, "empty");
        assert!(groups[0].branches.is_empty());
        assert_eq!(groups[1].remote, "origin");
    }
}
