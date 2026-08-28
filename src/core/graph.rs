//! Commit graph layout algorithm (ported from
//! `crates/rgitui_git/src/graph.rs` in rgitui).
//!
//! `compute_graph` assigns lanes and generates the edges between rows:
//! - the main/master chain stays on lane 0 and feature branches use other lanes;
//! - new branch tips get a lane and color, while free lanes are compacted inward;
//! - colors follow topology lanes rather than commit authors;
//! - memoized ancestry sets avoid walking the parent DAG for every query.
//!
//! Unlike rgitui, oids are `String`s (there is no git2 dependency) and refs are
//! identified from the `%D` decoration string.
//!
//! `GraphView` (in `git/graph.rs`) consumes the lane, edge, and color data.
//! `LogRow.graph` (the `git log --graph` ASCII output) remains available for
//! debugging and fallback rendering.

#![allow(dead_code)]

/// 一行提交（git log 解析产物，含 parents 供图算法使用）
#[derive(Clone, Debug)]
pub struct LogRow {
    /// lane/连线前缀字符（git log --graph 输出，等宽字体渲染）
    pub graph: String,
    /// 完整 oid（40 hex）
    pub oid: String,
    /// 短 oid（7 hex）
    pub short: String,
    pub author: String,
    /// 已格式化的提交时间（git --date=format）
    pub date: String,
    /// 作者时间戳（unix 秒，相对时间显示用）
    pub timestamp: i64,
    pub subject: String,
    /// 引用装饰（"HEAD -> main, origin/main"）
    pub decorations: String,
    /// 父提交 oid 列表（git log %P，空格分隔）
    pub parents: Vec<String>,
}

/// 提交在图中的位置（一行）
#[derive(Debug, Clone)]
pub struct GraphRow {
    /// 本提交节点所在 lane
    pub node_lane: usize,
    /// 本行绘制的连接边
    pub edges: Vec<GraphEdge>,
    /// 本行活跃 lane 总数
    pub lane_count: usize,
    /// Node color index carried by the node's topology lane.
    pub node_color: usize,
    /// 是否从上一行有入边（分支尖端为 false）
    pub has_incoming: bool,
    /// 是否 HEAD 提交
    pub is_head: bool,
    /// 是否 merge 提交（多于一个 parent）
    pub is_merge: bool,
}

/// 连接两行的边
#[derive(Debug, Clone)]
pub struct GraphEdge {
    /// 起点 lane（上一行）
    pub from_lane: usize,
    /// 终点 lane（本行）
    pub to_lane: usize,
    /// Color index carried by this topology edge.
    pub color_index: usize,
    /// 是否 merge 边（连向非主 parent）
    pub is_merge: bool,
}

// ===== 提交图列宽常量（行与列头共用，column_visibility 阈值推导同源） =====

/// Hash 列宽（短 oid 7 字符等宽）
pub const HASH_COL_WIDTH: f32 = 60.0;
/// Author 列宽（镜像 rgitui 默认 140；定宽，超长省略号）
pub const AUTHOR_COL_WIDTH: f32 = 140.0;
/// Date 列宽（相对时间）
pub const DATE_COL_WIDTH: f32 = 120.0;
/// Message 列最低可用宽：低于此值宁可整列隐藏也不硬塞
pub const MESSAGE_MIN_WIDTH: f32 = 120.0;
/// 行/列头子元素间隙（gap_2 = 8px）与右侧留白（pr_2）
const COL_GAP: f32 = 8.0;
const ROW_PAD_RIGHT: f32 = 8.0;

/// 响应式列显隐：按 GraphView 可用总宽返回 (显示 Author, 显示 Message)
///
/// 单调两档阶梯（无迟滞，不会在阈值附近抖动）：
/// - 宽 ≥ T1：五列全显（Message 剩余空间 ≥ MESSAGE_MIN_WIDTH）
/// - T2 ≤ 宽 < T1：藏 Author（140px 让给 Message）
/// - 宽 < T2：连 Message 一起藏，剩 Graph | Hash | Date（Date 永不藏）
///
/// 阈值 = 树列宽 + Hash/Date 定宽 +（Author 定宽）+ 间隙×(列数-1) + 右留白
/// + MESSAGE_MIN_WIDTH；间隙数随藏列减少，公式与行内 flex 布局严格对应。
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

