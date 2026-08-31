//! Commit graph layout and relative-time formatting.
//!
//! The graph layout follows the active-lane model used by VS Code and
//! LazyGit. Each row snapshots the lanes entering the commit, then produces
//! the lanes leaving it. This keeps the layout driven by commit topology
//! rather than branch names or inferred ancestry.

use std::collections::{HashMap, HashSet};

use crate::core::config::GraphHistoryPreference;

/// A commit row produced by the Git log parser.
#[derive(Clone, Debug)]
pub struct LogRow {
    /// Full object id.
    pub oid: String,
    /// Short object id.
    pub short: String,
    pub author: String,
    /// Formatted commit date.
    pub date: String,
    /// Author timestamp in Unix seconds.
    pub timestamp: i64,
    pub subject: String,
    /// Ref decorations such as `HEAD -> main, origin/main`.
    pub decorations: String,
    /// Parent object ids in Git's first-parent order.
    pub parents: Vec<String>,
}

/// A commit graph lane waiting for a particular parent commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLane {
    pub oid: String,
    pub color_index: usize,
}

impl GraphLane {
    fn new(oid: String, color_index: usize) -> Self {
        Self { oid, color_index }
    }
}

/// Commit graph state for one displayed row.
#[derive(Debug, Clone)]
pub struct GraphRow {
    /// Lanes entering this row from the previous commit row.
    pub input_lanes: Vec<GraphLane>,
    /// Lanes leaving this row toward parent commits.
    pub output_lanes: Vec<GraphLane>,
    /// Output lane indices created by this commit, in parent order.
    pub parent_lanes: Vec<usize>,
    /// This commit's lane in the input coordinate system.
    pub node_lane: usize,
    /// Input lanes that converge on this commit's node.
    pub node_input_lanes: Vec<usize>,
    /// Maximum number of lanes visible while drawing this row.
    pub lane_count: usize,
    /// Color carried by the commit node.
    pub node_color: usize,
    /// Whether this commit was already present in an active lane.
    pub has_incoming: bool,
    /// Whether this commit is HEAD.
    pub is_head: bool,
    /// Whether this commit has multiple parents.
    pub is_merge: bool,
}

// ===== Shared commit-list column sizing =====

/// Short object id column width.
pub const HASH_COL_WIDTH: f32 = 60.0;
/// Fixed author column width.
pub const AUTHOR_COL_WIDTH: f32 = 140.0;
/// Relative date column width.
pub const DATE_COL_WIDTH: f32 = 120.0;
/// Minimum usable message column width.
pub const MESSAGE_MIN_WIDTH: f32 = 120.0;
const COL_GAP: f32 = 8.0;
const ROW_PAD_RIGHT: f32 = 8.0;

/// Return whether the author and message columns fit in the available width.
pub fn column_visibility(total_w: f32, tree_w: f32) -> (bool, bool) {
    let t_author = tree_w
        + HASH_COL_WIDTH
        + AUTHOR_COL_WIDTH
        + DATE_COL_WIDTH
        + 4.0 * COL_GAP
        + ROW_PAD_RIGHT
        + MESSAGE_MIN_WIDTH;
    let t_message = tree_w
        + HASH_COL_WIDTH
        + DATE_COL_WIDTH
        + 3.0 * COL_GAP
        + ROW_PAD_RIGHT
        + MESSAGE_MIN_WIDTH;
    (total_w >= t_author, total_w >= t_message)
}

fn refs_of(decorations: &str) -> impl Iterator<Item = &str> {
    decorations
        .split(", ")
        .filter(|decoration| !decoration.is_empty())
}

fn is_head_ref(reference: &str) -> bool {
    reference == "HEAD" || reference.starts_with("HEAD ->")
}

fn is_current_head_ref(reference: &str, current_branch: &str) -> bool {
    is_head_ref(reference)
        || (!current_branch.is_empty() && reference == current_branch)
}

fn is_target_ref(reference: &str, target: &str) -> bool {
    reference == target
        || reference
            .strip_prefix("remotes/")
            .is_some_and(|reference| reference == target)
}

