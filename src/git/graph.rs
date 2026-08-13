//! M1：GraphView 提交树（照抄 rgitui：canvas + paint_path 画节点/连线）

use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, InteractiveElementExt, h_flex, v_flex};

use crate::core::graph::LogRow;
use crate::git::shared;

/// 提交树行高（h_9=36px：圆心距 36，节点直径 24，边缘间距 12 = 半径，验证项目同比例）
pub const ROW_HEIGHT: f32 = 36.0;
/// 树列宽（≥ 节点直径 24，圆不溢出列）
pub const COL_WIDTH: f32 = 24.0;
/// 节点空心圆半径（stroke 描边细线圆，直径 24）
const NODE_RADIUS: f32 = 12.0;

#[derive(Clone, Debug)]
pub enum GraphEvent {
    CommitSelected {
        short: String,
        subject: String,
        author: String,
        date: String,
        decorations: String,
    },
    ShowDiff(String),
}

pub struct GraphView {
    rows: Vec<LogRow>,
    selected: Option<usize>,
}

impl EventEmitter<GraphEvent> for GraphView {}

impl GraphView {
    pub fn new() -> Self {
        Self { rows: Vec::new(), selected: None }
    }

    pub fn set_rows(&mut self, rows: Vec<LogRow>, cx: &mut Context<Self>) {
        self.selected = self.selected.and_then(|i| rows.get(i).map(|_| i)).filter(|i| *i < rows.len());
        self.rows = rows;
        cx.notify();
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else { return };
        self.selected = Some(index);
        cx.emit(GraphEvent::CommitSelected {
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let mono = cx.theme().mono_font_family.clone();

        if self.rows.is_empty() {
            return v_flex().id("graph-view").size_full().items_center().justify_center()
                .bg(colors.background)
                .child(div().text_size(px(12.)).text_color(colors.muted).child("暂无提交"))
                .into_any_element();
        }

        let max_cols = self.rows.iter().map(|r| r.graph.chars().count()).max().unwrap_or(1);
        let tree_w = max_cols as f32 * COL_WIDTH + 8.0;

        let rows = self.rows.iter().enumerate().map(|(i, row)| {
            let this = cx.entity();
            let this_dbl = this.clone();
            let row = row.clone();
            let graph_str = row.graph.clone();
            let selected = self.selected == Some(i);
            let row_bg = if selected { colors.list_active } else { colors.background };
            // 节点圆内字母：提交者姓名前 2 个字符（验证项目同款 div 叠加）
            let node_col = row.graph.find('*').map(|c| c as f32);
            let node_letters: String = row.author.chars().take(2).collect();
            h_flex()
                .id(SharedString::from(format!("graph-row-{}", row.oid)))
                .w_full().h_9().flex_shrink_0().items_center().pr_2().gap_2()
                .bg(row_bg)
                .hover(|this| if !selected { this.bg(colors.list_hover) } else { this })
                .on_click(move |_e, _w, cx| { this.update(cx, |v, cx| v.select(i, cx)); })
                .on_double_click(move |_e, _w, cx| {
                    this_dbl.update(cx, |v, cx| {
                        if let Some(oid) = v.rows.get(i).map(|r| r.oid.clone()) {
                            cx.emit(GraphEvent::ShowDiff(oid));
                        }
                    });
                })
                // 树列：行内 canvas + HEAD 字母（absolute div 叠加）
                .child(
                    div().w(px(tree_w)).flex_shrink_0().h_full().relative().child(
                        canvas(
                            |_b: Bounds<Pixels>, _w: &mut Window, _c: &mut App| {},
                            move |bounds: Bounds<Pixels>, (): (), window: &mut Window, _c: &mut App| {
                                draw_tree(&graph_str, bounds, window, row_bg);
                            },
                        ).w_full().h_full(),
                    )
                    .when_some(node_col, |el, col| {
                        let xc = col * COL_WIDTH + COL_WIDTH / 2.0;
                        let letters = node_letters.clone();
                        el.child(
                            div().absolute()
                                .left(px(xc - 12.))
                                .top(px(ROW_HEIGHT / 2.0 - 15.))
                                .w(px(24.)).h(px(30.))
                                .flex().items_center().justify_center()
                                .text_size(px(13.))
                                .text_color(colors.foreground)
                                .child(shared(letters)),
                        )
                    }),
                )
                .child(
                    div().flex_shrink_0().font_family(mono.clone()).text_size(px(12.))
                        .text_color(colors.blue).child(shared(row.short)),
                )
                .child(
                    div().flex_1().min_w_0().text_size(px(12.))
                        .text_color(colors.foreground).child(shared(row.author)),
                )
        }).collect::<Vec<_>>();

        v_flex().id("graph-view").size_full().bg(colors.background)
            .child(v_flex().id("graph-scroll").flex_1().min_h_0().overflow_y_scroll()
                .child(v_flex().id("graph-rows").children(rows)))
            .into_any_element()
    }
}

/// lane 配色（照抄 rgitui GRAPH_LANE_COLORS）
pub fn lane_color(index: usize) -> Hsla {
    const COLORS: [fn() -> Hsla; 10] = [
        || hsla(267.0, 84.0, 75.0, 1.0),
        || hsla(217.0, 92.0, 65.0, 1.0),
        || hsla(115.0, 60.0, 65.0, 1.0),
        || hsla(23.0, 92.0, 65.0, 1.0),
        || hsla(343.0, 81.0, 65.0, 1.0),
        || hsla(170.0, 65.0, 60.0, 1.0),
        || hsla(41.0, 86.0, 70.0, 1.0),
        || hsla(189.0, 75.0, 60.0, 1.0),
        || hsla(316.0, 72.0, 72.0, 1.0),
        || hsla(10.0, 70.0, 75.0, 1.0),
    ];
    COLORS[index % COLORS.len()]()
}

/// 绘制一行提交树（照抄验证项目：stroke 描边空心圆 + stroke 细线）
///
/// - 节点：PathBuilder::stroke + arc_to 画圆环（空心细线圆，验证项目同款）
/// - 连线：stroke 细线（列分离，天然不进入圆内）
fn draw_tree(graph: &str, bounds: Bounds<Pixels>, window: &mut Window, _row_bg: Hsla) {
    let origin_x = bounds.origin.x;
    let origin_y = bounds.origin.y;
    let mid_y = ROW_HEIGHT / 2.0;

    for (col, ch) in graph.chars().enumerate() {
        let xc = origin_x + px(col as f32 * COL_WIDTH + COL_WIDTH / 2.0);
        let color = lane_color(col);
        match ch {
            '*' => {
                // 空心细线圆：stroke 描边圆环（arc_to 两个半圆闭合）
                if let Some(p) = build_stroked_circle(xc, origin_y + px(mid_y), NODE_RADIUS, px(1.5))
                {
                    window.paint_path(p, color);
                }
                // 圆上/下竖线片段：连接上下节点，止于圆周（不进入圆内）
                paint_stroke_line(
                    xc,
                    origin_y,
                    xc,
                    origin_y + px(mid_y - NODE_RADIUS),
                    color,
                    window,
                );
                paint_stroke_line(
                    xc,
                    origin_y + px(mid_y + NODE_RADIUS),
                    xc,
                    origin_y + px(ROW_HEIGHT),
                    color,
                    window,
                );
            }
            '|' => {
                paint_stroke_line(xc, origin_y, xc, origin_y + px(ROW_HEIGHT), color, window);
            }
            '-' | '_' => {
                let y = if ch == '_' { ROW_HEIGHT - 1.0 } else { mid_y };
                paint_stroke_line(
                    origin_x + px(col as f32 * COL_WIDTH),
                    origin_y + px(y),
                    origin_x + px(col as f32 * COL_WIDTH + COL_WIDTH),
                    origin_y + px(y),
                    color,
                    window,
                );
            }
            '/' | '\\' => {
                let x0 = origin_x + px(col as f32 * COL_WIDTH);
                let x1 = origin_x + px(col as f32 * COL_WIDTH + COL_WIDTH);
                if ch == '/' {
                    paint_stroke_line(x1, origin_y, x0, origin_y + px(ROW_HEIGHT), color, window);
                } else {
                    paint_stroke_line(x0, origin_y, x1, origin_y + px(ROW_HEIGHT), color, window);
                }
            }
            _ => {}
        }
    }
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