/// 记忆化的祖先可达性（照抄 rgitui：per-descendant 祖先集合，按需计算并复用）
struct AncestorCache {
    sets: std::collections::HashMap<String, std::collections::HashSet<String>>,
}

impl AncestorCache {
    fn new() -> Self {
        Self {
            sets: std::collections::HashMap::new(),
        }
    }

    /// ancestor 是否为 descendant 的祖先（自反：自己算自己祖先）
    fn is_ancestor_of(
        &mut self,
        ancestor: &str,
        descendant: &str,
        commits: &[LogRow],
        oid_to_idx: &std::collections::HashMap<String, usize>,
    ) -> bool {
        if ancestor == descendant {
            return true;
        }
        self.proper_ancestors(descendant, commits, oid_to_idx)
            .contains(ancestor)
    }

    fn proper_ancestors(
        &mut self,
        descendant: &str,
        commits: &[LogRow],
        oid_to_idx: &std::collections::HashMap<String, usize>,
    ) -> &std::collections::HashSet<String> {
        self.sets.entry(descendant.to_string()).or_insert_with(|| {
            let mut ancestors = std::collections::HashSet::new();
            let mut queue = match oid_to_idx.get(descendant) {
                Some(&idx) => commits[idx].parents.clone(),
                None => Vec::new(),
            };
            while let Some(current) = queue.pop() {
                if ancestors.insert(current.clone()) {
                    if let Some(&idx) = oid_to_idx.get(&current) {
                        queue.extend(commits[idx].parents.iter().cloned());
                    }
                }
            }
            ancestors
        })
    }
}

/// 装饰字符串 → 引用列表（"HEAD -> main, origin/main" → ["HEAD -> main", "origin/main"]）
fn refs_of(decorations: &str) -> Vec<&str> {
    if decorations.is_empty() {
        Vec::new()
    } else {
        decorations.split(", ").collect()
    }
}

fn is_head_ref(r: &str) -> bool {
    r == "HEAD" || r.starts_with("HEAD ->")
}

fn is_main_ref(r: &str) -> bool {
    r == "main"
        || r == "master"
        || r.ends_with("/main")
        || r.ends_with("/master")
}

/// 找 main/master 分支尖端 oid（提交按拓扑序，第一个匹配的 ref 最靠前）
fn find_main_branch_tip(commits: &[LogRow]) -> Option<String> {
    let mut main_tip: Option<String> = None;
    let mut master_tip: Option<String> = None;

    for commit in commits {
        for r in refs_of(&commit.decorations) {
            if is_main_ref(r) && main_tip.is_none() {
                main_tip = Some(commit.oid.clone());
            }
            if (r == "master" || r.ends_with("/master")) && master_tip.is_none()
            {
                master_tip = Some(commit.oid.clone());
            }
        }
    }

    main_tip.or(master_tip)
}

/// 计算 main 分支第一父链（merge 时选最可能是主线的 parent）
fn compute_main_chain(
    main_tip: &str,
    commits: &[LogRow],
    oid_to_idx: &std::collections::HashMap<String, usize>,
) -> std::collections::HashSet<String> {
    let mut chain = std::collections::HashSet::new();
    let mut current = Some(main_tip.to_string());
    while let Some(oid) = current {
        chain.insert(oid.clone());
        current = oid_to_idx.get(&oid).and_then(|&idx| {
            let commit = &commits[idx];
            if commit.parents.len() <= 1 {
                commit.parents.first().cloned()
            } else {
                pick_main_parent(&commit.parents, commits, oid_to_idx)
            }
        });
    }
    chain
}

/// merge 提交里选主 parent：优先没有功能分支 ref 指向的 parent
fn pick_main_parent(
    parents: &[String],
    commits: &[LogRow],
    oid_to_idx: &std::collections::HashMap<String, usize>,
) -> Option<String> {
    let mut non_feature_parent = None;
    for parent_oid in parents {
        let has_feature_ref = oid_to_idx.get(parent_oid).is_some_and(|idx| {
            commits[*idx].decorations.split(", ").any(|r| {
                r != "main"
                    && r != "master"
                    && !r.ends_with("/main")
                    && !r.ends_with("/master")
            })
        });
        if !has_feature_ref && non_feature_parent.is_none() {
            non_feature_parent = Some(parent_oid.clone());
        }
    }
    non_feature_parent.or_else(|| parents.first().cloned())
}