/// Keep the current branch, its tracked upstream, and their reachable history.
pub fn filter_log_rows(
    commits: &[LogRow],
    preference: GraphHistoryPreference,
    current_branch: &str,
    upstream: Option<&str>,
) -> Vec<LogRow> {
    if preference == GraphHistoryPreference::AllBranches {
        return commits.to_vec();
    }

    let head_oid = commits.iter().find_map(|commit| {
        refs_of(&commit.decorations)
            .any(|reference| is_current_head_ref(reference, current_branch))
            .then(|| commit.oid.clone())
    });
    let upstream_oid = upstream.and_then(|target| {
        commits.iter().find_map(|commit| {
            refs_of(&commit.decorations)
                .any(|reference| is_target_ref(reference, target))
                .then(|| commit.oid.clone())
        })
    });

    let mut roots = Vec::with_capacity(2);
    if let Some(oid) = head_oid {
        roots.push(oid);
    }
    if let Some(oid) = upstream_oid {
        if !roots.iter().any(|root| root == &oid) {
            roots.push(oid);
        }
    }

    // If decorations are unavailable, retain the log instead of presenting an
    // empty history that cannot be explained to the user.
    if roots.is_empty() {
        return commits.to_vec();
    }

    let commits_by_oid = commits
        .iter()
        .map(|commit| (commit.oid.as_str(), commit))
        .collect::<HashMap<_, _>>();
    let mut visible = HashSet::new();
    let mut pending = roots;
    while let Some(oid) = pending.pop() {
        if !visible.insert(oid.clone()) {
            continue;
        }
        if let Some(commit) = commits_by_oid.get(oid.as_str()) {
            pending.extend(commit.parents.iter().cloned());
        }
    }

    commits
        .iter()
        .filter(|commit| visible.contains(&commit.oid))
        .cloned()
        .collect()
}

struct ColorAllocator {
    next: usize,
}

impl ColorAllocator {
    fn allocate(&mut self) -> usize {
        let color = self.next;
        self.next = self.next.saturating_add(1);
        color
    }
}

