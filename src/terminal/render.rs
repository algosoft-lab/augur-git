//! Fixed-grid planning for the terminal's first rendering path.
//!
//! The planner deliberately knows nothing about GPUI. It turns the
//! coordinate-rich terminal snapshot into runs that can be painted at exact
//! cell positions. ANSI colors and decorations are restored in a later pass;
//! keeping this plain path small gives us a reliable geometry baseline first.

use alacritty_terminal::term::cell::Flags as CellFlags;

use super::model::{TerminalCellSnapshot, TerminalSnapshot};

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

#[cfg(test)]
mod tests {
    use super::{PlainTextRun, build_plain_render_plan};
    use crate::terminal::model::{
        TerminalCellSnapshot, TerminalColor, TerminalPointSnapshot,
        TerminalSnapshot,
    };
    use alacritty_terminal::term::cell::Flags as CellFlags;

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
}
