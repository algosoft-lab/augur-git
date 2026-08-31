//! GPUI painter for active-lane commit graphs.

use gpui::*;

use crate::core::graph::GraphRow;

use super::{COL_WIDTH, GRAPH_LEFT_PAD, NODE_RADIUS, ROW_HEIGHT};

const TURN_RADIUS: f32 = 6.0;
const STROKE_WIDTH: f32 = 1.5;
const CURVE_CONTROL: f32 = 0.552_284_8;

/// Draw one graph row using rounded orthogonal lane transitions.
pub(super) fn draw_graph_row(
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

    // Parent lanes are created by the current commit. All other output lanes
    // are continuations of input lanes and must be rendered first.
    let mut parent_lane = vec![false; row.output_lanes.len()];
    for &lane in &row.parent_lanes {
        if let Some(is_parent) = parent_lane.get_mut(lane) {
            *is_parent = true;
        }
    }
    let mut used_output = vec![false; row.output_lanes.len()];
    let mut node_input_lane = vec![false; row.input_lanes.len()];
    for &lane in &row.node_input_lanes {
        if let Some(is_node_input) = node_input_lane.get_mut(lane) {
            *is_node_input = true;
        }
    }

    for (from_lane, input) in row.input_lanes.iter().enumerate() {
        if node_input_lane[from_lane] {
            if from_lane != row.node_lane {
                paint_to_node_route(
                    lane_x(from_lane),
                    node_x,
                    origin_y,
                    origin_y + px(mid_y),
                    lane_color(input.color_index),
                    window,
                );
            }
            continue;
        }

        let Some(to_lane) = row
            .output_lanes
            .iter()
            .enumerate()
            .find(|(lane, output)| {
                !used_output[*lane]
                    && !parent_lane[*lane]
                    && output.oid == input.oid
            })
            .map(|(lane, _)| lane)
        else {
            continue;
        };
        used_output[to_lane] = true;

        let color = lane_color(input.color_index);
        if from_lane == to_lane {
            paint_stroke_line(
                lane_x(from_lane),
                origin_y,
                lane_x(to_lane),
                origin_y + px(ROW_HEIGHT),
                color,
                window,
            );
        } else {
            paint_through_route(
                lane_x(from_lane),
                lane_x(to_lane),
                origin_y,
                origin_y + px(ROW_HEIGHT),
                mid_y,
                color,
                window,
            );
        }
    }

    // Draw each parent pipe from the node to the corresponding output lane.
    // The first parent normally stays on the node lane; secondary parents use
    // a rounded turn and continue down their newly appended lane.
    for &to_lane in &row.parent_lanes {
        if to_lane == row.node_lane {
            continue;
        }
        let Some(parent) = row.output_lanes.get(to_lane) else {
            continue;
        };
        paint_from_node_route(
            node_x,
            lane_x(to_lane),
            origin_y + px(mid_y),
            origin_y + px(ROW_HEIGHT),
            lane_color(parent.color_index),
            window,
        );
    }

    let node_color = lane_color(row.node_color);
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
    if row.parent_lanes.contains(&row.node_lane) {
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
        if let Some(path) =
            build_filled_circle(node_x, origin_y + px(mid_y), NODE_RADIUS)
        {
            window.paint_path(path, node_color);
        }
    } else if let Some(path) =
        build_stroked_circle(node_x, origin_y + px(mid_y), NODE_RADIUS, px(1.5))
    {
        window.paint_path(path, node_color);
    }
}

fn paint_through_route(
    from_x: Pixels,
    to_x: Pixels,
    top_y: Pixels,
    bottom_y: Pixels,
    mid_y: f32,
    color: Hsla,
    window: &mut Window,
) {
    let goes_right = to_x > from_x;
    let direction = if goes_right { 1.0 } else { -1.0 };
    let from_turn_x = from_x + px(direction * TURN_RADIUS);
    let to_turn_x = to_x - px(direction * TURN_RADIUS);
    let center_y = top_y + px(mid_y);
    let first_turn_y = center_y - px(TURN_RADIUS);
    let second_turn_y = center_y + px(TURN_RADIUS);
    let control = px(TURN_RADIUS * CURVE_CONTROL);

    let mut path = PathBuilder::stroke(px(STROKE_WIDTH));
    path.move_to(point(from_x, top_y));
    path.line_to(point(from_x, first_turn_y));
    path.cubic_bezier_to(
        point(from_turn_x, center_y),
        point(from_x, first_turn_y + control),
        point(
            from_turn_x - px(direction * TURN_RADIUS * CURVE_CONTROL),
            center_y,
        ),
    );
    path.line_to(point(to_turn_x, center_y));
    path.cubic_bezier_to(
        point(to_x, second_turn_y),
        point(
            to_turn_x + px(direction * TURN_RADIUS * CURVE_CONTROL),
            center_y,
        ),
        point(to_x, second_turn_y - control),
    );
    path.line_to(point(to_x, bottom_y));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn paint_to_node_route(
    from_x: Pixels,
    node_x: Pixels,
    top_y: Pixels,
    center_y: Pixels,
    color: Hsla,
    window: &mut Window,
) {
    let direction = if node_x > from_x { 1.0 } else { -1.0 };
    let from_turn_x = from_x + px(direction * TURN_RADIUS);
    let first_turn_y = center_y - px(TURN_RADIUS);
    let node_edge_x = node_x - px(direction * NODE_RADIUS);
    let control = px(TURN_RADIUS * CURVE_CONTROL);

    let mut path = PathBuilder::stroke(px(STROKE_WIDTH));
    path.move_to(point(from_x, top_y));
    path.line_to(point(from_x, first_turn_y));
    path.cubic_bezier_to(
        point(from_turn_x, center_y),
        point(from_x, first_turn_y + control),
        point(
            from_turn_x - px(direction * TURN_RADIUS * CURVE_CONTROL),
            center_y,
        ),
    );
    path.line_to(point(node_edge_x, center_y));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn paint_from_node_route(
    node_x: Pixels,
    target_x: Pixels,
    node_y: Pixels,
    bottom_y: Pixels,
    color: Hsla,
    window: &mut Window,
) {
    let goes_right = target_x > node_x;
    let direction = if goes_right { 1.0 } else { -1.0 };
    let target_turn_x = target_x - px(direction * TURN_RADIUS);
    let turn_y = node_y + px(TURN_RADIUS);
    let node_edge_x = node_x + px(direction * NODE_RADIUS);
    let control = px(TURN_RADIUS * CURVE_CONTROL);

    let mut path = PathBuilder::stroke(px(STROKE_WIDTH));
    path.move_to(point(node_edge_x, node_y));
    path.line_to(point(target_turn_x, node_y));
    path.cubic_bezier_to(
        point(target_x, turn_y),
        point(
            target_turn_x + px(direction * TURN_RADIUS * CURVE_CONTROL),
            node_y,
        ),
        point(target_x, turn_y - control),
    );
    path.line_to(point(target_x, bottom_y));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

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

fn paint_stroke_line(
    x1: Pixels,
    y1: Pixels,
    x2: Pixels,
    y2: Pixels,
    color: Hsla,
    window: &mut Window,
) {
    let mut path = PathBuilder::stroke(px(STROKE_WIDTH));
    path.move_to(point(x1, y1));
    path.line_to(point(x2, y2));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
