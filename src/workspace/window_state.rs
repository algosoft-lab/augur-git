use gpui::{
    App, Bounds, Window, WindowBounds, WindowDecorations, WindowOptions, point,
    px, size,
};
use gpui_component::TitleBar;

use crate::core::config::{
    DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, MIN_WINDOW_HEIGHT,
    MIN_WINDOW_WIDTH, UiState, WindowState,
};

pub fn initial_window_options(
    cx: &mut App,
    state: &WindowState,
) -> WindowOptions {
    let primary_display = cx.primary_display();
    let selected_display = select_display(cx, state, primary_display.clone());
    let (window_bounds, display_id) = selected_display
        .map(|display| {
            let visible_bounds = display.visible_bounds();
            let bounds = restore_bounds(state, visible_bounds);
            let window_bounds = if state.maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            };
            (window_bounds, Some(display.id()))
        })
        .unwrap_or_else(|| {
            let desired_size = size(
                px(DEFAULT_WINDOW_WIDTH as f32),
                px(DEFAULT_WINDOW_HEIGHT as f32),
            );
            let bounds = Bounds::centered(None, desired_size, cx);
            let window_bounds = if state.maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            };
            (window_bounds, None)
        });

    WindowOptions {
        window_bounds: Some(window_bounds),
        display_id,
        titlebar: Some(TitleBar::title_bar_options()),
        window_decorations: Some(WindowDecorations::Client),
        window_min_size: Some(gpui::Size {
            width: px(MIN_WINDOW_WIDTH as f32),
            height: px(MIN_WINDOW_HEIGHT as f32),
        }),
        ..Default::default()
    }
}

pub fn capture_window_state(window: &Window) -> WindowState {
    let bounds = window.window_bounds().get_bounds();
    let mut state = WindowState {
        x: Some(f32::from(bounds.origin.x).round() as i32),
        y: Some(f32::from(bounds.origin.y).round() as i32),
        width: f32::from(bounds.size.width).round() as u32,
        height: f32::from(bounds.size.height).round() as u32,
        maximized: matches!(window.window_bounds(), WindowBounds::Maximized(_)),
    };
    state.normalize();
    state
}

fn select_display(
    cx: &App,
    state: &WindowState,
    primary_display: Option<std::rc::Rc<dyn gpui::PlatformDisplay>>,
) -> Option<std::rc::Rc<dyn gpui::PlatformDisplay>> {
    let Some((x, y)) = state.x.zip(state.y) else {
        return primary_display.or_else(|| cx.displays().into_iter().next());
    };
    let saved_width = state.width.max(MIN_WINDOW_WIDTH) as f32;
    let saved_height = state.height.max(MIN_WINDOW_HEIGHT) as f32;
    cx.displays()
        .into_iter()
        .find(|display| {
            intersects(
                x as f32,
                y as f32,
                saved_width,
                saved_height,
                display.visible_bounds(),
            )
        })
        .or(primary_display)
        .or_else(|| cx.displays().into_iter().next())
}

fn restore_bounds(
    state: &WindowState,
    visible_bounds: Bounds<gpui::Pixels>,
) -> Bounds<gpui::Pixels> {
    let display_x = f32::from(visible_bounds.origin.x);
    let display_y = f32::from(visible_bounds.origin.y);
    let display_width = f32::from(visible_bounds.size.width);
    let display_height = f32::from(visible_bounds.size.height);
    let width = (state.width.max(MIN_WINDOW_WIDTH) as f32).min(display_width);
    let height =
        (state.height.max(MIN_WINDOW_HEIGHT) as f32).min(display_height);
    let centered_x = display_x + (display_width - width) / 2.0;
    let centered_y = display_y + (display_height - height) / 2.0;
    let x = state
        .x
        .map(|value| value as f32)
        .unwrap_or(centered_x)
        .clamp(display_x, display_x + display_width - width);
    let y = state
        .y
        .map(|value| value as f32)
        .unwrap_or(centered_y)
        .clamp(display_y, display_y + display_height - height);

    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

fn intersects(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    visible_bounds: Bounds<gpui::Pixels>,
) -> bool {
    let visible_x = f32::from(visible_bounds.origin.x);
    let visible_y = f32::from(visible_bounds.origin.y);
    let visible_width = f32::from(visible_bounds.size.width);
    let visible_height = f32::from(visible_bounds.size.height);
    x < visible_x + visible_width
        && x + width > visible_x
        && y < visible_y + visible_height
        && y + height > visible_y
}

pub fn update_ui_state_window(state: &mut UiState, window: &Window) {
    state.window = capture_window_state(window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_bounds_clamps_offscreen_window() {
        let state = WindowState {
            x: Some(5000),
            y: Some(5000),
            width: 1000,
            height: 700,
            maximized: false,
        };
        let visible =
            Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
        let bounds = restore_bounds(&state, visible);
        assert_eq!(f32::from(bounds.origin.x), 920.0);
        assert_eq!(f32::from(bounds.origin.y), 380.0);
    }

    #[test]
    fn restore_bounds_uses_defaults_without_position() {
        let state = WindowState::default();
        let visible =
            Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
        let bounds = restore_bounds(&state, visible);
        assert_eq!(f32::from(bounds.origin.x), 320.0);
        assert_eq!(f32::from(bounds.origin.y), 140.0);
    }
}