const MAIN_COLOR_INDEX: usize = 0;

/// A topology lane that is waiting for its expected parent commit.
#[derive(Debug, Clone)]
struct ActiveLane {
    expected_oid: String,
    color_index: usize,
}

impl ActiveLane {
    fn new(expected_oid: String, color_index: usize) -> Self {
        Self {
            expected_oid,
            color_index,
        }
    }
}

/// Allocates deterministic color identities for newly created topology lines.
///
/// Color identities are intentionally independent of the numeric lane column.
/// The renderer maps them onto the active theme palette, so a lane can move or
/// be reused without changing the identity carried by an existing line.
struct ColorAllocator {
    next: usize,
}

impl ColorAllocator {
    fn new(main_color_reserved: bool) -> Self {
        Self {
            next: usize::from(main_color_reserved),
        }
    }

    fn allocate(&mut self) -> usize {
        let color = self.next;
        self.next = self.next.saturating_add(1);
        color
    }
}

/// Compute the commit graph layout using topology-driven colors.
pub fn compute_graph(commits: &[LogRow]) -> Vec<GraphRow> {
    if commits.is_empty() {
        return Vec::new();
    }

    let oid_to_idx: std::collections::HashMap<String, usize> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.oid.clone(), i))
        .collect();

    let mut ancestry = AncestorCache::new();

    // Find HEAD across all commits; a remote branch may be ahead of it.
    let head_oid = commits
        .iter()
        .find(|c| refs_of(&c.decorations).iter().any(|r| is_head_ref(r)))
        .map(|c| c.oid.clone());

    // Build the main branch's first-parent chain so it stays on lane 0.
    let main_tip = find_main_branch_tip(commits);
    let main_chain: std::collections::HashSet<String> = match &main_tip {
        Some(tip) => compute_main_chain(tip, commits, &oid_to_idx),
        None => head_oid
            .as_ref()
            .map(|head| compute_main_chain(head, commits, &oid_to_idx))
            .unwrap_or_default(),
    };

    // Reserve the primary color whenever a main chain can be identified.
    // This keeps feature colors out of the main color even when their tips are
    // listed before the main tip or HEAD in the log.
    let mut color_allocator = ColorAllocator::new(!main_chain.is_empty());

    // Active lanes carry both the expected oid and their topology color.
    let mut lanes: Vec<Option<ActiveLane>> = Vec::new();

    // Reserve lane 0 when the main tip is not the first listed commit, which
    // prevents the main line from bending when that tip is reached.
    let reserve_lane_0 = main_tip
        .as_ref()
        .is_some_and(|tip| commits.first().is_some_and(|c| &c.oid != tip));
    if reserve_lane_0 {
        if let Some(tip) = &main_tip {
            lanes.push(Some(ActiveLane::new(tip.clone(), MAIN_COLOR_INDEX)));
        }
    }

    let mut rows = Vec::with_capacity(commits.len());

    for (_idx, commit) in commits.iter().enumerate() {
        let oid = &commit.oid;
        let is_merge = commit.parents.len() > 1;
        let is_head = head_oid.as_ref() == Some(oid);
        let on_main = main_chain.contains(oid);
        let (node_lane, has_incoming) = if on_main {
            if matches!(lanes.first(), Some(Some(lane)) if lane.expected_oid == *oid)
            {
                (0, true)
            } else if matches!(lanes.first(), Some(Some(lane)) if ancestry.is_ancestor_of(oid, &lane.expected_oid, commits, &oid_to_idx))
            {
                let color = lanes[0]
                    .as_ref()
                    .map(|lane| lane.color_index)
                    .unwrap_or(MAIN_COLOR_INDEX);
                lanes[0] = Some(ActiveLane::new(oid.clone(), color));
                (0, true)
            } else if matches!(lanes.first(), Some(None)) || lanes.is_empty() {
                if lanes.is_empty() {
                    lanes.push(None);
                }
                lanes[0] = Some(ActiveLane::new(oid.clone(), MAIN_COLOR_INDEX));
                (0, false)
            } else if matches!(lanes.first(), Some(Some(lane)) if main_chain.contains(&lane.expected_oid))
            {
                let color = lanes[0]
                    .as_ref()
                    .map(|lane| lane.color_index)
                    .unwrap_or(MAIN_COLOR_INDEX);
                lanes[0] = Some(ActiveLane::new(oid.clone(), color));
                (0, true)
            } else {
                find_lane(
                    oid,
                    &mut lanes,
                    &mut color_allocator,
                    commits,
                    &oid_to_idx,
                    &mut ancestry,
                    None,
                )
            }
        } else {
            find_lane(
                oid,
                &mut lanes,
                &mut color_allocator,
                commits,
                &oid_to_idx,
                &mut ancestry,
                Some(0),
            )
        };

        let node_color = lanes[node_lane]
            .as_ref()
            .map(|lane| lane.color_index)
            .unwrap_or(MAIN_COLOR_INDEX);

        // Draw through edges for other active lanes and convergence edges for
        // stale lanes that are waiting for this commit.
        let mut edges = Vec::new();
        for (lane, slot) in lanes.iter_mut().enumerate() {
            if lane == node_lane {
                continue;
            }
            if let Some(active_lane) = slot {
                if active_lane.expected_oid == *oid {
                    edges.push(GraphEdge {
                        from_lane: lane,
                        to_lane: node_lane,
                        color_index: active_lane.color_index,
                        is_merge: false,
                    });
                    *slot = None;
                } else {
                    edges.push(GraphEdge {
                        from_lane: lane,
                        to_lane: lane,
                        color_index: active_lane.color_index,
                        is_merge: false,
                    });
                }
            }
        }

        // Release the current lane before routing its parents.
        lanes[node_lane] = None;

        // Remove other lanes waiting for the same oid and turn them into
        // convergence edges.
        for (lane_idx, lane) in lanes.iter_mut().enumerate() {
            if matches!(lane, Some(active_lane) if active_lane.expected_oid == *oid)
            {
                let color = lane
                    .as_ref()
                    .map(|active_lane| active_lane.color_index)
                    .unwrap_or(MAIN_COLOR_INDEX);
                *lane = None;
                if let Some(edge) = edges
                    .iter_mut()
                    .find(|e| e.from_lane == lane_idx && e.to_lane == lane_idx)
                {
                    edge.to_lane = node_lane;
                }
                if !edges
                    .iter()
                    .any(|e| e.from_lane == lane_idx && e.to_lane == node_lane)
                {
                    edges.push(GraphEdge {
                        from_lane: lane_idx,
                        to_lane: node_lane,
                        color_index: color,
                        is_merge: false,
                    });
                }
            }
        }

        // Route parents. For a main-chain merge, keep the main-chain parent as
        // the primary parent so lane 0 remains the visual main line.
        if !commit.parents.is_empty() {
            let (primary, secondaries) = if on_main && commit.parents.len() > 1
            {
                if let Some(main_parent_pos) =
                    commit.parents.iter().position(|p| main_chain.contains(p))
                {
                    let primary = commit.parents[main_parent_pos].clone();
                    let secondaries: Vec<String> = commit
                        .parents
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| i != main_parent_pos)
                        .map(|(_, p)| p.clone())
                        .collect();
                    (primary, secondaries)
                } else {
                    (commit.parents[0].clone(), commit.parents[1..].to_vec())
                }
            } else {
                (commit.parents[0].clone(), commit.parents[1..].to_vec())
            };

            let primary_on_main = on_main && main_chain.contains(&primary);
            let primary_lane = if primary_on_main {
                if matches!(lanes.first(), Some(Some(lane)) if lane.expected_oid == primary)
                {
                    0
                } else if matches!(lanes.first(), Some(None))
                    || lanes.is_empty()
                {
                    if lanes.is_empty() {
                        lanes.push(None);
                    }
                    lanes[0] = Some(ActiveLane::new(
                        primary.clone(),
                        MAIN_COLOR_INDEX,
                    ));
                    0
                } else if node_lane == 0 {
                    lanes[0] =
                        Some(ActiveLane::new(primary.clone(), node_color));
                    0
                } else {
                    route_primary_parent(
                        &primary,
                        node_lane,
                        node_color,
                        &mut lanes,
                        commits,
                        &oid_to_idx,
                        &mut ancestry,
                    )
                }
            } else {
                route_primary_parent(
                    &primary,
                    node_lane,
                    node_color,
                    &mut lanes,
                    commits,
                    &oid_to_idx,
                    &mut ancestry,
                )
            };

            edges.push(GraphEdge {
                from_lane: node_lane,
                to_lane: primary_lane,
                color_index: node_color,
                is_merge: false,
            });

            for parent in &secondaries {
                let (parent_lane, parent_color) = route_secondary_parent(
                    parent,
                    node_lane,
                    &mut lanes,
                    commits,
                    &oid_to_idx,
                    &mut ancestry,
                    &mut color_allocator,
                );
                // A merge edge carries the color of the secondary topology
                // line, not the color of the merge commit.
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: parent_lane,
                    color_index: parent_color,
                    is_merge: true,
                });
            }
        }

        // Compact trailing empty lanes.
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }

        let lane_count = lanes.len().max(node_lane + 1);

        rows.push(GraphRow {
            node_lane,
            edges,
            lane_count,
            node_color,
            has_incoming,
            is_head,
            is_merge,
        });
    }

    // Strip pure lane-0 through-lines above a reserved main tip. These lines
    // are artifacts of reserving the lane before the tip is reached.
    if reserve_lane_0 {
        if let Some(tip) = &main_tip {
            if let Some(&tip_idx) = oid_to_idx.get(tip) {
                if tip_idx > 0 {
                    let strip_until = rows
                        .iter()
                        .take(tip_idx)
                        .position(|r| {
                            r.edges
                                .iter()
                                .any(|e| e.to_lane == 0 && e.from_lane != 0)
                        })
                        .map(|i| (i + 1).min(tip_idx))
                        .unwrap_or(tip_idx);

                    let mut last_stripped = None;
                    for (idx, row) in
                        rows.iter_mut().enumerate().take(strip_until)
                    {
                        let before = row.edges.len();
                        row.edges
                            .retain(|e| !(e.from_lane == 0 && e.to_lane == 0));
                        if row.edges.len() != before {
                            last_stripped = Some(idx);
                        }
                    }
                    if last_stripped == Some(tip_idx - 1) {
                        if let Some(tip_row) = rows.get_mut(tip_idx) {
                            if tip_row.node_lane == 0 {
                                tip_row.has_incoming = false;
                            }
                        }
                    }
                }
            }
        }
    }

    rows
}