/// Compute active graph lanes for commits already ordered by Git.
pub fn compute_graph(commits: &[LogRow]) -> Vec<GraphRow> {
    let mut active_lanes = Vec::<GraphLane>::new();
    let mut colors = ColorAllocator { next: 0 };
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let input_lanes = active_lanes.clone();
        let current_lane =
            input_lanes.iter().position(|lane| lane.oid == commit.oid);
        let node_lane = current_lane.unwrap_or(input_lanes.len());
        let node_color = current_lane
            .and_then(|lane| input_lanes.get(lane).map(|lane| lane.color_index))
            .unwrap_or_else(|| colors.allocate());

        // Remove every lane waiting for the current commit. Inserting the
        // first parent at the original lane index preserves the visible
        // first-parent line, while duplicate lanes converge at this node.
        let node_input_lanes = current_lane
            .map(|_| {
                input_lanes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, lane)| {
                        (lane.oid == commit.oid).then_some(index)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut output_lanes = input_lanes
            .iter()
            .filter(|lane| lane.oid != commit.oid)
            .cloned()
            .collect::<Vec<_>>();
        let mut parent_lanes = Vec::with_capacity(commit.parents.len());

        if let Some(first_parent) = commit.parents.first() {
            let first_parent_lane =
                GraphLane::new(first_parent.clone(), node_color);
            let output_index = current_lane
                .map(|lane| lane.min(output_lanes.len()))
                .unwrap_or(output_lanes.len());
            output_lanes.insert(output_index, first_parent_lane);
            parent_lanes.push(output_index);
        }

        // Additional parents become new pipes at the right. If a parent is
        // already represented by another active lane, carry that lane's
        // color into the new pipe so the convergence remains readable.
        for parent in commit.parents.iter().skip(1) {
            let color = input_lanes
                .iter()
                .find(|lane| lane.oid == *parent)
                .map(|lane| lane.color_index)
                .unwrap_or_else(|| colors.allocate());
            output_lanes.push(GraphLane::new(parent.clone(), color));
            parent_lanes.push(output_lanes.len() - 1);
        }

        let is_head = refs_of(&commit.decorations).any(is_head_ref);
        let lane_count =
            input_lanes.len().max(output_lanes.len()).max(node_lane + 1);
        let is_merge = commit.parents.len() > 1;

        rows.push(GraphRow {
            input_lanes,
            output_lanes: output_lanes.clone(),
            parent_lanes,
            node_lane,
            lane_count,
            node_color,
            node_input_lanes: node_input_lanes.clone(),
            has_incoming: !node_input_lanes.is_empty(),
            is_head,
            is_merge,
        });
        active_lanes = output_lanes;
    }

    rows
}

/// Format a commit timestamp relative to `now`.
pub fn format_relative_time(
    timestamp: i64,
    now: i64,
    locale: crate::core::i18n::Locale,
) -> String {
    use crate::core::i18n::{text, text_args};

    let minutes = ((now - timestamp).max(0) / 60) as i64;
    let number = |value: i64| value.to_string();

    if minutes < 1 {
        text(locale, "rel-now")
    } else if minutes < 60 {
        text_args(locale, "rel-min", &[("n", &number(minutes))])
    } else if minutes < 60 * 24 {
        text_args(locale, "rel-hour", &[("n", &number(minutes / 60))])
    } else if minutes < 60 * 24 * 7 {
        text_args(locale, "rel-day", &[("n", &number(minutes / (60 * 24)))])
    } else if minutes < 60 * 24 * 30 {
        text_args(
            locale,
            "rel-week",
            &[("n", &number(minutes / (60 * 24 * 7)))],
        )
    } else if minutes < 60 * 24 * 365 {
        text_args(
            locale,
            "rel-month",
            &[("n", &number(minutes / (60 * 24 * 30)))],
        )
    } else {
        text_args(
            locale,
            "rel-year",
            &[("n", &number(minutes / (60 * 24 * 365)))],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_commit(oid_byte: u8, parents: &[u8], decorations: &str) -> LogRow {
        LogRow {
            oid: format!("{oid_byte:040x}"),
            short: format!("{oid_byte:07x}"),
            author: "Test".into(),
            date: "2026-01-01 00:00".into(),
            timestamp: 0,
            subject: format!("Commit {oid_byte}"),
            decorations: decorations.into(),
            parents: parents.iter().map(|b| format!("{b:040x}")).collect(),
        }
    }

    fn lane_oids(lanes: &[GraphLane]) -> Vec<&str> {
        lanes.iter().map(|lane| lane.oid.as_str()).collect()
    }

    #[test]
    fn linear_history_stays_on_one_lane() {
        let commits = vec![
            make_commit(1, &[2], "HEAD -> main"),
            make_commit(2, &[3], ""),
            make_commit(3, &[], ""),
        ];
        let rows = compute_graph(&commits);

        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.node_lane == 0));
        assert_eq!(
            lane_oids(&rows[0].output_lanes),
            vec!["0000000000000000000000000000000000000002"]
        );
        assert!(rows[0].is_head);
        assert!(!rows[0].is_merge);
    }

    #[test]
    fn unrelated_branch_tip_is_appended_without_main_special_case() {
        let commits = vec![
            make_commit(3, &[1], "feature"),
            make_commit(2, &[1], "HEAD -> main"),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);

        assert_eq!(rows[0].node_lane, 0);
        assert!(!rows[0].has_incoming);
        assert_eq!(rows[1].node_lane, 1);
        assert!(!rows[1].has_incoming);
        assert_eq!(rows[2].node_lane, 0);
        assert!(rows[2].has_incoming);
        assert_ne!(rows[0].node_color, rows[1].node_color);
    }

    #[test]
    fn merge_keeps_first_parent_and_appends_secondary_parent() {
        let commits = vec![
            make_commit(4, &[2, 3], "HEAD -> main"),
            make_commit(3, &[2], "feature"),
            make_commit(2, &[1], ""),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);

        assert_eq!(rows[0].node_lane, 0);
        assert_eq!(rows[0].parent_lanes, vec![0, 1]);
        assert_eq!(
            lane_oids(&rows[0].output_lanes),
            vec![
                "0000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000003",
            ]
        );
        assert_eq!(rows[1].node_lane, 1);
        assert_eq!(rows[1].node_input_lanes, vec![1]);
        assert_eq!(rows[0].output_lanes[1].color_index, rows[1].node_color);
        assert_eq!(
            rows[1].output_lanes[1].color_index,
            rows[2].input_lanes[1].color_index
        );
        assert_eq!(rows[2].input_lanes.len(), 2);
        assert_eq!(rows[2].node_lane, 0);
        assert!(rows[0].is_merge);
    }

    #[test]
    fn converging_lanes_are_consumed_at_the_shared_parent() {
        let commits = vec![
            make_commit(4, &[3, 2], "HEAD -> main"),
            make_commit(3, &[1], "feature"),
            make_commit(2, &[1], ""),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);

        assert_eq!(rows[2].node_lane, 1);
        assert_eq!(rows[2].parent_lanes, vec![1]);
        assert_eq!(
            lane_oids(&rows[2].output_lanes),
            vec![
                "0000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000001",
            ]
        );
        assert_eq!(rows[3].node_input_lanes, vec![0, 1]);
        assert_eq!(rows[3].output_lanes.len(), 0);
    }

    #[test]
    fn octopus_merge_preserves_parent_order() {
        let commits = vec![make_commit(9, &[1, 2, 3], "HEAD -> main")];
        let rows = compute_graph(&commits);

        assert_eq!(rows[0].parent_lanes, vec![0, 1, 2]);
        assert_eq!(
            lane_oids(&rows[0].output_lanes),
            vec![
                "0000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000003",
            ]
        );
    }

    #[test]
    fn root_commit_removes_its_lane_and_compacts_following_lanes() {
        let commits = vec![
            make_commit(4, &[3, 2], "HEAD -> main"),
            make_commit(3, &[], ""),
            make_commit(2, &[], ""),
        ];
        let rows = compute_graph(&commits);

        assert_eq!(rows[0].output_lanes.len(), 2);
        assert_eq!(rows[1].node_lane, 0);
        assert_eq!(
            lane_oids(&rows[1].output_lanes),
            vec!["0000000000000000000000000000000000000002",]
        );
        assert_eq!(rows[2].node_lane, 0);
        assert_eq!(rows[2].output_lanes.len(), 0);
    }

    #[test]
    fn missing_parent_remains_a_safe_active_lane() {
        let rows = compute_graph(&[make_commit(1, &[99], "HEAD")]);

        assert_eq!(rows[0].parent_lanes, vec![0]);
        assert_eq!(rows[0].lane_count, 1);
        assert_eq!(rows[0].output_lanes[0].oid, format!("{:040x}", 99));
    }

    #[test]
    fn current_history_includes_tracked_upstream_but_excludes_unrelated_refs() {
        let commits = vec![
            make_commit(5, &[1], "HEAD -> feature"),
            make_commit(4, &[2], "other"),
            make_commit(3, &[1], "origin/feature"),
            make_commit(2, &[], ""),
            make_commit(1, &[], ""),
        ];
        let rows = filter_log_rows(
            &commits,
            GraphHistoryPreference::CurrentBranch,
            "feature",
            Some("origin/feature"),
        );

        assert_eq!(
            rows.iter()
                .map(|row| row.oid[39..].to_string())
                .collect::<Vec<_>>(),
            vec!["5", "3", "1"]
        );
    }

    #[test]
    fn all_history_preserves_unrelated_refs() {
        let commits = vec![
            make_commit(2, &[1], "HEAD -> main"),
            make_commit(3, &[4], "topic"),
            make_commit(1, &[], ""),
            make_commit(4, &[], ""),
        ];
        let rows = filter_log_rows(
            &commits,
            GraphHistoryPreference::AllBranches,
            "main",
            None,
        );

        assert_eq!(rows.len(), commits.len());
    }

    #[test]
    fn topology_colors_follow_lanes_not_authors() {
        let mut commits = vec![
            make_commit(1, &[2], "HEAD -> main"),
            make_commit(2, &[3], ""),
            make_commit(3, &[], ""),
        ];
        commits[0].author = "Alice".into();
        commits[1].author = "Bob".into();
        commits[2].author = "Carol".into();

        let rows = compute_graph(&commits);
        assert!(rows.iter().all(|row| row.node_color == 0));
    }

    #[test]
    fn relative_time_format() {
        use crate::core::i18n::Locale;

        let now = 1_800_000_000i64;
        assert_eq!(format_relative_time(now, now, Locale::English), "just now");
        assert_eq!(
            format_relative_time(now - 5 * 60, now, Locale::English),
            "5min ago"
        );
        assert_eq!(
            format_relative_time(
                now - 24 * 3600,
                now,
                Locale::SimplifiedChinese
            ),
            "1 天前"
        );
        assert_eq!(
            format_relative_time(now + 100, now, Locale::English),
            "just now"
        );
    }

    #[test]
    fn column_visibility_is_monotonic() {
        let tree_w = 44.0;
        let (t1, t2) = (tree_w + 480.0, tree_w + 332.0);

        assert_eq!(column_visibility(t1, tree_w), (true, true));
        assert_eq!(column_visibility(t1 - 0.1, tree_w), (false, true));
        assert_eq!(column_visibility(t2 - 0.1, tree_w), (false, false));
        for width in [t2, t1 - 1.0, t1] {
            let (author, message) = column_visibility(width, tree_w);
            let (previous_author, previous_message) =
                column_visibility(width - 1.0, tree_w);
            assert!(author >= previous_author && message >= previous_message);
        }
    }
}
