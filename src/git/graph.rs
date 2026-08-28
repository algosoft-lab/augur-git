//! M1：GraphView 提交树（镜像 rgitui：compute_graph 布局 + canvas 画 lane/节点/连线）

use crate::core::git::CheckoutTarget;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenuItem},
    theme::ThemeColor,
    v_flex,
};
use std::ops::Range;

use crate::core::graph::{
    AUTHOR_COL_WIDTH, DATE_COL_WIDTH, GraphRow, HASH_COL_WIDTH, LogRow,
    column_visibility, compute_graph, format_relative_time,
};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

/// 提交树行高（h_9=36px：圆心距 36，节点直径 24，边缘间距 12 = 半径，验证项目同比例）
pub const ROW_HEIGHT: f32 = 36.0;
/// 树列宽（≥ 节点直径 24，圆不溢出列）
pub const COL_WIDTH: f32 = 24.0;
/// 节点空心圆半径（stroke 描边细线圆，直径 24）
const NODE_RADIUS: f32 = 12.0;
/// 树列左侧留白（首个节点圆不贴左边界）
const GRAPH_LEFT_PAD: f32 = 12.0;

#[derive(Clone, Debug)]
pub enum GraphEvent {
    CommitSelected {
        /// 完整 oid（底部面板查文件清单/diff 用）
        oid: String,
        short: String,
        subject: String,
        author: String,
        date: String,
        decorations: String,
    },
    /// Check out the selected commit.
    CheckoutRef(CheckoutTarget),
    /// Copy the selected commit OID to the system clipboard.
    CopyRef(String),
}

pub struct GraphView {
    rows: Vec<LogRow>,
    layout: Vec<GraphRow>,
    selected: Option<usize>,
    /// 界面语言（Workspace 切换语言时同步）
    locale: Locale,
    /// 自身实测渲染宽（测量 canvas 回写；初始视为宽屏，首帧后校正）。
    /// 响应式列显隐以它为准——拖侧栏挤压中央列时窗口宽并未变
    content_width: f32,
    scroll_handle: UniformListScrollHandle,
    list_id: SharedString,
}

#[derive(Clone)]
struct GraphRenderOptions {
    tree_w: f32,
    show_author: bool,
    show_message: bool,
    now: i64,
    colors: ThemeColor,
    mono: SharedString,
    lane_colors: [Hsla; 10],
}

impl EventEmitter<GraphEvent> for GraphView {}

impl GraphView {
    pub fn new(tab_id: u64, locale: Locale) -> Self {
        Self {
            rows: Vec::new(),
            layout: Vec::new(),
            selected: None,
            locale,
            content_width: f32::INFINITY,
            scroll_handle: UniformListScrollHandle::new(),
            list_id: SharedString::from(format!("graph-rows-{tab_id}")),
        }
    }

    /// 切换语言（Workspace::set_language 同步）
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    pub fn set_rows(&mut self, rows: Vec<LogRow>, cx: &mut Context<Self>) {
        self.selected = self
            .selected
            .and_then(|i| rows.get(i).map(|_| i))
            .filter(|i| *i < rows.len());
        self.layout = compute_graph(&rows);
        log::debug!(
            "[graph_perf] graph layout cached: rows={}",
            self.layout.len()
        );
        self.rows = rows;
        cx.notify();
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        self.selected = Some(index);
        cx.emit(GraphEvent::CommitSelected {
            oid: row.oid.clone(),
            short: row.short.clone(),
            subject: row.subject.clone(),
            author: row.author.clone(),
            date: row.date.clone(),
            decorations: row.decorations.clone(),
        });
        cx.notify();
    }
}

impl Render for GraphView {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mono = cx.theme().mono_font_family.clone();