/// Find the lane for a commit: exact oid, ancestor, then a new lane.
fn find_lane(
    oid: &str,
    lanes: &mut Vec<Option<ActiveLane>>,
    colors: &mut ColorAllocator,
    commits: &[LogRow],
    oid_to_idx: &std::collections::HashMap<String, usize>,
    ancestry: &mut AncestorCache,
    skip_lane: Option<usize>,
) -> (usize, bool) {
    if let Some(pos) = lanes.iter().enumerate().position(|(i, s)| {
        Some(i) != skip_lane
            && matches!(s, Some(lane) if lane.expected_oid == oid)
    }) {
        return (pos, true);
    }

    if let Some(pos) = lanes.iter().enumerate().position(|(i, s)| {
        Some(i) != skip_lane
            && matches!(s, Some(lane) if ancestry.is_ancestor_of(oid, &lane.expected_oid, commits, oid_to_idx))
    }) {
        let color = lanes[pos]
            .as_ref()
            .map(|lane| lane.color_index)
            .unwrap_or_else(|| colors.allocate());
        lanes[pos] = Some(ActiveLane::new(oid.to_string(), color));
        return (pos, true);
    }

    let pos = alloc_lane(lanes, skip_lane);
    lanes[pos] = Some(ActiveLane::new(oid.to_string(), colors.allocate()));
    (pos, false)
}

