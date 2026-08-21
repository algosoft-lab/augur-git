//! M1：提交树布局算法（从 rgitui 的 `crates/rgitui_git/src/graph.rs` 移植）
//!
//! `compute_graph` 为提交列表分配 lane 并生成行间连接边：
//! - main/master 链始终占 lane 0，功能分支被推到其他 lane
//! - 分支尖端后来出现时分配新 lane；lane 空闲时向内压缩
//! - 颜色按提交者分配：不同提交者不同圈色，连线与所属提交的圈色一致
//! - 祖先可达性用记忆化集合（AncestorCache），避免每次查询重走 parent DAG
//!
//! 与 rgitui 的差异：oid 用 String（无 git2 依赖），refs 从 %D 装饰字符串判断。
//!
//! 由 GraphView（git/graph.rs）消费：lane/边/颜色与 rgitui 一致。
//! LogRow.graph（git log --graph ASCII）保留作调试/兜底。

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
    /// 节点圆点颜色索引（按 lane 颜色）
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
    /// 颜色索引（连线与发出它的提交圈色一致）
    pub color_index: usize,
    /// 是否 merge 边（连向非主 parent）
    pub is_merge: bool,
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
    r == "main" || r == "master" || r.ends_with("/main") || r.ends_with("/master")
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
            if (r == "master" || r.ends_with("/master")) && master_tip.is_none() {
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
                r != "main" && r != "master" && !r.ends_with("/main") && !r.ends_with("/master")
            })
        });
        if !has_feature_ref && non_feature_parent.is_none() {
            non_feature_parent = Some(parent_oid.clone());
        }
    }
    non_feature_parent.or_else(|| parents.first().cloned())
}

