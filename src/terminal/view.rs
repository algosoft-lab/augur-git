//! GPUI view and canvas bridge for one visible Agent terminal.

use std::sync::Arc;
use std::time::Duration;

use alacritty_terminal::term::cell::Flags as CellFlags;
use gpui::prelude::*;
use gpui::*;
use gpui_component::ActiveTheme;

use super::geometry::TerminalGeometry;
use super::model::TerminalSnapshot;
use super::render::{
    PlainRenderPlan, StyledRenderPlan, build_plain_render_plan,
    build_styled_render_plan, terminal_color_to_hsla, terminal_text_run,
};
use super::{CELL_WIDTH, TerminalBackend, TerminalEvent, encode_key};

/// GPUI rendering and input bridge for one visible Agent test.
pub struct TerminalView {
    backend: Arc<TerminalBackend>,
    focus_handle: FocusHandle,
    _poll_task: Option<Task<()>>,
    child_exit: Option<Option<i32>>,
    error: Option<String>,
}

impl TerminalView {
    pub fn new(backend: Arc<TerminalBackend>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let view_entity = cx.entity();
        let backend_for_task = backend.clone();
        let poll_task = cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let events = backend_for_task.drain_events();
                let done = events.iter().any(|event| {
                    matches!(
                        event,
                        TerminalEvent::ChildExit(_) | TerminalEvent::Error(_)
                    )
                });
                view_entity.update(cx, |view, cx| {
                    view.apply(events, cx);
                });
                if done {
                    break;
                }
            }
        });
        Self {
            backend,
            focus_handle,
            _poll_task: Some(poll_task),
            child_exit: None,
            error: None,
        }
    }

    fn apply(&mut self, events: Vec<TerminalEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                TerminalEvent::ChildExit(code) => self.child_exit = Some(code),
                TerminalEvent::Error(error) => self.error = Some(error),
                TerminalEvent::Wakeup => {}
            }
        }
        cx.notify();
    }

    pub fn completion(&self) -> Option<Result<Option<i32>, String>> {
        if let Some(code) = self.child_exit {
            Some(Ok(code))
        } else {
            self.error.clone().map(Err)
        }
    }

    fn send_key(&self, event: &KeyDownEvent) {
        if let Some(bytes) = encode_key(event) {
            self.backend.send_bytes(bytes);
        }
    }
}

