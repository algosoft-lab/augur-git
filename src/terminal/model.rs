//! Data-only snapshots of the parsed terminal grid.
//!
//! Keeping the snapshot independent from GPUI makes the cell coordinates and
//! ANSI state easy to test before the renderer consumes them.

use alacritty_terminal::grid::GridIterator;
use alacritty_terminal::index::Point as TerminalPoint;
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::color::{COUNT, Colors};
use alacritty_terminal::term::{RenderableContent, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, Rgb};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalColor {
    /// A named color which was not overridden by the terminal palette.
    Named(u16),
    Indexed(u8),
    Rgb {
        r: u8,
        g: u8,
        b: u8,
    },
}

impl TerminalColor {
    pub(crate) fn rgb(rgb: Rgb) -> Self {
        Self::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalPointSnapshot {
    pub line: i32,
    pub column: usize,
}

impl TerminalPointSnapshot {
    pub(crate) fn new(point: TerminalPoint) -> Self {
        Self {
            line: point.line.0,
            column: point.column.0,
        }
    }
}

/// One terminal cell with its original grid coordinate.
///
/// Wide-character spacer cells are intentionally kept here. They are skipped
/// only by a renderer after it has used their coordinates to preserve the
/// terminal grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalCellSnapshot {
    pub line: i32,
    pub column: usize,
    pub character: char,
    pub zero_width: Vec<char>,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub flags: u16,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TerminalSnapshot {
    pub columns: usize,
    pub screen_lines: usize,
    pub display_offset: usize,
    pub cells: Vec<TerminalCellSnapshot>,
    pub cursor: Option<TerminalPointSnapshot>,
    pub alternate_screen: bool,
    /// Visible row ranges covered by the current host-side selection.
    pub selection: Option<SelectionRange>,
    /// Palette overrides copied from the terminal while it was locked.
    ///
    /// `None` means the terminal is using the renderer's default ANSI color
    /// for that entry. Keeping the palette in the snapshot prevents a later
    /// paint pass from racing with terminal escape-sequence updates.
    pub palette: Vec<Option<TerminalColor>>,

    // Transitional compatibility for the existing renderer. The fixed-grid
    // renderer will consume `cells` directly and these fields can then be
    // removed without changing the PTY model again.
    pub rows: Vec<String>,
    pub cell_rows: Vec<Vec<TerminalCellSnapshot>>,
    pub legacy_cursor: Option<(usize, usize)>,
}

impl TerminalSnapshot {
    pub(crate) fn from_renderable(
        content: RenderableContent<'_>,
        columns: usize,
        screen_lines: usize,
    ) -> Self {
        let RenderableContent {
            display_iter,
            selection,
            cursor,
            display_offset,
            colors,
            mode,
        } = content;
        let palette = copy_palette(colors);
        let (cells, rows, cell_rows) = collect_cells(display_iter, &palette);
        let cursor = match cursor.shape {
            CursorShape::Hidden => None,
            _ => Some(TerminalPointSnapshot::new(cursor.point)),
        };
        let legacy_cursor = cursor.map(|point| {
            (
                (point.line + display_offset as i32).max(0) as usize,
                point.column,
            )
        });

        Self {
            columns,
            screen_lines,
            display_offset,
            cells,
            cursor,
            alternate_screen: mode.contains(TermMode::ALT_SCREEN),
            selection,
            palette,
            rows,
            cell_rows,
            legacy_cursor,
        }
    }
}

fn copy_palette(colors: &Colors) -> Vec<Option<TerminalColor>> {
    (0..COUNT)
        .map(|index| colors[index].map(TerminalColor::rgb))
        .collect()
}

fn collect_cells(
    display_iter: GridIterator<'_, alacritty_terminal::term::cell::Cell>,
    palette: &[Option<TerminalColor>],
) -> (
    Vec<TerminalCellSnapshot>,
    Vec<String>,
    Vec<Vec<TerminalCellSnapshot>>,
) {
    let mut cells = Vec::new();
    let mut rows = Vec::<String>::new();
    let mut cell_rows = Vec::<Vec<TerminalCellSnapshot>>::new();
    let mut current_line = None;

    for indexed in display_iter {
        let line = indexed.point.line.0;
        if current_line != Some(line) {
            rows.push(String::new());
            cell_rows.push(Vec::new());
            current_line = Some(line);
        }

        let cell = indexed.cell;
        let snapshot = TerminalCellSnapshot {
            line,
            column: indexed.point.column.0,
            character: if cell.flags.contains(CellFlags::HIDDEN) {
                ' '
            } else {
                cell.c
            },
            zero_width: cell.zerowidth().unwrap_or_default().to_vec(),
            foreground: snapshot_color(cell.fg, palette),
            background: snapshot_color(cell.bg, palette),
            flags: cell.flags.bits(),
        };
        cells.push(snapshot.clone());

        // These compatibility rows intentionally preserve the old compressed
        // representation. The coordinate-rich `cells` vector above is the
        // authoritative model for the fixed-grid renderer.
        if !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            if let Some(row) = rows.last_mut() {
                row.push(snapshot.character);
                if !cell.flags.contains(CellFlags::HIDDEN) {
                    row.extend(snapshot.zero_width.iter().copied());
                }
            }
            if let Some(row) = cell_rows.last_mut() {
                row.push(snapshot);
            }
        }
    }

    (cells, rows, cell_rows)
}

fn snapshot_color(
    color: AnsiColor,
    palette: &[Option<TerminalColor>],
) -> TerminalColor {
    match color {
        AnsiColor::Named(named) => palette
            .get(named as usize)
            .and_then(Clone::clone)
            .unwrap_or(TerminalColor::Named(named as u16)),
        AnsiColor::Indexed(index) => TerminalColor::Indexed(index),
        AnsiColor::Spec(rgb) => TerminalColor::rgb(rgb),
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalColor, TerminalSnapshot};
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::{Config as TerminalConfig, Term};
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    fn snapshot(
        output: &[u8],
        columns: usize,
        lines: usize,
    ) -> TerminalSnapshot {
        let dimensions = TermSize::new(columns, lines);
        let mut terminal =
            Term::new(TerminalConfig::default(), &dimensions, VoidListener);
        let mut processor = Processor::<StdSyncHandler>::new();
        processor.advance(&mut terminal, output);
        TerminalSnapshot::from_renderable(
            terminal.renderable_content(),
            terminal.columns(),
            terminal.screen_lines(),
        )
    }

    #[test]
    fn keeps_real_columns_for_wide_and_combining_cells() {
        let snapshot = snapshot("A中e\u{301}B".as_bytes(), 20, 2);

        let visible = snapshot
            .cells
            .iter()
            .filter(|cell| cell.line == 0 && cell.character != ' ')
            .collect::<Vec<_>>();
        assert_eq!(visible[0].column, 0);
        assert_eq!(visible[0].character, 'A');
        assert_eq!(visible[1].column, 1);
        assert_eq!(visible[1].character, '中');
        assert!(snapshot.cells.iter().any(|cell| {
            cell.line == 0
                && cell.column == 2
                && CellFlags::from_bits_retain(cell.flags)
                    .contains(CellFlags::WIDE_CHAR_SPACER)
        }));
        let combining = snapshot
            .cells
            .iter()
            .find(|cell| cell.line == 0 && cell.character == 'e')
            .expect("combining base cell");
        assert_eq!(combining.column, 3);
        assert_eq!(combining.zero_width, vec!['\u{301}']);
        assert_eq!(
            snapshot
                .cells
                .iter()
                .find(|cell| cell.line == 0 && cell.character == 'B')
                .map(|cell| cell.column),
            Some(4)
        );
    }

    #[test]
    fn captures_newlines_clear_cursor_and_alternate_screen() {
        let snapshot = snapshot(
            b"first\r\nsecond\x1b[2J\x1b[3;4Hxy\x1b[?1049h\x1b[2Jalt",
            20,
            5,
        );
        assert!(snapshot.columns == 20 && snapshot.screen_lines == 5);
        assert!(snapshot.rows.iter().any(|row| row.contains("alt")));
        assert!(snapshot.alternate_screen);
        assert_eq!(
            snapshot.cursor.map(|cursor| (cursor.line, cursor.column)),
            Some((2, 8))
        );
    }

    #[test]
    fn preserves_terminal_palette_overrides_in_the_snapshot() {
        let snapshot =
            snapshot(b"\x1b]4;1;rgb:1212/3434/5656\x07\x1b[31mred", 20, 2);
        let red = snapshot
            .cells
            .iter()
            .find(|cell| cell.character == 'r')
            .expect("red cell");
        assert_eq!(
            red.foreground,
            TerminalColor::Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56
            }
        );
    }

    use alacritty_terminal::term::cell::Flags as CellFlags;
}