/// 计算提交图布局（照抄 rgitui compute_graph，String oid 化）
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

    // HEAD oid（扫描全部提交——远端分支领先时 HEAD 可能不是最新）
    let head_oid = commits
        .iter()
        .find(|c| refs_of(&c.decorations).iter().any(|r| is_head_ref(r)))
        .map(|c| c.oid.clone());

    // main 分支及第一父链（让 main 稳定在 lane 0）
    let main_tip = find_main_branch_tip(commits);
    let main_chain: std::collections::HashSet<String> = match &main_tip {
        Some(tip) => compute_main_chain(tip, commits, &oid_to_idx),
        None => head_oid
            .as_ref()
            .map(|head| compute_main_chain(head, commits, &oid_to_idx))
            .unwrap_or_default(),
    };

    // 提交者 → 颜色索引（首见顺序分配，保证不同提交者不同色）
    let mut author_colors: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut next_author_color: usize = 0;

    // 活跃 lane：(期望到达的 oid, 颜色索引)
    let mut lanes: Vec<Option<(String, usize)>> = Vec::new();

    // main_tip 不是列表首位时预占 lane 0（避免主线在 main_tip 处拐弯）
    let reserve_lane_0 = main_tip
        .as_ref()
        .is_some_and(|tip| commits.first().is_some_and(|c| &c.oid != tip));
    if reserve_lane_0 {
        if let Some(tip) = &main_tip {
            let color = author_color_index(
                &commits[oid_to_idx[tip]].author,
                &mut author_colors,
                &mut next_author_color,
            );
            lanes.push(Some((tip.clone(), color)));
        }
    }

    let mut rows = Vec::with_capacity(commits.len());

    for (_idx, commit) in commits.iter().enumerate() {
        let oid = &commit.oid;
        let is_merge = commit.parents.len() > 1;
        let is_head = head_oid.as_ref() == Some(oid);
        let on_main = main_chain.contains(oid);
        // 圈色 = 提交者颜色；本提交发出的连线（出边/上下段）同色
        let author_idx =
            author_color_index(&commit.author, &mut author_colors, &mut next_author_color);

        let (node_lane, has_incoming) = if on_main {
            if matches!(lanes.first(), Some(Some((o, _))) if o == oid) {
                (0, true)
            } else if matches!(lanes.first(), Some(Some((expected, _))) if ancestry.is_ancestor_of(oid, expected, commits, &oid_to_idx))
            {
                let color = lanes[0].as_ref().map(|(_, c)| *c).unwrap_or(author_idx);
                lanes[0] = Some((oid.clone(), color));
                (0, true)
            } else if matches!(lanes.first(), Some(None)) || lanes.is_empty() {
                let was_empty = lanes.is_empty();
                let color = if was_empty {
                    lanes.push(None);
                    author_idx
                } else {
                    0
                };
                lanes[0] = Some((oid.clone(), color));
                (0, false)
            } else if matches!(lanes.first(), Some(Some((expected, _))) if main_chain.contains(expected))
            {
                let color = lanes[0].as_ref().map(|(_, c)| *c).unwrap_or(author_idx);
                lanes[0] = Some((oid.clone(), color));
                (0, true)
            } else {
                find_lane(
                    oid,
                    &mut lanes,
                    author_idx,
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
                author_idx,
                commits,
                &oid_to_idx,
                &mut ancestry,
                Some(0),
            )
        };

        let node_color = author_idx;

        // 贯通边（本行其他活跃 lane 继续向下），以及等待本提交的陈旧 lane 的汇入边
        let mut edges = Vec::new();
        for (lane, slot) in lanes.iter_mut().enumerate() {
            if lane == node_lane {
                continue;
            }
            if let Some((expected_oid, color)) = slot {
                if expected_oid == oid {
                    edges.push(GraphEdge {
                        from_lane: lane,
                        to_lane: node_lane,
                        color_index: *color,
                        is_merge: false,
                    });
                    *slot = None;
                } else {
                    edges.push(GraphEdge {
                        from_lane: lane,
                        to_lane: lane,
                        color_index: *color,
                        is_merge: false,
                    });
                }
            }
        }

        // 释放本提交的 lane 再分配 parents
        lanes[node_lane] = None;

        // 清除其他也在等同一 oid 的 lane（zombie 清理，转成汇入边）
        for (lane_idx, lane) in lanes.iter_mut().enumerate() {
            if matches!(lane, Some((o, _)) if o == oid) {
                let color = lane.as_ref().map(|(_, c)| *c).unwrap_or(0);
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

        // parents：主链 merge 提交把主链 parent 当主 parent 路由
        if !commit.parents.is_empty() {
            let (primary, secondaries) = if on_main && commit.parents.len() > 1 {
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
                if matches!(lanes.first(), Some(Some((o, _))) if *o == primary) {
                    0
                } else if matches!(lanes.first(), Some(None)) || lanes.is_empty() {
                    if lanes.is_empty() {
                        lanes.push(None);
                    }
                    lanes[0] = Some((primary, node_color));
                    0
                } else if node_lane == 0 {
                    lanes[0] = Some((primary, node_color));
                    0
                } else {
                    route_parent(
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
                route_parent(
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
                let parent_lane = route_parent(
                    parent,
                    node_lane,
                    node_color,
                    &mut lanes,
                    commits,
                    &oid_to_idx,
                    &mut ancestry,
                );
                // merge 边也是本提交的连线：与圈同色
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: parent_lane,
                    color_index: node_color,
                    is_merge: true,
                });
            }
        }

        // 压缩尾部空 lane
        while lanes.last() == Some(&None) {
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

    // 修剪 main_tip 上方纯 lane-0 贯通线（预占 lane 产生的幽灵线）
    if reserve_lane_0 {
        if let Some(tip) = &main_tip {
            if let Some(&tip_idx) = oid_to_idx.get(tip) {
                if tip_idx > 0 {
                    let strip_until = rows
                        .iter()
                        .take(tip_idx)
                        .position(|r| r.edges.iter().any(|e| e.to_lane == 0 && e.from_lane != 0))
                        .map(|i| (i + 1).min(tip_idx))
                        .unwrap_or(tip_idx);

                    let mut last_stripped = None;
                    for (idx, row) in rows.iter_mut().enumerate().take(strip_until) {
                        let before = row.edges.len();
                        row.edges.retain(|e| !(e.from_lane == 0 && e.to_lane == 0));
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

/// 提交者 → 颜色索引（首见顺序分配，保证不同提交者不同色、同提交者同色）
fn author_color_index(
    author: &str,
    map: &mut std::collections::HashMap<String, usize>,
    next: &mut usize,
) -> usize {
    *map.entry(author.to_string()).or_insert_with(|| {
        let c = *next;
        *next += 1;
        c
    })
}

/// 为提交找 lane：精确 oid 匹配 → 祖先匹配 → 新开 lane
fn find_lane(
    oid: &str,
    lanes: &mut Vec<Option<(String, usize)>>,
    author_idx: usize,
    commits: &[LogRow],
    oid_to_idx: &std::collections::HashMap<String, usize>,
    ancestry: &mut AncestorCache,
    skip_lane: Option<usize>,
) -> (usize, bool) {
    if let Some(pos) = lanes
        .iter()
        .enumerate()
        .position(|(i, s)| Some(i) != skip_lane && matches!(s, Some((o, _)) if o == oid))
    {
        return (pos, true);
    }

    if let Some(pos) = lanes.iter().enumerate().position(|(i, s)| {
        Some(i) != skip_lane
            && matches!(s, Some((expected_oid, _)) if ancestry.is_ancestor_of(oid, expected_oid, commits, oid_to_idx))
    }) {
        let color = lanes[pos].as_ref().map(|(_, c)| *c).unwrap_or(author_idx);
        lanes[pos] = Some((oid.to_string(), color));
        return (pos, true);
    }

    let pos = alloc_lane(lanes, skip_lane);
    lanes[pos] = Some((oid.to_string(), author_idx));
    (pos, false)
}

/// 把 parent 路由到已有 lane 或新开 lane（新 lane 沿用本提交圈色，连线与圈同色）
fn route_parent(
    parent: &str,
    node_lane: usize,
    node_color: usize,
    lanes: &mut Vec<Option<(String, usize)>>,
    commits: &[LogRow],
    oid_to_idx: &std::collections::HashMap<String, usize>,
    ancestry: &mut AncestorCache,
) -> usize {
    if let Some(target) = lanes
        .iter()
        .position(|s| matches!(s, Some((o, _)) if o == parent))
    {
        return target;
    }

    if lanes.get(node_lane) == Some(&None) {
        lanes[node_lane] = Some((parent.to_string(), node_color));
        return node_lane;
    }

    if let Some(target) = lanes.iter().position(|s| {
        matches!(s, Some((expected_oid, _)) if ancestry.is_ancestor_of(parent, expected_oid, commits, oid_to_idx))
    }) {
        return target;
    }

    let pos = alloc_lane(lanes, None);
    lanes[pos] = Some((parent.to_string(), node_color));
    pos
}

/// 找第一个空闲 lane，没有则追加
fn alloc_lane(lanes: &mut Vec<Option<(String, usize)>>, skip_lane: Option<usize>) -> usize {
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
pub fn format_relative_time(timestamp: i64, now: i64, locale: crate::core::i18n::Locale) -> String {
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
        // main: 1 → 2 → 4（merge）；feature: 3 从 2 分出，4 合入
        let commits = vec![
            make_commit(4, &[2, 3], "HEAD -> main"),
            make_commit(3, &[2], ""), // feature tip
            make_commit(2, &[1], ""),
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);
        // merge 提交在主 lane
        assert_eq!(rows[0].node_lane, 0);
        assert!(rows[0].is_merge);
        // feature 提交不在 lane 0
        assert_ne!(rows[1].node_lane, 0);
        // 有 merge 边
        assert!(rows[0].edges.iter().any(|e| e.is_merge));
        assert_eq!(rows[0].edges.iter().filter(|e| e.is_merge).count(), 1);
    }

    #[test]
    fn feature_branch_ahead_of_main_gets_own_lane() {
        // main tip 在后：feature 领先
        let commits = vec![
            make_commit(3, &[1], "feature"),      // 功能分支尖端
            make_commit(2, &[1], "HEAD -> main"), // main 落后
            make_commit(1, &[], ""),
        ];
        let rows = compute_graph(&commits);
        assert_eq!(rows.len(), 3);
        // main 链在 lane 0
        assert_eq!(rows[1].node_lane, 0);
        assert_eq!(rows[2].node_lane, 0);
        // feature 尖端不在 lane 0
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
    fn same_author_same_color() {
        // 线性链：同一提交者 → 全程同色
        let commits = vec![
            make_commit(1, &[2], "HEAD -> main"),
            make_commit(2, &[3], ""),
            make_commit(3, &[4], ""),
            make_commit(4, &[], ""),
        ];
        let rows = compute_graph(&commits);
        let c0 = rows[0].node_color;
        assert!(rows.iter().all(|r| r.node_color == c0));
    }

    #[test]
    fn circle_color_follows_author() {
        // 不同提交者不同圈色；同提交者同色；出边（含 merge 边）与圈同色
        let mut commits = vec![
            make_commit(1, &[2], "HEAD -> main"),
            make_commit(2, &[3], ""),
            make_commit(3, &[], ""),
        ];
        commits[0].author = "Alice".into();
        commits[1].author = "Bob".into();
        commits[2].author = "Bob".into();
        let rows = compute_graph(&commits);
        assert_ne!(rows[0].node_color, rows[1].node_color);
        assert_eq!(rows[1].node_color, rows[2].node_color);
        // 每个提交发出的边颜色 = 自己的圈色
        for row in &rows {
            assert!(
                row.edges
                    .iter()
                    .filter(|e| e.from_lane == row.node_lane)
                    .all(|e| e.color_index == row.node_color)
            );
        }
    }
}