impl Render for TerminalView {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let focus = self.focus_handle.clone();
        let backend = self.backend.clone();
        let geometry_backend = backend.clone();
        let mono = cx.theme().mono_font_family.clone();
        let terminal_colors = colors;
        let terminal_canvas = canvas(
            move |bounds, window, _cx| {
                let geometry = terminal_geometry_for_bounds(bounds, window);
                let (generation, snapshot) =
                    geometry_backend.synchronize_viewport(geometry);
                let plan = build_styled_render_plan(&snapshot);
                let plain_plan = build_plain_render_plan(&snapshot);
                StyledCanvasState {
                    frame: TerminalFrame {
                        geometry,
                        snapshot,
                        generation,
                    },
                    plan,
                    plain_plan,
                }
            },
            move |bounds, state, window, cx| {
                paint_styled_terminal(
                    bounds,
                    state,
                    &terminal_colors,
                    window,
                    cx,
                );
            },
        )
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .font_family(mono.clone())
        .text_size(px(13.));
        let backend_for_mouse = backend.clone();
        let backend_for_keyboard = backend.clone();
        let error = self.error.clone();
        let mut surface = div()
            .id("agent-terminal-surface")
            .size_full()
            .min_h_0()
            .bg(colors.background)
            .focusable()
            .on_mouse_down(
                MouseButton::Left,
                move |_event, window: &mut Window, cx: &mut App| {
                    window.focus(&focus, cx);
                },
            )
            .on_mouse_down(MouseButton::Middle, {
                let backend = backend.clone();
                move |event, _window, _cx| {
                    backend.send_mouse_report(
                        1,
                        event.position,
                        true,
                        event.modifiers,
                    );
                }
            })
            .on_mouse_down(MouseButton::Right, {
                let backend = backend.clone();
                move |event, _window, _cx| {
                    backend.send_mouse_report(
                        2,
                        event.position,
                        true,
                        event.modifiers,
                    );
                }
            })
            .on_mouse_down(MouseButton::Left, move |event, _window, _cx| {
                if !backend_for_mouse.send_mouse_report(
                    0,
                    event.position,
                    true,
                    event.modifiers,
                ) {
                    backend_for_mouse.begin_selection(event.position);
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let backend = backend.clone();
                move |event, _window, _cx| {
                    if !backend.send_mouse_report(
                        0,
                        event.position,
                        false,
                        event.modifiers,
                    ) {
                        backend.finish_selection();
                    }
                }
            })
            .on_mouse_up(MouseButton::Middle, {
                let backend = backend.clone();
                move |event, _window, _cx| {
                    backend.send_mouse_report(
                        1,
                        event.position,
                        false,
                        event.modifiers,
                    );
                }
            })
            .on_mouse_up(MouseButton::Right, {
                let backend = backend.clone();
                move |event, _window, _cx| {
                    backend.send_mouse_report(
                        2,
                        event.position,
                        false,
                        event.modifiers,
                    );
                }
            })
            .on_mouse_move({
                let backend = backend.clone();
                move |event, _window, _cx| {
                    if event.pressed_button.is_some() {
                        if !backend.send_mouse_report(
                            32,
                            event.position,
                            true,
                            event.modifiers,
                        ) {
                            backend.update_selection(event.position);
                        }
                    }
                }
            })
            .on_key_down(move |event, _window, cx| {
                let key = event.keystroke.key.as_str();
                let modifiers = event.keystroke.modifiers;
                let copy = key.eq_ignore_ascii_case("c")
                    && ((modifiers.platform && !modifiers.control)
                        || (modifiers.control && modifiers.shift));
                if copy {
                    if let Some(text) = backend_for_keyboard.selected_text() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        return;
                    }
                }
                let paste = key.eq_ignore_ascii_case("v")
                    && ((modifiers.platform && !modifiers.control)
                        || (modifiers.control && modifiers.shift));
                if paste {
                    if let Some(text) =
                        cx.read_from_clipboard().and_then(|item| item.text())
                    {
                        backend_for_keyboard.paste_text(&text);
                        return;
                    }
                }
                this.update(cx, |view, _| view.send_key(event));
            })
            .on_scroll_wheel({
                let backend = backend.clone();
                move |event, _window, _cx| {
                    let delta = match event.delta {
                        ScrollDelta::Lines(point) => point.y,
                        ScrollDelta::Pixels(point) => {
                            backend.line_delta(f32::from(point.y))
                        }
                    };
                    if delta.abs() > f32::EPSILON {
                        let button =
                            if delta.is_sign_negative() { 64 } else { 65 };
                        if !backend.send_mouse_report(
                            button,
                            event.position,
                            true,
                            event.modifiers,
                        ) {
                            backend.scroll_lines(delta.round() as i32);
                        }
                    }
                }
            })
            .child(
                div()
                    .id("agent-terminal-scroll")
                    .size_full()
                    .min_h_0()
                    .p_2()
                    .relative()
                    .overflow_hidden()
                    .font_family(mono)
                    .text_size(px(13.))
                    .child(terminal_canvas),
            );
        if let Some(error) = error {
            surface = surface.child(
                div()
                    .absolute()
                    .bottom_2()
                    .left_2()
                    .text_color(colors.red)
                    .text_size(px(11.))
                    .child(SharedString::from(error)),
            );
        }
        surface
    }
}

struct TerminalFrame {
    geometry: TerminalGeometry,
    snapshot: TerminalSnapshot,
    generation: u64,
}

struct StyledCanvasState {
    frame: TerminalFrame,
    plan: StyledRenderPlan,
    plain_plan: PlainRenderPlan,
}