        if self.rows.is_empty() {
            return v_flex()
                .id("graph-view")
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .bg(colors.background)
                // 空态也持续测量宽度：首批行到达时列显隐即正确
                .child(measure_width_canvas(cx.entity()))
                .child(
                    div()
                        .size(px(24.))
                        .text_color(colors.muted_foreground)
                        .child(crate::git::lucide("git-commit-horizontal")),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(self.locale, "graph-empty"))),
                )
                .into_any_element();
        }

        let max_lanes = self
            .layout
            .iter()
            .map(|row| row.lane_count)
            .max()
            .unwrap_or(1);
        let tree_w = GRAPH_LEFT_PAD + max_lanes as f32 * COL_WIDTH + 8.0;
        // 响应式列显隐：以自身实测宽为准（窄则先藏 Author 再藏 Message，见 column_visibility）
        let (show_author, show_message) =
            column_visibility(self.content_width, tree_w);
        // 相对时间基准（unix 秒）
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let lane_colors = crate::theme::lane_colors(cx);
        let render_options = GraphRenderOptions {
            tree_w,
            show_author,
            show_message,
            now,
            colors,
            mono,
            lane_colors,
        };
        let rows = uniform_list(
            self.list_id.clone(),
            self.rows.len(),
            cx.processor(move |graph, range: Range<usize>, _window, cx| {
                range
                    .map(|index| graph.render_row(index, &render_options, cx))
                    .collect()
            }),
        )
        .track_scroll(&self.scroll_handle)
        .flex_1()
        .min_h_0();

        v_flex()
            .id("graph-view")
            .size_full()
            .bg(colors.background)
            // 宽度测量：0 高 canvas 每帧回写自身宽（变化才 notify，收敛不循环）
            .child(measure_width_canvas(cx.entity()))
            .child(self.column_header(
                &colors,
                tree_w,
                show_author,
                show_message,
            ))
            .child(rows)
            .into_any_element()
    }
}