/// Route a primary parent, carrying the current node's color forward.
fn route_primary_parent(
    parent: &str,
    node_lane: usize,
    node_color: usize,
    lanes: &mut Vec<Option<ActiveLane>>,
    commits: &[LogRow],
    oid_to_idx: &std::collections::HashMap<String, usize>,
    ancestry: &mut AncestorCache,
) -> usize {
    if let Some(target) = lanes
        .iter()
        .position(|s| matches!(s, Some(lane) if lane.expected_oid == parent))
    {
        return target;
    }

    if lanes.get(node_lane).is_some_and(Option::is_none) {
        lanes[node_lane] =
            Some(ActiveLane::new(parent.to_string(), node_color));
        return node_lane;
    }

    if let Some(target) = lanes.iter().position(|s| {
        matches!(s, Some(lane) if ancestry.is_ancestor_of(parent, &lane.expected_oid, commits, oid_to_idx))
    }) {
        return target;
    }

    let pos = alloc_lane(lanes, None);
    lanes[pos] = Some(ActiveLane::new(parent.to_string(), node_color));
    pos
}

/// Route a secondary parent, preserving an existing lane color or allocating a
/// new topology color. The current node lane is excluded so a merge branch
/// cannot collapse into the primary line.
fn route_secondary_parent(
    parent: &str,
    node_lane: usize,
    lanes: &mut Vec<Option<ActiveLane>>,
    commits: &[LogRow],
    oid_to_idx: &std::collections::HashMap<String, usize>,
    ancestry: &mut AncestorCache,
    colors: &mut ColorAllocator,
) -> (usize, usize) {
    if let Some(target) = lanes
        .iter()
        .position(|s| matches!(s, Some(lane) if lane.expected_oid == parent))
    {
        let color = lanes[target]
            .as_ref()
            .map(|lane| lane.color_index)
            .unwrap_or(MAIN_COLOR_INDEX);
        return (target, color);
    }

    if let Some(target) = lanes.iter().position(|s| {
        matches!(s, Some(lane) if ancestry.is_ancestor_of(parent, &lane.expected_oid, commits, oid_to_idx))
    }) {
        let color = lanes[target]
            .as_ref()
            .map(|lane| lane.color_index)
            .unwrap_or(MAIN_COLOR_INDEX);
        return (target, color);
    }

    let pos = alloc_lane(lanes, Some(node_lane));
    let color = colors.allocate();
    lanes[pos] = Some(ActiveLane::new(parent.to_string(), color));
    (pos, color)
}

