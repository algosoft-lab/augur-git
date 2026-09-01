//! Fixed-grid planning for the terminal's first rendering path.
//!
//! The planner deliberately knows nothing about GPUI. It turns the
//! coordinate-rich terminal snapshot into runs that can be painted at exact
//! cell positions. The plain plan remains available as a conservative
//! fallback while the styled plan adds ANSI colors and decorations.

use alacritty_terminal::term::cell::Flags as CellFlags;
use gpui::{
    FontStyle, FontWeight, Hsla, Rgba, StrikethroughStyle, UnderlineStyle, px,
};
use gpui_component::theme::ThemeColor;

use super::model::{TerminalCellSnapshot, TerminalColor, TerminalSnapshot};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlainTextRun {
    pub line: usize,
    pub column: usize,
    pub text: String,
    pub cell_count: usize,
    pub has_wide_character: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PlainRenderPlan {
    pub runs: Vec<PlainTextRun>,
    pub cursor: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalTextStyle {
    pub foreground: TerminalColor,
    pub flags: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StyledTextRun {
    pub line: usize,
    pub column: usize,
    pub text: String,
    pub cell_count: usize,
    pub style: TerminalTextStyle,
    pub has_wide_character: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalBackgroundRegion {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub color: TerminalColor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSelectionRegion {
    pub line: usize,
    pub start_column: usize,
    pub end_column: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StyledRenderPlan {
    pub runs: Vec<StyledTextRun>,
    pub backgrounds: Vec<TerminalBackgroundRegion>,
    pub selections: Vec<TerminalSelectionRegion>,
    pub cursor: Option<(usize, usize)>,
}

pub(crate) fn build_plain_render_plan(
    snapshot: &TerminalSnapshot,
) -> PlainRenderPlan {
    let mut runs = Vec::new();
    let mut line_numbers = Vec::new();
    let mut start = 0;

    while start < snapshot.cells.len() {
        let line = snapshot.cells[start].line;
        line_numbers.push(line);
        let mut end = start + 1;
        while end < snapshot.cells.len() && snapshot.cells[end].line == line {
            end += 1;
        }
        append_line_runs(
            &snapshot.cells[start..end],
            line_numbers.len() - 1,
            &mut runs,
        );
        start = end;
    }

    let cursor = snapshot.cursor.and_then(|cursor| {
        line_numbers
            .iter()
            .position(|line| *line == cursor.line)
            .map(|line| (line, cursor.column))
    });

    PlainRenderPlan { runs, cursor }
}

fn append_line_runs(
    cells: &[TerminalCellSnapshot],
    line: usize,
    runs: &mut Vec<PlainTextRun>,
) {
    let Some(first) = cells.iter().position(is_renderable_cell) else {
        return;
    };
    let last = cells.iter().rposition(is_renderable_cell).unwrap_or(first);
    let mut current: Option<PlainTextRun> = None;

    for cell in &cells[first..=last] {
        let flags = CellFlags::from_bits_retain(cell.flags);
        if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            continue;
        }

        let cell_width = if flags.contains(CellFlags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let blank = cell.character == ' ' && cell.zero_width.is_empty();
        if blank {
            if let Some(run) = current.as_mut()
                && !run.has_wide_character
                && run.column + run.cell_count == cell.column
            {
                run.text.push(' ');
                run.cell_count += 1;
            }
            continue;
        }

        let can_append = cell_width == 1
            && current.as_ref().is_some_and(|run| {
                !run.has_wide_character
                    && run.column + run.cell_count == cell.column
            });
        if !can_append {
            if let Some(run) = current.take() {
                runs.push(run);
            }
            current = Some(PlainTextRun {
                line,
                column: cell.column,
                text: String::new(),
                cell_count: 0,
                has_wide_character: false,
            });
        }
        let Some(run) = current.as_mut() else {
            continue;
        };
        run.text.push(cell.character);
        run.text.extend(cell.zero_width.iter().copied());
        run.cell_count += cell_width;
        run.has_wide_character |= cell_width == 2;
    }

    if let Some(run) = current {
        runs.push(run);
    }
}

fn is_renderable_cell(cell: &TerminalCellSnapshot) -> bool {
    let flags = CellFlags::from_bits_retain(cell.flags);
    !flags.contains(CellFlags::WIDE_CHAR_SPACER)
        && (cell.character != ' ' || !cell.zero_width.is_empty())
}

pub(crate) fn build_styled_render_plan(
    snapshot: &TerminalSnapshot,
) -> StyledRenderPlan {
    let mut runs = Vec::new();
    let mut backgrounds = Vec::new();
    let mut selections = Vec::new();
    let mut line_numbers = Vec::new();
    let mut start = 0;

    while start < snapshot.cells.len() {
        let line = snapshot.cells[start].line;
        line_numbers.push(line);
        let mut end = start + 1;
        while end < snapshot.cells.len() && snapshot.cells[end].line == line {
            end += 1;
        }
        let cells = &snapshot.cells[start..end];
        append_background_regions(
            cells,
            line_numbers.len() - 1,
            &mut backgrounds,
        );
        append_styled_line_runs(cells, line_numbers.len() - 1, &mut runs);
        start = end;
    }

    let cursor = snapshot.cursor.and_then(|cursor| {
        line_numbers
            .iter()
            .position(|line| *line == cursor.line)
            .map(|line| (line, cursor.column))
    });

    append_selection_regions(snapshot, &line_numbers, &mut selections);

    StyledRenderPlan {
        runs,
        backgrounds,
        selections,
        cursor,
    }
}

fn append_background_regions(
    cells: &[TerminalCellSnapshot],
    line: usize,
    regions: &mut Vec<TerminalBackgroundRegion>,
) {
    for cell in cells {
        let (_, background) = effective_colors(cell);
        if background == TerminalColor::Named(257) {
            continue;
        }
        let Some(last) = regions.last_mut() else {
            regions.push(TerminalBackgroundRegion {
                line,
                start_column: cell.column,
                end_column: cell.column,
                color: background,
            });
            continue;
        };
        if last.line == line
            && last.color == background
            && last.end_column.saturating_add(1) == cell.column
        {
            last.end_column = cell.column;
        } else {
            regions.push(TerminalBackgroundRegion {
                line,
                start_column: cell.column,
                end_column: cell.column,
                color: background,
            });
        }
    }
}

fn append_selection_regions(
    snapshot: &TerminalSnapshot,
    line_numbers: &[i32],
    regions: &mut Vec<TerminalSelectionRegion>,
) {
    let Some(selection) = snapshot.selection else {
        return;
    };
    let start_line = selection.start.line.0.min(selection.end.line.0);
    let end_line = selection.start.line.0.max(selection.end.line.0);
    let start_column = selection.start.column.0.min(snapshot.columns);
    let end_column = selection
        .end
        .column
        .0
        .saturating_add(1)
        .min(snapshot.columns);
    for (line, line_number) in line_numbers.iter().copied().enumerate() {
        if !(start_line..=end_line).contains(&line_number) {
            continue;
        }
        let (start_column, end_column) = if selection.is_block {
            (start_column, end_column)
        } else if line_number == start_line && line_number == end_line {
            (start_column, end_column)
        } else if line_number == start_line {
            (start_column, snapshot.columns)
        } else if line_number == end_line {
            (0, end_column)
        } else {
            (0, snapshot.columns)
        };
        if start_column < end_column {
            regions.push(TerminalSelectionRegion {
                line,
                start_column,
                end_column,
            });
        }
    }
}

fn append_styled_line_runs(
    cells: &[TerminalCellSnapshot],
    line: usize,
    runs: &mut Vec<StyledTextRun>,
) {
    let Some(first) = cells.iter().position(is_renderable_cell) else {
        return;
    };
    let last = cells.iter().rposition(is_renderable_cell).unwrap_or(first);
    let mut current: Option<StyledTextRun> = None;

    for cell in &cells[first..=last] {
        let flags = CellFlags::from_bits_retain(cell.flags);
        if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            continue;
        }
        let (foreground, _) = effective_colors(cell);
        let style = TerminalTextStyle {
            foreground,
            flags: cell.flags,
        };
        let cell_width = if flags.contains(CellFlags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let blank = cell.character == ' ' && cell.zero_width.is_empty();
        if blank {
            if let Some(run) = current.as_mut()
                && !run.has_wide_character
                && run.style == style
                && run.column + run.cell_count == cell.column
            {
                run.text.push(' ');
                run.cell_count += 1;
            }
            continue;
        }

        let can_append = cell_width == 1
            && current.as_ref().is_some_and(|run| {
                !run.has_wide_character
                    && run.style == style
                    && run.column + run.cell_count == cell.column
            });
        if !can_append {
            if let Some(run) = current.take() {
                runs.push(run);
            }
            current = Some(StyledTextRun {
                line,
                column: cell.column,
                text: String::new(),
                cell_count: 0,
                style,
                has_wide_character: false,
            });
        }
        let Some(run) = current.as_mut() else {
            continue;
        };
        run.text.push(cell.character);
        run.text.extend(cell.zero_width.iter().copied());
        run.cell_count += cell_width;
        run.has_wide_character |= cell_width == 2;
    }

    if let Some(run) = current {
        runs.push(run);
    }
}

fn effective_colors(
    cell: &TerminalCellSnapshot,
) -> (TerminalColor, TerminalColor) {
    let flags = CellFlags::from_bits_retain(cell.flags);
    let mut foreground = cell.foreground;
    let mut background = cell.background;
    if flags.contains(CellFlags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }
    (foreground, background)
}

pub(crate) fn terminal_color_to_hsla(
    color: TerminalColor,
    palette: &[Option<TerminalColor>],
    colors: &ThemeColor,
) -> Hsla {
    if let TerminalColor::Named(index) = color
        && let Some(Some(TerminalColor::Rgb { r, g, b })) =
            palette.get(index as usize)
    {
        return rgb_color(*r, *g, *b);
    }
    match color {
        TerminalColor::Rgb { r, g, b } => rgb_color(r, g, b),
        TerminalColor::Indexed(index) => {
            let (r, g, b) = xterm_rgb(index);
            rgb_color(r, g, b)
        }
        TerminalColor::Named(index) => match index {
            256 | 267 => colors.foreground,
            257 | 268 => colors.background,
            258 => colors.foreground,
            259..=266 => {
                let base = index.saturating_sub(259);
                let (r, g, b) = xterm_rgb(base as u8);
                rgb_color(r / 2, g / 2, b / 2)
            }
            _ => {
                let (r, g, b) = xterm_rgb(index.min(255) as u8);
                rgb_color(r, g, b)
            }
        },
    }
}

pub(crate) fn terminal_text_run(
    style: TerminalTextStyle,
    text_len: usize,
    font: gpui::Font,
    foreground: Hsla,
) -> gpui::TextRun {
    let flags = CellFlags::from_bits_retain(style.flags);
    let weight = flags
        .contains(CellFlags::BOLD)
        .then_some(FontWeight::BOLD)
        .unwrap_or(font.weight);
    let font_style = flags
        .contains(CellFlags::ITALIC)
        .then_some(FontStyle::Italic)
        .unwrap_or(font.style);
    let font = gpui::Font {
        weight,
        style: font_style,
        ..font
    };
    gpui::TextRun {
        len: text_len,
        font,
        color: foreground,
        underline: flags.intersects(CellFlags::ALL_UNDERLINES).then_some(
            UnderlineStyle {
                thickness: px(1.),
                color: Some(foreground),
                wavy: flags.contains(CellFlags::UNDERCURL),
            },
        ),
        strikethrough: flags.contains(CellFlags::STRIKEOUT).then_some(
            StrikethroughStyle {
                thickness: px(1.),
                color: Some(foreground),
            },
        ),
        ..gpui::TextRun::default()
    }
}

fn rgb_color(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: 1.,
    }
    .into()
}

pub(crate) fn xterm_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0 => (0, 0, 0),
        1 => (205, 49, 49),
        2 => (13, 188, 121),
        3 => (229, 229, 16),
        4 => (36, 114, 200),
        5 => (188, 63, 188),
        6 => (17, 168, 205),
        7 => (229, 229, 229),
        8 => (102, 102, 102),
        9 => (241, 76, 76),
        10 => (35, 209, 139),
        11 => (245, 245, 67),
        12 => (59, 142, 234),
        13 => (214, 112, 214),
        14 => (41, 184, 219),
        15 => (255, 255, 255),
        16..=231 => {
            let value = index - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            (cube_component(r), cube_component(g), cube_component(b))
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            (value, value, value)
        }
    }
}

fn cube_component(value: u8) -> u8 {
    if value == 0 { 0 } else { value * 40 + 55 }
}

#[cfg(test)]
mod tests {
    use super::{
        PlainTextRun, build_plain_render_plan, build_styled_render_plan,
        terminal_color_to_hsla,
    };
    use crate::terminal::model::{
        TerminalCellSnapshot, TerminalColor, TerminalPointSnapshot,
        TerminalSnapshot,
    };
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line, Point};
    use alacritty_terminal::selection::SelectionRange;
    use alacritty_terminal::term::cell::Flags as CellFlags;
    use gpui_component::theme::ThemeColor;

    fn cell(line: i32, column: usize, character: char) -> TerminalCellSnapshot {
        TerminalCellSnapshot {
            line,
            column,
            character,
            zero_width: Vec::new(),
            foreground: TerminalColor::Named(256),
            background: TerminalColor::Named(257),
            flags: 0,
        }
    }

    #[test]
    fn keeps_grid_columns_when_building_plain_runs() {
        let mut wide = cell(4, 3, '中');
        wide.flags = (CellFlags::WIDE_CHAR).bits();
        let mut spacer = cell(4, 4, ' ');
        spacer.flags = CellFlags::WIDE_CHAR_SPACER.bits();
        let snapshot = TerminalSnapshot {
            cells: vec![
                cell(4, 0, ' '),
                cell(4, 1, 'A'),
                cell(4, 2, ' '),
                wide,
                spacer,
                cell(4, 5, 'B'),
            ],
            cursor: Some(TerminalPointSnapshot { line: 4, column: 5 }),
            ..TerminalSnapshot::default()
        };
        let plan = build_plain_render_plan(&snapshot);
        assert_eq!(
            plan.runs,
            vec![
                PlainTextRun {
                    line: 0,
                    column: 1,
                    text: "A ".to_string(),
                    cell_count: 2,
                    has_wide_character: false,
                },
                PlainTextRun {
                    line: 0,
                    column: 3,
                    text: "中".to_string(),
                    cell_count: 2,
                    has_wide_character: true,
                },
                PlainTextRun {
                    line: 0,
                    column: 5,
                    text: "B".to_string(),
                    cell_count: 1,
                    has_wide_character: false,
                },
            ]
        );
        assert_eq!(plan.cursor, Some((0, 5)));
    }

    #[test]
    fn combining_marks_stay_in_the_base_cell_run() {
        let mut base = cell(0, 0, 'e');
        base.zero_width.push('\u{301}');
        let snapshot = TerminalSnapshot {
            cells: vec![base, cell(0, 1, 'x')],
            ..TerminalSnapshot::default()
        };
        let plan = build_plain_render_plan(&snapshot);
        assert_eq!(plan.runs[0].text, "e\u{301}x");
        assert_eq!(plan.runs[0].cell_count, 2);
    }

    #[test]
    fn blank_rows_do_not_create_text_runs_but_keep_cursor_line_mapping() {
        let snapshot = TerminalSnapshot {
            cells: vec![cell(0, 0, ' '), cell(1, 0, 'x')],
            cursor: Some(TerminalPointSnapshot { line: 0, column: 2 }),
            ..TerminalSnapshot::default()
        };
        let plan = build_plain_render_plan(&snapshot);
        assert!(plan.runs.iter().all(|run| run.line == 1));
        assert_eq!(plan.cursor, Some((0, 2)));
    }

    #[test]
    fn styled_plan_merges_backgrounds_and_resolves_inverse_before_painting() {
        let mut first = cell(0, 1, 'A');
        first.background = TerminalColor::Named(1);
        let mut second = cell(0, 2, 'B');
        second.background = TerminalColor::Named(1);
        let mut inverse = cell(0, 3, 'C');
        inverse.foreground = TerminalColor::Named(2);
        inverse.flags = CellFlags::INVERSE.bits();
        let snapshot = TerminalSnapshot {
            columns: 8,
            cells: vec![cell(0, 0, ' '), first, second, inverse],
            selection: Some(SelectionRange {
                start: Point::new(Line(0), Column(1)),
                end: Point::new(Line(0), Column(2)),
                is_block: false,
            }),
            ..TerminalSnapshot::default()
        };
        let plan = build_styled_render_plan(&snapshot);
        assert_eq!(plan.backgrounds.len(), 2);
        assert_eq!(plan.backgrounds[0].start_column, 1);
        assert_eq!(plan.backgrounds[0].end_column, 2);
        assert_eq!(plan.backgrounds[1].start_column, 3);
        assert_eq!(plan.backgrounds[1].color, TerminalColor::Named(2));
        assert_eq!(plan.selections[0].start_column, 1);
        assert_eq!(plan.selections[0].end_column, 3);
        assert_eq!(plan.runs[1].style.foreground, TerminalColor::Named(257));
    }

    #[test]
    fn dim_background_does_not_turn_into_foreground() {
        let mut colors = ThemeColor::default();
        colors.foreground = gpui::Hsla::white();
        colors.background = gpui::Hsla::black();
        let mapped =
            terminal_color_to_hsla(TerminalColor::Named(268), &[], &colors);
        assert_eq!(mapped, colors.background);
        assert_ne!(mapped, colors.foreground);
    }

    #[test]
    fn ansi_fixture_keeps_truecolor_and_inverse_cell_coordinates() {
        let dimensions = alacritty_terminal::term::test::TermSize::new(20, 2);
        let mut terminal = alacritty_terminal::term::Term::new(
            alacritty_terminal::term::Config::default(),
            &dimensions,
            alacritty_terminal::event::VoidListener,
        );
        let mut processor = alacritty_terminal::vte::ansi::Processor::<
            alacritty_terminal::vte::ansi::StdSyncHandler,
        >::new();
        processor.advance(
            &mut terminal,
            b"\x1b[38;2;1;2;3mA\x1b[48;2;4;5;6mB\x1b[7mC",
        );
        let snapshot = TerminalSnapshot::from_renderable(
            terminal.renderable_content(),
            terminal.columns(),
            terminal.screen_lines(),
        );
        let plan = build_styled_render_plan(&snapshot);
        assert_eq!(
            plan.runs[0].style.foreground,
            TerminalColor::Rgb { r: 1, g: 2, b: 3 }
        );
        assert_eq!(plan.runs[0].text, "AB");
        assert_eq!(plan.runs[0].cell_count, 2);
        assert_eq!(
            plan.backgrounds[0].color,
            TerminalColor::Rgb { r: 4, g: 5, b: 6 }
        );
        assert_eq!(
            plan.runs[1].style.foreground,
            TerminalColor::Rgb { r: 4, g: 5, b: 6 }
        );
        assert_eq!(plan.runs[1].column, 2);
    }

    #[test]
    fn selection_regions_share_the_same_line_and_column_geometry() {
        let snapshot = TerminalSnapshot {
            columns: 6,
            cells: (0..6)
                .map(|column| cell(0, column, 'a'))
                .chain((0..6).map(|column| cell(1, column, 'b')))
                .collect(),
            selection: Some(SelectionRange {
                start: Point::new(Line(0), Column(2)),
                end: Point::new(Line(1), Column(3)),
                is_block: false,
            }),
            ..TerminalSnapshot::default()
        };
        let plan = build_styled_render_plan(&snapshot);
        assert_eq!(
            plan.selections,
            vec![
                super::TerminalSelectionRegion {
                    line: 0,
                    start_column: 2,
                    end_column: 6,
                },
                super::TerminalSelectionRegion {
                    line: 1,
                    start_column: 0,
                    end_column: 4,
                },
            ]
        );
    }
}