impl GraphView {
    fn render_row(
        &self,
        index: usize,
        options: &GraphRenderOptions,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.rows.get(index).cloned() else {
            return div().into_any_element();
        };
        let Some(graph_row) = self.layout.get(index).cloned() else {
            return div().into_any_element();
        };
        let graph = cx.entity();
        let selected = self.selected == Some(index);
        let colors = options.colors;
        let tree_w = options.tree_w;
        let lane_colors = options.lane_colors;
        let row_bg = if selected {
            colors.list_active
        } else {
            colors.background
        };
        // The author initials sit on top of the graph node canvas.
        let node_col = Some(graph_row.node_lane as f32);
        let node_letters: String = row.author.chars().take(2).collect();
        let checkout_label = i18n::text(self.locale, "context-checkout");
        let copy_label = i18n::text(self.locale, "context-copy-commit");
        let commit_target = CheckoutTarget::Commit(row.oid.clone());
        let copy_value = row.oid.clone();
        let graph_for_click = graph.clone();

        let row_element = h_flex()
            .id(SharedString::from(format!("graph-row-{}", row.oid)))
            .w_full()
            .h_9()
            .flex_shrink_0()
            .items_center()
            .pr_2()
            .gap_2()
            .bg(row_bg)
            .hover(|this| {
                if !selected {
                    this.bg(colors.list_hover)
                } else {
                    this
                }
            })
            .on_click(move |_e, _w, cx| {
                graph_for_click.update(cx, |v, cx| v.select(index, cx));
            })
            // Graph column: row canvas plus HEAD initials overlay.
            .child(
                div()
                    .w(px(tree_w))
                    .flex_shrink_0()
                    .h_full()
                    .relative()
                    .child(
                        canvas(
                            |_b: Bounds<Pixels>,
                             _w: &mut Window,
                             _c: &mut App| {},
                            move |bounds: Bounds<Pixels>,
                                  (): (),
                                  window: &mut Window,
                                  _cx: &mut App| {
                                draw_graph_row(
                                    &graph_row,
                                    bounds,
                                    window,
                                    &lane_colors,
                                );
                            },
                        )
                        .w_full()
                        .h_full(),
                    )
                    .when_some(node_col, |el, col| {
                        let xc =
                            GRAPH_LEFT_PAD + col * COL_WIDTH + COL_WIDTH / 2.0;
                        let letters = node_letters.clone();
                        el.child(
                            div()
                                .absolute()
                                .left(px(xc - 12.))
                                .top(px(ROW_HEIGHT / 2.0 - 15.))
                                .w(px(24.))
                                .h(px(30.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .text_color(colors.foreground)
                                .child(shared(letters)),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(HASH_COL_WIDTH))
                    .flex_shrink_0()
                    .font_family(options.mono.clone())
                    .text_size(px(12.))
                    .text_color(colors.blue)
                    .child(shared(row.short)),
            )
            // Message column takes remaining space and truncates when needed.
            .when(options.show_message, |el| {
                el.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(colors.foreground)
                        .child(shared(row.subject.clone())),
                )
            })
            // Author column is fixed width and truncates long names.
            .when(options.show_author, |el| {
                el.child(
                    div()
                        .w(px(AUTHOR_COL_WIDTH))
                        .flex_shrink_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(colors.foreground)
                        .child(shared(row.author)),
                )
            })
            // Date column displays the relative commit time.
            .child(
                div()
                    .w(px(DATE_COL_WIDTH))
                    .flex_shrink_0()
                    .text_size(px(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(format_relative_time(
                        row.timestamp,
                        options.now,
                        self.locale,
                    ))),
            );

        row_element
            .context_menu(move |menu, _window, _cx| {
                let graph_for_checkout = graph.clone();
                let graph_for_copy = graph.clone();
                let commit_target = commit_target.clone();
                let copy_value = copy_value.clone();

                menu.item(
                    PopupMenuItem::new(checkout_label.clone())
                        .icon(crate::git::lucide("git-commit-horizontal"))
                        .on_click(move |_event, _window, cx| {
                            graph_for_checkout.update(cx, |_graph, cx| {
                                cx.emit(GraphEvent::CheckoutRef(
                                    commit_target.clone(),
                                ));
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(copy_label.clone())
                        .icon(IconName::Copy)
                        .on_click(move |_event, _window, cx| {
                            graph_for_copy.update(cx, |_graph, cx| {
                                cx.emit(GraphEvent::CopyRef(
                                    copy_value.clone(),
                                ));
                            });
                        }),
                )
            })
            .into_any_element()
    }

    /// 列头（参考 rgitui graph header：26px 条、muted 半粗小标签、下边框；
    /// 列宽与行内列对齐：Graph 树列 / Hash / Message(flex) / Author / Date，
    /// 后三列随窗口变窄按 column_visibility 收缩至隐藏，与行同步）
    fn column_header(
        &self,
        colors: &ThemeColor,
        tree_w: f32,
        show_author: bool,
        show_message: bool,
    ) -> impl IntoElement {
        let label = |text: &str| {
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted_foreground)
                .child(shared(text))
        };
        // 列间分割短线：1px 宽、14px 高，absolute 定位在本列左缘左侧 4px
        // （= 与左邻列的 8px 间隙中点），不参与布局；随本列隐藏自动消失
        let col = |text: &str, w: f32, with_divider: bool| {
            div()
                .relative()
                .w(px(w))
                .flex_shrink_0()
                .when(with_divider, |el| {
                    el.child(
                        div()
                            .absolute()
                            .left(px(-4.))
                            .top(px(6.))
                            .w(px(1.))
                            .h(px(14.))
                            .bg(colors.border),
                    )
                })
                .child(label(text))
        };
        h_flex()
            .id("graph-header")
            .w_full()
            .h(px(26.))
            .flex_shrink_0()
            .pr_2()
            .gap_2()
            .items_center()
            .relative()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            // Graph 列：与行内树画布同宽
            .child(
                div()
                    .w(px(tree_w))
                    .flex_shrink_0()
                    .pl_1()
                    .child(label(&i18n::text(self.locale, "col-graph"))),
            )
            // Hash 列：与行内短 oid 同宽（左缘分割线隔开树列）
            .child(col(
                &i18n::text(self.locale, "col-hash"),
                HASH_COL_WIDTH,
                true,
            ))
            // Message 列：占满剩余（与行内 flex_1 对齐）
            .when(show_message, |el| {
                el.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .relative()
                        .child(
                            div()
                                .absolute()
                                .left(px(-4.))
                                .top(px(6.))
                                .w(px(1.))
                                .h(px(14.))
                                .bg(colors.border),
                        )
                        .child(label(&i18n::text(self.locale, "col-message"))),
                )
            })
            // Author 列：定宽（与行内 140 对齐）
            .when(show_author, |el| {
                el.child(col(
                    &i18n::text(self.locale, "col-author"),
                    AUTHOR_COL_WIDTH,
                    true,
                ))
            })
            // Date 列：与行内日期同宽
            .child(col(
                &i18n::text(self.locale, "col-date"),
                DATE_COL_WIDTH,
                true,
            ))
    }
}

/// 宽度测量 canvas（w_full、0 高，不占布局）：prepaint 取自身 bounds 宽回写
/// GraphView。回写 defer 出布局阶段（prepaint 内直接 update 实体会重入布局），
/// 且仅宽度变化时 notify——收敛后不再触发重绘，拖拽连续变化也只是逐帧跟随
fn measure_width_canvas(entity: Entity<GraphView>) -> impl IntoElement {
    canvas(
        move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
            let w = f32::from(bounds.size.width);
            if w > 0.0 && w.is_finite() {
                cx.defer(move |cx: &mut App| {
                    entity.update(cx, |view, cx| {
                        if view.content_width != w {
                            view.content_width = w;
                            cx.notify();
                        }
                    });
                });
            }
        },
        |_bounds: Bounds<Pixels>,
         _state: (),
         _window: &mut Window,
         _cx: &mut App| {},
    )
    .w_full()
    .h(px(0.))
}

/// 绘制一行提交图（镜像 rgitui：lane 布局 + 边止于圆周）
///
/// - 竖线（同 lane 贯通/出入边）：上段到圆顶、下段从圆底，不进入圆内
/// - 跨 lane 连接（入边/出边）：改用 augur-exp 弧线方案 —— 从 lane 端点
///   贝塞尔弧到本节点圆周（lane 在右 → 3 点钟，在左 → 9 点钟），末端切线指向圆心；
///   配合节点上段竖线（= exp 的竖直直线段），两圆连接即 exp 的「直线 + 弧线」造型
/// - 跨 lane 贯通（不碰本行节点的 lane 迁移）：贯穿斜线
/// - 节点：空心描边圆（stroke）；HEAD 提交实心圆（fill）
fn draw_graph_row(
    row: &GraphRow,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    lane_colors: &[Hsla; 10],
) {
    let lane_color = |index: usize| lane_colors[index % lane_colors.len()];
    let origin_x = bounds.origin.x;
    let origin_y = bounds.origin.y;
    let mid_y = ROW_HEIGHT / 2.0;
    let lane_x = |lane: usize| {
        origin_x
            + px(GRAPH_LEFT_PAD + lane as f32 * COL_WIDTH + COL_WIDTH / 2.0)
    };
    let node_x = lane_x(row.node_lane);

    // 1. 入边（到本行节点）+ 跨 lane 贯通边
    for e in &row.edges {
        if e.to_lane == row.node_lane {
            if e.from_lane == row.node_lane {
                continue; // 同 lane 入边：由节点上段竖线处理
            }
            // 跨 lane 入边：上一行起点列 → 本节点 3/9 点钟（exp 弧线）
            paint_stroke_arc(
                lane_x(e.from_lane),
                origin_y,
                node_x,
                origin_y + px(mid_y),
                e.from_lane > row.node_lane,
                lane_color(e.color_index),
                window,
            );
        } else if e.from_lane == e.to_lane {
            // 贯通竖线：本行该 lane 无节点，全程贯通（不能断开；
            // 旧版只画上/下两段留中间空档，圆与圆之间的直线会断）
            let x = lane_x(e.from_lane);
            let color = lane_color(e.color_index);
            paint_stroke_line(
                x,
                origin_y,
                x,
                origin_y + px(ROW_HEIGHT),
                color,
                window,
            );
        } else if e.from_lane != row.node_lane && e.to_lane != row.node_lane {
            // 跨 lane 贯通（lane 迁移，不碰本行节点）：贯穿斜线
            paint_stroke_line(
                lane_x(e.from_lane),
                origin_y,
                lane_x(e.to_lane),
                origin_y + px(ROW_HEIGHT),
                lane_color(e.color_index),
                window,
            );
        }
    }

    // 2. 出边（本行节点 → parent lane）：跨 lane 改为 exp 弧线（止于本节点圆周）
    for e in &row.edges {
        if e.from_lane == row.node_lane && e.to_lane != row.node_lane {
            paint_stroke_arc(
                lane_x(e.to_lane),
                origin_y + px(ROW_HEIGHT),
                node_x,
                origin_y + px(mid_y),
                e.to_lane > row.node_lane,
                lane_color(e.color_index),
                window,
            );
        }
    }

    // 3. 节点圆（先画上下竖线段，再画圆覆盖衔接）
    let node_color = lane_color(row.node_color);
    // 节点上段：有入边（含同 lane）才画；无入边（分支尖端）不画
    if row.has_incoming {
        paint_stroke_line(
            node_x,
            origin_y,
            node_x,
            origin_y + px(mid_y - NODE_RADIUS),
            node_color,
            window,
        );
    }
    // 节点下段：仅同 lane 出边（主线继续往下）才画；
    // 跨 lane 出边下方没有连线（由弧线承接），悬空小线段不显示
    if row
        .edges
        .iter()
        .any(|e| e.from_lane == row.node_lane && e.to_lane == row.node_lane)
    {
        paint_stroke_line(
            node_x,
            origin_y + px(mid_y + NODE_RADIUS),
            node_x,
            origin_y + px(ROW_HEIGHT),
            node_color,
            window,
        );
    }

    if row.is_head {
        // HEAD 提交：实心圆
        if let Some(p) =
            build_filled_circle(node_x, origin_y + px(mid_y), NODE_RADIUS)
        {
            window.paint_path(p, node_color);
        }
    } else if let Some(p) =
        build_stroked_circle(node_x, origin_y + px(mid_y), NODE_RADIUS, px(1.5))
    {
        window.paint_path(p, node_color);
    }
}

/// 实心圆（HEAD 提交节点）
fn build_filled_circle(
    cx: Pixels,
    cy: Pixels,
    radius: f32,
) -> Option<gpui::Path<Pixels>> {
    let mut builder = PathBuilder::fill();
    builder.move_to(point(cx + px(radius), cy));
    builder.arc_to(
        point(px(radius), px(radius)),
        px(0.),
        false,
        false,
        point(cx - px(radius), cy),
    );
    builder.arc_to(
        point(px(radius), px(radius)),
        px(0.),
        false,
        false,
        point(cx + px(radius), cy),
    );
    builder.close();
    builder.build().ok()
}

/// 描边空心圆（照抄验证项目：PathBuilder::stroke + arc_to 两个半圆）
fn build_stroked_circle(
    cx: Pixels,
    cy: Pixels,
    radius: f32,
    width: Pixels,
) -> Option<gpui::Path<Pixels>> {
    let mut builder = PathBuilder::stroke(width);
    builder.move_to(point(cx + px(radius), cy));
    builder.arc_to(
        point(px(radius), px(radius)),
        px(0.),
        false,
        false,
        point(cx - px(radius), cy),
    );
    builder.arc_to(
        point(px(radius), px(radius)),
        px(0.),
        false,
        false,
        point(cx + px(radius), cy),
    );
    builder.close();
    builder.build().ok()
}

/// stroke 画直线（照抄 zed：PathBuilder::stroke + paint_path）
fn paint_stroke_line(
    x1: Pixels,
    y1: Pixels,
    x2: Pixels,
    y2: Pixels,
    color: Hsla,
    window: &mut Window,
) {
    let mut path = PathBuilder::stroke(px(1.5));
    path.move_to(point(x1, y1));
    path.line_to(point(x2, y2));
    if let Ok(p) = path.build() {
        window.paint_path(p, color);
    }
}

/// 跨 lane 弧线（照抄 augur-exp draw_scene 的三次贝塞尔）：从 lane 端点
/// （行顶/行底）弧到本节点圆周 —— lane 在右接 3 点钟、在左接 9 点钟，
/// 末端切线水平指向圆心；起点切线竖直，与 lane 竖线顺接；控制点 0.75r（同 exp）
fn paint_stroke_arc(
    lane_x: Pixels,
    lane_y: Pixels,
    node_x: Pixels,
    node_mid_y: Pixels,
    lane_is_right: bool,
    color: Hsla,
    window: &mut Window,
) {
    let end_x = if lane_is_right {
        node_x + px(NODE_RADIUS)
    } else {
        node_x - px(NODE_RADIUS)
    };
    // 起点切线竖直：lane 端在节点上方 → 向下，在下方 → 向上
    let ctrl_a_y = if lane_y < node_mid_y {
        lane_y + px(NODE_RADIUS * 0.75)
    } else {
        lane_y - px(NODE_RADIUS * 0.75)
    };
    // 末端切线水平：控制点放在圆周外侧，切线正指圆心
    let ctrl_b_x = if lane_is_right {
        end_x + px(NODE_RADIUS * 0.75)
    } else {
        end_x - px(NODE_RADIUS * 0.75)
    };
    let mut path = PathBuilder::stroke(px(1.5));
    path.move_to(point(lane_x, lane_y));
    path.cubic_bezier_to(
        point(end_x, node_mid_y),
        point(lane_x, ctrl_a_y),
        point(ctrl_b_x, node_mid_y),
    );
    if let Ok(p) = path.build() {
        window.paint_path(p, color);
    }
}