/// Find the first free lane, appending one when necessary.
fn alloc_lane(
    lanes: &mut Vec<Option<ActiveLane>>,
    skip_lane: Option<usize>,
) -> usize {
    if let Some(pos) = lanes
        .iter()
        .enumerate()
        .position(|(i, l)| l.is_none() && Some(i) != skip_lane)
    {
        pos
    } else {
        lanes.push(None);
        let pos = lanes.len() - 1;
        if Some(pos) == skip_lane {
            lanes.push(None);
            lanes.len() - 1
        } else {
            pos
        }
    }
}

/// 相对时间（镜像 rgitui format_relative_time；月份用 m、分钟用 min 避免歧义）：
/// en: just now / Xmin ago / Xh ago / Xd ago / Xw ago / Xm ago / Xy ago
/// zh: 刚刚 / X 分钟前 / X 小时前 / X 天前 / X 周前 / X 个月前 / X 年前
pub fn format_relative_time(
    timestamp: i64,
    now: i64,
    locale: crate::core::i18n::Locale,
) -> String {
    use crate::core::i18n::{text, text_args};

    let minutes = ((now - timestamp).max(0) / 60) as i64;
    let n = |v: i64| v.to_string();

    if minutes < 1 {
        text(locale, "rel-now")
    } else if minutes < 60 {
        text_args(locale, "rel-min", &[("n", &n(minutes))])
    } else if minutes < 60 * 24 {
        text_args(locale, "rel-hour", &[("n", &n(minutes / 60))])
    } else if minutes < 60 * 24 * 7 {
        text_args(locale, "rel-day", &[("n", &n(minutes / (60 * 24)))])
    } else if minutes < 60 * 24 * 30 {
        text_args(locale, "rel-week", &[("n", &n(minutes / (60 * 24 * 7)))])
    } else if minutes < 60 * 24 * 365 {
        text_args(locale, "rel-month", &[("n", &n(minutes / (60 * 24 * 30)))])
    } else {
        text_args(locale, "rel-year", &[("n", &n(minutes / (60 * 24 * 365)))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_commit(oid_byte: u8, parents: &[u8], decorations: &str) -> LogRow {
        let oid = format!("{:040x}", oid_byte);
        LogRow {
            graph: String::new(),
            oid,
            short: format!("{:07x}", oid_byte),
            author: "Test".into(),
            date: "2026-01-01 00:00".into(),
            timestamp: 0,
            subject: format!("Commit {oid_byte}"),
            decorations: decorations.into(),
            parents: parents.iter().map(|b| format!("{:040x}", b)).collect(),
        }
    }

    #[test]
    fn linear_history_all_on_lane_zero() {
        let commits = vec![
            make_commit(1, &[2], "HEAD -> main"),
            make_commit(2, &[3], ""),
            make_commit(3, &[], ""),
        ];
        let rows = compute_graph(&commits);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].node_lane, 0);
        assert_eq!(rows[1].node_lane, 0);
        assert_eq!(rows[2].node_lane, 0);
        assert!(rows[0].is_head);
        assert!(!rows[0].is_merge);
    }

    #[test]
    fn merge_commit_gets_second_lane() {
        // main: 1 -> 2 -> 4 (merge); feature: 3 branches from 2 and merges
        // into 4.
        let commits = vec![
            make_commit(4, &[2, 3], "HEAD -> main"),
            make_commit(3, &[2], ""), // feature tip
            make_commit(2, &[1], ""),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);
        // The merge commit stays on the main lane.
        assert_eq!(rows[0].node_lane, 0);
        assert!(rows[0].is_merge);
        // The feature commit is not on lane 0.
        assert_ne!(rows[1].node_lane, 0);
        // A merge edge is present.
        assert!(rows[0].edges.iter().any(|e| e.is_merge));
        assert_eq!(rows[0].edges.iter().filter(|e| e.is_merge).count(), 1);
    }

    #[test]
    fn feature_branch_ahead_of_main_gets_own_lane() {
        // The main tip is listed later, so the feature is ahead of main.
        let commits = vec![
            make_commit(3, &[1], "feature"), // feature tip
            make_commit(2, &[1], "HEAD -> main"), // main is behind
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);
        assert_eq!(rows.len(), 3);
        // The main chain stays on lane 0.
        assert_eq!(rows[1].node_lane, 0);
        assert_eq!(rows[2].node_lane, 0);
        // The feature tip is not on lane 0.
        assert_ne!(rows[0].node_lane, 0);
    }

    #[test]
    fn relative_time_format() {
        // 基准时间固定，逐档验证（含用户示例：昨天 1d / 7 天 1w / 一个月 1m / 一年 1y）
        use crate::core::i18n::Locale;
        let now = 1_800_000_000i64;
        let en = Locale::English;
        let zh = Locale::SimplifiedChinese;
        assert_eq!(format_relative_time(now, now, en), "just now");
        assert_eq!(format_relative_time(now - 30, now, en), "just now");
        assert_eq!(format_relative_time(now - 5 * 60, now, en), "5min ago");
        assert_eq!(format_relative_time(now - 3 * 3600, now, en), "3h ago");
        assert_eq!(format_relative_time(now - 24 * 3600, now, en), "1d ago");
        assert_eq!(format_relative_time(now - 7 * 86400, now, en), "1w ago");
        assert_eq!(format_relative_time(now - 30 * 86400, now, en), "1m ago");
        assert_eq!(format_relative_time(now - 365 * 86400, now, en), "1y ago");
        // 未来时间钳制为 just now
        assert_eq!(format_relative_time(now + 100, now, en), "just now");
        // 中文逐档
        assert_eq!(format_relative_time(now, now, zh), "刚刚");
        assert_eq!(format_relative_time(now - 5 * 60, now, zh), "5 分钟前");
        assert_eq!(format_relative_time(now - 3 * 3600, now, zh), "3 小时前");
        assert_eq!(format_relative_time(now - 24 * 3600, now, zh), "1 天前");
        assert_eq!(format_relative_time(now - 7 * 86400, now, zh), "1 周前");
        assert_eq!(format_relative_time(now - 30 * 86400, now, zh), "1 个月前");
        assert_eq!(format_relative_time(now - 365 * 86400, now, zh), "1 年前");
    }

    #[test]
    fn column_visibility_staircase() {
        // 阈值随树列宽推导：T1 = tree+60+140+120+4*8+8+120 = tree+480
        //               T2 = tree+60+120+3*8+8+120  = tree+332
        let tree_w = 44.0;
        let (t1, t2) = (tree_w + 480.0, tree_w + 332.0);
        // 宽：五列全显
        assert_eq!(column_visibility(t1, tree_w), (true, true));
        assert_eq!(column_visibility(t1 + 0.1, tree_w), (true, true));
        // 中：恰在 T1 之下 → 藏 Author，Message 保住
        assert_eq!(column_visibility(t1 - 0.1, tree_w), (false, true));
        assert_eq!(column_visibility(t2, tree_w), (false, true));
        // 窄：T2 之下 → 连 Message 也藏
        assert_eq!(column_visibility(t2 - 0.1, tree_w), (false, false));
        assert_eq!(column_visibility(0.0, tree_w), (false, false));
        // 阶梯单调：宽度增加不会让列更少
        for w in [t2 - 1.0, t2, t1 - 1.0, t1, t1 + 1.0] {
            let (a2, m2) = column_visibility(w, tree_w);
            let (a1, m1) = column_visibility(w - 1.0, tree_w);
            assert!(a2 >= a1 && m2 >= m1, "monotonic at {w}");
        }
        // 树列越宽（lane 多）阈值越高
        let wide_tree = 200.0;
        assert_eq!(column_visibility(t1, wide_tree), (false, false));
    }

    #[test]
    fn topology_color_ignores_author() {
        // A linear lane keeps one color even when every commit has a
        // different author.
        let mut commits = vec![
            make_commit(1, &[2], "HEAD -> main"),
            make_commit(2, &[3], ""),
            make_commit(3, &[4], ""),
            make_commit(4, &[], ""),
        ];
        commits[0].author = "Alice".into();
        commits[1].author = "Bob".into();
        commits[2].author = "Carol".into();
        commits[3].author = "Dave".into();
        let rows = compute_graph(&commits);
        assert!(rows.iter().all(|row| row.node_color == MAIN_COLOR_INDEX));
        for row in &rows {
            assert!(
                row.edges
                    .iter()
                    .filter(|edge| {
                        edge.from_lane == row.node_lane && !edge.is_merge
                    })
                    .all(|e| e.color_index == row.node_color)
            );
        }
    }

    #[test]
    fn feature_lane_keeps_color_through_merge() {
        // main: 4 -> 2 -> 1; feature: 3 -> 2, merged by 4.
        let commits = vec![
            make_commit(4, &[2, 3], "HEAD -> main"),
            make_commit(3, &[2], "feature"),
            make_commit(2, &[1], ""),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);
        let main_color = rows[0].node_color;
        let merge_edge = rows[0]
            .edges
            .iter()
            .find(|edge| edge.is_merge)
            .expect("merge edge should be present");

        assert_eq!(main_color, MAIN_COLOR_INDEX);
        assert_ne!(merge_edge.color_index, main_color);
        assert_eq!(rows[1].node_color, merge_edge.color_index);
        assert_eq!(rows[2].node_color, main_color);
        assert!(
            rows[1]
                .edges
                .iter()
                .filter(|edge| !edge.is_merge
                    && edge.from_lane == rows[1].node_lane)
                .all(|edge| edge.color_index == rows[1].node_color)
        );
    }

    #[test]
    fn feature_ahead_of_main_uses_distinct_color_and_converges() {
        // The feature tip is listed before the main tip, so the primary color
        // must still be reserved for lane 0.
        let commits = vec![
            make_commit(3, &[1], "feature"),
            make_commit(2, &[1], "HEAD -> main"),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);
        let feature_color = rows[0].node_color;

        assert_eq!(rows[1].node_color, MAIN_COLOR_INDEX);
        assert_ne!(feature_color, MAIN_COLOR_INDEX);
        assert_eq!(rows[2].node_color, MAIN_COLOR_INDEX);
        assert!(rows[2].edges.iter().any(|edge| {
            edge.from_lane != edge.to_lane && edge.color_index == feature_color
        }));
    }
}
