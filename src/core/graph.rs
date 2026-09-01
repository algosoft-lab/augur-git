//! Commit graph layout and relative-time formatting.
//!
//! The graph layout follows the active-lane model used by VS Code and
//! LazyGit. Each row snapshots the lanes entering the commit, then produces
//! the lanes leaving it. This keeps the layout driven by commit topology
//! rather than branch names or inferred ancestry.
//!
//! History scoping happens in the Git worker's paged log query
//! (`crate::core::git::commit_log`), not here: every row passed to
//! `compute_graph` is displayed.

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
    /// Complete raw commit message, including the subject and body.
    pub message: String,
    /// Ref decorations such as `HEAD -> main, origin/main`.
    pub decorations: String,
    /// Parent object ids in Git's first-parent order.
    pub parents: Vec<String>,
}

/// The kind of ref behind a commit decoration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    /// The `HEAD` marker (including `HEAD -> branch`).
    Head,
    /// A local branch such as `main` or `feature/x`.
    LocalBranch,
    /// A remote-tracking branch such as `origin/main`.
    RemoteBranch,
    /// An annotated or lightweight tag.
    Tag,
}

/// A single ref label displayed next to a commit in the graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
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

/// Parse `%D` ref decorations into display labels, VS Code style.
///
/// `remote_names` lists configured remotes (e.g. `origin`) so a local branch
/// whose name contains a slash is not mistaken for a remote-tracking branch.
/// When the remote list is unavailable, any slashed name is treated as a
/// remote-tracking branch. `origin/HEAD` alias refs are omitted because they
/// only duplicate their symbolic target.
pub fn parse_ref_labels(
    decorations: &str,
    remote_names: &[String],
) -> Vec<RefLabel> {
    let mut labels = Vec::new();
    for reference in refs_of(decorations) {
        if reference == "HEAD" {
            labels.push(RefLabel {
                name: "HEAD".to_string(),
                kind: RefKind::Head,
            });
            continue;
        }
        if let Some(target) = reference.strip_prefix("HEAD -> ") {
            labels.push(RefLabel {
                name: "HEAD".to_string(),
                kind: RefKind::Head,
            });
            push_branch_label(&mut labels, target, remote_names);
            continue;
        }
        if let Some(tag) = reference.strip_prefix("tag: ") {
            labels.push(RefLabel {
                name: tag.to_string(),
                kind: RefKind::Tag,
            });
            continue;
        }
        push_branch_label(&mut labels, reference, remote_names);
    }
    labels
}

fn push_branch_label(
    labels: &mut Vec<RefLabel>,
    reference: &str,
    remote_names: &[String],
) {
    let name = reference.strip_prefix("remotes/").unwrap_or(reference);
    if name.ends_with("/HEAD") {
        return;
    }
    let is_remote = match name.split_once('/') {
        Some((prefix, _)) => {
            if remote_names.is_empty() {
                true
            } else {
                remote_names.iter().any(|remote| remote == prefix)
            }
        }
        None => false,
    };
    labels.push(RefLabel {
        name: name.to_string(),
        kind: if is_remote {
            RefKind::RemoteBranch
        } else {
            RefKind::LocalBranch
        },
    });
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
            message: format!("Commit {oid_byte}"),
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
    fn diverged_upstream_lane_converges_at_the_merge_base() {
        // Local chain 6->5->2 and remote-only commit 4 (origin/main tip)
        // share the merge base 2, matching a fetch-diverged branch.
        let commits = vec![
            make_commit(6, &[5], "HEAD -> main"),
            make_commit(5, &[2], ""),
            make_commit(4, &[2], "origin/main"),
            make_commit(2, &[1], ""),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);

        assert_eq!(rows[0].node_lane, 0);
        assert_eq!(rows[2].node_lane, 1);
        assert!(!rows[2].has_incoming);
        assert_eq!(rows[3].node_input_lanes, vec![0, 1]);
        assert_ne!(rows[0].node_color, rows[2].node_color);
    }

    #[test]
    fn ref_labels_head_and_remote_are_classified() {
        let labels = parse_ref_labels(
            "HEAD -> main, origin/main, origin/HEAD, tag: v1.0",
            &["origin".to_string()],
        );

        assert_eq!(
            labels,
            vec![
                RefLabel {
                    name: "HEAD".into(),
                    kind: RefKind::Head
                },
                RefLabel {
                    name: "main".into(),
                    kind: RefKind::LocalBranch
                },
                RefLabel {
                    name: "origin/main".into(),
                    kind: RefKind::RemoteBranch
                },
                RefLabel {
                    name: "v1.0".into(),
                    kind: RefKind::Tag
                },
            ]
        );
    }

    #[test]
    fn ref_labels_slashed_local_branch_is_not_mistaken_for_remote() {
        let labels = parse_ref_labels(
            "feature/x, remotes/origin/topic, HEAD",
            &["origin".to_string()],
        );

        assert_eq!(
            labels,
            vec![
                RefLabel {
                    name: "feature/x".into(),
                    kind: RefKind::LocalBranch
                },
                RefLabel {
                    name: "origin/topic".into(),
                    kind: RefKind::RemoteBranch
                },
                RefLabel {
                    name: "HEAD".into(),
                    kind: RefKind::Head
                },
            ]
        );
    }

    #[test]
    fn ref_labels_without_known_remotes_treats_slashes_as_remotes() {
        let labels = parse_ref_labels("origin/main", &[]);
        assert_eq!(
            labels,
            vec![RefLabel {
                name: "origin/main".into(),
                kind: RefKind::RemoteBranch
            }]
        );
        assert!(parse_ref_labels("", &[]).is_empty());
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
