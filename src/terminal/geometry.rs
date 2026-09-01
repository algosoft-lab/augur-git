//! Terminal viewport geometry shared by layout, input, and PTY resize code.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerminalGeometry {
    pub origin_x: f32,
    pub origin_y: f32,
    pub width: f32,
    pub height: f32,
    pub cell_width: f32,
    pub line_height: f32,
    pub columns: u16,
    pub lines: u16,
}

impl Default for TerminalGeometry {
    fn default() -> Self {
        Self {
            origin_x: 0.,
            origin_y: 0.,
            width: 0.,
            height: 0.,
            cell_width: 8.,
            line_height: 18.,
            columns: 120,
            lines: 32,
        }
    }
}

impl TerminalGeometry {
    pub(crate) fn from_bounds(
        origin_x: f32,
        origin_y: f32,
        width: f32,
        height: f32,
        cell_width: f32,
        line_height: f32,
        scale_factor: f32,
    ) -> Self {
        let scale_factor = scale_factor.max(1.);
        let origin_x = snap(origin_x, scale_factor);
        let origin_y = snap(origin_y, scale_factor);
        let width = snap(width.max(0.), scale_factor);
        let height = snap(height.max(0.), scale_factor);
        let cell_width = snap(cell_width.max(1.), scale_factor);
        let line_height = snap(line_height.max(1.), scale_factor);
        let columns = grid_count(width, cell_width, 2);
        let lines = grid_count(height, line_height, 1);
        Self {
            origin_x,
            origin_y,
            width,
            height,
            cell_width,
            line_height,
            columns,
            lines,
        }
    }

    pub(crate) fn local_position(&self, x: f32, y: f32) -> (f32, f32) {
        (
            (x - self.origin_x).clamp(0., self.width.max(0.)),
            (y - self.origin_y).clamp(0., self.height.max(0.)),
        )
    }

    pub(crate) fn line_delta(&self, pixels: f32) -> f32 {
        pixels / self.line_height.max(1.)
    }
}

pub(crate) fn grid_count(size: f32, cell: f32, minimum: u16) -> u16 {
    if !size.is_finite() || !cell.is_finite() || cell <= 0. {
        return minimum;
    }
    (size / cell).floor().max(f32::from(minimum)) as u16
}

fn snap(value: f32, scale_factor: f32) -> f32 {
    (value * scale_factor).round() / scale_factor
}

#[cfg(test)]
mod tests {
    use super::{TerminalGeometry, grid_count};

    #[test]
    fn computes_grid_from_the_actual_bounds() {
        let geometry = TerminalGeometry::from_bounds(
            37.25, 112.75, 801.2, 407.6, 8.75, 19.25, 1.5,
        );
        assert_eq!(geometry.origin_x, 37.333_332);
        assert_eq!(geometry.origin_y, 112.666_664);
        assert_eq!(geometry.columns, 92);
        assert_eq!(geometry.lines, 21);
    }

    #[test]
    fn converts_window_coordinates_to_clamped_viewport_coordinates() {
        let geometry =
            TerminalGeometry::from_bounds(100., 200., 80., 40., 8., 20., 1.);
        assert_eq!(geometry.local_position(107., 214.), (7., 14.));
        assert_eq!(geometry.local_position(1., 2.), (0., 0.));
        assert_eq!(geometry.local_position(999., 999.), (80., 40.));
        assert_eq!(geometry.line_delta(30.), 1.5);
    }

    #[test]
    fn guards_invalid_cell_sizes_and_small_viewports() {
        assert_eq!(grid_count(100., 0., 2), 2);
        assert_eq!(grid_count(f32::NAN, 8., 2), 2);
        assert_eq!(grid_count(1., 8., 2), 2);
    }
}