fn paint_styled_terminal(
    bounds: Bounds<Pixels>,
    state: StyledCanvasState,
    colors: &gpui_component::theme::ThemeColor,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        window.paint_quad(fill(bounds, colors.background));

        let geometry = state.frame.geometry;
        let snapshot = &state.frame.snapshot;
        if snapshot.columns != geometry.columns as usize
            || snapshot.screen_lines != geometry.lines as usize
        {
            log::debug!(
                "[agent_terminal] skipped stale terminal frame generation={} snapshot={}x{} geometry={}x{}",
                state.frame.generation,
                snapshot.columns,
                snapshot.screen_lines,
                geometry.columns,
                geometry.lines,
            );
            return;
        }

        let text_style = window.text_style();
        let font = text_style.font();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = px(geometry.line_height);
        let cell_width = px(geometry.cell_width);
        let origin_x = px(geometry.origin_x);
        let origin_y = px(geometry.origin_y);
        for region in state.plan.backgrounds {
            let color = terminal_color_to_hsla(
                region.color,
                &snapshot.palette,
                colors,
            );
            let origin = point(
                origin_x + px(region.start_column as f32 * geometry.cell_width),
                origin_y + px(region.line as f32 * geometry.line_height),
            );
            let region_bounds = Bounds::new(
                origin,
                size(
                    px((region.end_column - region.start_column + 1) as f32
                        * geometry.cell_width),
                    line_height,
                ),
            );
            window.paint_quad(fill(region_bounds, color));
        }

        for region in state.plan.selections {
            let origin = point(
                origin_x + px(region.start_column as f32 * geometry.cell_width),
                origin_y + px(region.line as f32 * geometry.line_height),
            );
            let region_bounds = Bounds::new(
                origin,
                size(
                    px((region.end_column - region.start_column) as f32
                        * geometry.cell_width),
                    line_height,
                ),
            );
            window.paint_quad(fill(region_bounds, colors.selection));
        }

        let mut styled_paint_failed = false;
        for run in &state.plan.runs {
            if run.text.is_empty() || run.cell_count == 0 {
                continue;
            }
            let text = SharedString::from(run.text.clone());
            let mut foreground = terminal_color_to_hsla(
                run.style.foreground,
                &snapshot.palette,
                colors,
            );
            if CellFlags::from_bits_retain(run.style.flags)
                .contains(CellFlags::DIM)
            {
                foreground.fade_out(0.4);
            }
            let text_run = terminal_text_run(
                run.style,
                text.len(),
                font.clone(),
                foreground,
            );
            let shaped = window.text_system().shape_line(
                text.clone(),
                font_size,
                &[text_run],
                Some(cell_width),
            );
            let origin = point(
                origin_x + px(run.column as f32 * geometry.cell_width),
                origin_y + px(run.line as f32 * geometry.line_height),
            );
            if let Err(error) = shaped.paint(
                origin,
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            ) {
                log::debug!(
                    "[agent_terminal] styled text paint failed; using plain fallback: {error}"
                );
                styled_paint_failed = true;
                break;
            }
        }

        if styled_paint_failed {
            for run in state.plain_plan.runs {
                if run.text.is_empty() || run.cell_count == 0 {
                    continue;
                }
                let text = SharedString::from(run.text);
                let fallback_run = TextRun {
                    len: text.len(),
                    font: font.clone(),
                    color: colors.foreground,
                    ..TextRun::default()
                };
                let fallback = window.text_system().shape_line(
                    text,
                    font_size,
                    &[fallback_run],
                    Some(cell_width),
                );
                let origin = point(
                    origin_x + px(run.column as f32 * geometry.cell_width),
                    origin_y + px(run.line as f32 * geometry.line_height),
                );
                let _ = fallback.paint(
                    origin,
                    line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        }

        if let Some((line, column)) = state.plan.cursor
            && line < geometry.lines as usize
            && column < geometry.columns as usize
        {
            let cursor_bounds = Bounds::new(
                point(
                    origin_x + px(column as f32 * geometry.cell_width),
                    origin_y + px(line as f32 * geometry.line_height),
                ),
                size(cell_width, line_height),
            );
            window.paint_quad(fill(
                cursor_bounds,
                colors.foreground.opacity(0.35),
            ));
        }
    });
}

fn terminal_geometry_for_bounds(
    bounds: Bounds<Pixels>,
    window: &Window,
) -> TerminalGeometry {
    let text_style = window.text_style();
    let font_id = window.text_system().resolve_font(&text_style.font());
    let font_size = text_style.font_size.to_pixels(window.rem_size());
    let cell_width = window
        .text_system()
        .advance(font_id, font_size, 'm')
        .map(|advance| f32::from(advance.width))
        .ok()
        .filter(|width| width.is_finite() && *width > 0.)
        .unwrap_or(f32::from(CELL_WIDTH));
    let line_height =
        f32::from(text_style.line_height_in_pixels(window.rem_size()));
    TerminalGeometry::from_bounds(
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
        cell_width,
        line_height,
        window.scale_factor(),
    )
}
