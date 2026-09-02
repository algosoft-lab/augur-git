//! Embedded PTY-backed terminal used for visible external Agent diagnostics.
//!
//! The terminal owns no shell policy and does not interpret Agent output. It
//! provides a small GPUI view over `alacritty_terminal`'s cross-platform PTY
//! and ANSI state machine.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Point as TerminalPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config as TerminalConfig, Term, TermMode};
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};
use gpui::*;

use crate::agent::{AgentLaunchSpec, AgentTestDirectory};

mod geometry;
mod model;
mod render;
mod view;

use geometry::TerminalGeometry;
use model::TerminalSnapshot;

pub use view::TerminalView;

const DEFAULT_COLUMNS: usize = 120;
const DEFAULT_LINES: usize = 32;
const CELL_WIDTH: u16 = 8;
const CELL_HEIGHT: u16 = 18;

#[derive(Clone, Debug)]
pub enum TerminalEvent {
    Wakeup,
    ChildExit(Option<i32>),
    Error(String),
}

#[derive(Clone)]
struct TerminalProxy {
    events: Sender<TerminalEvent>,
    test_directory: Option<AgentTestDirectory>,
    /// The terminal parser emits responses to device queries as `PtyWrite`.
    /// Keep those responses inside the PTY; only OSC and other host side
    /// effects are intentionally discarded.
    input_sender: Arc<Mutex<Option<EventLoopSender>>>,
    child_exit_seen: Arc<AtomicBool>,
}

impl TerminalProxy {
    fn cleanup_resources(&self) {
        if let Some(test_directory) = &self.test_directory {
            if test_directory.cleanup().is_err() {
                log::debug!(
                    "[agent_terminal] temporary test directory cleanup deferred"
                );
            }
        }
    }
}

impl EventListener for TerminalProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                let _ = self.events.send(TerminalEvent::Wakeup);
            }
            Event::ChildExit(status) => {
                if !self.child_exit_seen.swap(true, Ordering::AcqRel) {
                    self.cleanup_resources();
                    let _ = self
                        .events
                        .send(TerminalEvent::ChildExit(status.code()));
                }
            }
            Event::Exit => {
                // `Term::exit` follows the PTY child notification. Preserve
                // an unknown exit status when a platform cannot provide one.
                if !self.child_exit_seen.swap(true, Ordering::AcqRel) {
                    self.cleanup_resources();
                    let _ = self.events.send(TerminalEvent::ChildExit(None));
                }
            }
            Event::PtyWrite(text) => {
                let sender = self
                    .input_sender
                    .lock()
                    .ok()
                    .and_then(|sender| sender.clone());
                if let Some(sender) = sender {
                    let _ =
                        sender.send(Msg::Input(Cow::Owned(text.into_bytes())));
                }
            }
            // OSC clipboard, title, bell, and hyperlink side effects are not
            // forwarded to the host application.
            Event::ClipboardStore(_, _)
            | Event::ClipboardLoad(_, _)
            | Event::ColorRequest(_, _)
            | Event::TextAreaSizeRequest(_)
            | Event::CursorBlinkingChange
            | Event::MouseCursorDirty
            | Event::ResetTitle
            | Event::Title(_)
            | Event::Bell => {}
        }
    }
}

struct TerminalDimensions {
    columns: usize,
    lines: usize,
}

impl Dimensions for TerminalDimensions {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// A PTY and parsed terminal model. It is `Send + Sync` through the fair
/// mutex, allowing the PTY event loop and GPUI entity to run independently.
pub struct TerminalBackend {
    sender: EventLoopSender,
    terminal: Arc<FairMutex<Term<TerminalProxy>>>,
    events: Arc<Mutex<Receiver<TerminalEvent>>>,
    events_sender: Sender<TerminalEvent>,
    child_exit_seen: Arc<AtomicBool>,
    _test_directory: Option<AgentTestDirectory>,
    shutdown_requested: AtomicBool,
    geometry: Mutex<TerminalGeometry>,
    viewport_generation: AtomicU64,
}

impl TerminalBackend {
    pub fn spawn(
        spec: &AgentLaunchSpec,
        test_directory: Option<AgentTestDirectory>,
        working_directory: &Path,
        window_id: u64,
    ) -> anyhow::Result<Self> {
        let executable = crate::agent::resolve_executable(&spec.executable)?;
        let (events_tx, events_rx) = mpsc::channel();
        let events_for_join = events_tx.clone();
        let input_sender = Arc::new(Mutex::new(None));
        let child_exit_seen = Arc::new(AtomicBool::new(false));
        let proxy = TerminalProxy {
            events: events_tx,
            test_directory: test_directory.clone(),
            input_sender: input_sender.clone(),
            child_exit_seen,
        };
        let dimensions = TerminalDimensions {
            columns: DEFAULT_COLUMNS,
            lines: DEFAULT_LINES,
        };
        let terminal = Arc::new(FairMutex::new(Term::new(
            TerminalConfig {
                scrolling_history: 10_000,
                osc52: alacritty_terminal::term::Osc52::Disabled,
                ..TerminalConfig::default()
            },
            &dimensions,
            proxy.clone(),
        )));

        let mut env = HashMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());

        let working_directory = normalize_working_directory(working_directory);
        let mut options = PtyOptions {
            shell: Some(Shell::new(
                terminal_program(executable.as_path()),
                spec.args.clone(),
            )),
            working_directory: Some(working_directory),
            drain_on_exit: true,
            env,
            ..PtyOptions::default()
        };
        #[cfg(target_os = "windows")]
        {
            options.escape_args = true;
        }
        let pty = tty::new(
            &options,
            WindowSize {
                num_lines: DEFAULT_LINES as u16,
                num_cols: DEFAULT_COLUMNS as u16,
                cell_width: CELL_WIDTH,
                cell_height: CELL_HEIGHT,
            },
            window_id,
        )?;
        let events_sender = proxy.events.clone();
        let child_exit_seen = proxy.child_exit_seen.clone();
        let event_loop =
            EventLoop::new(terminal.clone(), proxy, pty, true, false)?;
        let sender = event_loop.channel();
        if let Ok(mut input) = input_sender.lock() {
            *input = Some(sender.clone());
        }
        let event_loop_join = event_loop.spawn();
        let child_exit_for_join = child_exit_seen.clone();
        let test_directory_for_join = test_directory.clone();
        std::thread::spawn(move || {
            let _ = event_loop_join.join();
            if !child_exit_for_join.swap(true, Ordering::AcqRel) {
                if let Some(test_directory) = test_directory_for_join {
                    if test_directory.cleanup().is_err() {
                        log::debug!(
                            "[agent_terminal] temporary test directory cleanup deferred"
                        );
                    }
                }
                log::error!(
                    "[agent_terminal] PTY event loop stopped before child exit"
                );
                let _ = events_for_join.send(TerminalEvent::Error(
                    "PTY event loop stopped unexpectedly".to_string(),
                ));
            }
        });

        Ok(Self {
            sender,
            terminal,
            events: Arc::new(Mutex::new(events_rx)),
            events_sender,
            child_exit_seen,
            _test_directory: test_directory,
            shutdown_requested: AtomicBool::new(false),
            geometry: Mutex::new(TerminalGeometry::default()),
            viewport_generation: AtomicU64::new(0),
        })
    }

    pub fn send_bytes(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let _ = self.sender.send(Msg::Input(Cow::Owned(bytes)));
    }

    pub fn shutdown(&self) {
        if self
            .shutdown_requested
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        // Give an interactive CLI a chance to handle Ctrl-C (and terminate
        // its own descendants) before the PTY is dropped and force-closed.
        self.send_bytes(vec![0x03]);
        let sender = self.sender.clone();
        let events_sender = self.events_sender.clone();
        let child_exit_seen = self.child_exit_seen.clone();
        let test_directory = self._test_directory.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            if !child_exit_seen.swap(true, Ordering::AcqRel) {
                if let Some(test_directory) = test_directory {
                    if test_directory.cleanup().is_err() {
                        log::debug!(
                            "[agent_terminal] temporary test directory cleanup deferred"
                        );
                    }
                }
                let _ = events_sender.send(TerminalEvent::ChildExit(None));
            }
            let _ = sender.send(Msg::Shutdown);
        });
    }

    fn send_resize(&self, geometry: TerminalGeometry) {
        let _ = self
            .sender
            .send(Msg::Resize(window_size_for_geometry(geometry)));
    }

    pub fn scroll_lines(&self, lines: i32) {
        let mut terminal = self.terminal.lock();
        terminal.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
    }

    /// Start a simple host-side text selection when the Agent did not request
    /// mouse reporting. Coordinates are viewport-relative and clamped by the
    /// terminal state machine before being converted to grid coordinates.
    pub fn begin_selection(&self, position: Point<Pixels>) -> bool {
        let geometry = self.geometry();
        let mut terminal = self.terminal.lock();
        if terminal.mode().contains(TermMode::MOUSE_MODE) {
            return false;
        }
        let point = viewport_point(
            self.local_position(position),
            terminal.grid().display_offset(),
            geometry,
        );
        terminal.selection =
            Some(Selection::new(SelectionType::Simple, point, Side::Left));
        true
    }

    pub fn update_selection(&self, position: Point<Pixels>) -> bool {
        let geometry = self.geometry();
        let mut terminal = self.terminal.lock();
        if terminal.selection.is_none() {
            return false;
        }
        let point = viewport_point(
            self.local_position(position),
            terminal.grid().display_offset(),
            geometry,
        );
        if let Some(selection) = terminal.selection.as_mut() {
            selection.update(point, Side::Right);
        }
        true
    }

    pub fn finish_selection(&self) -> bool {
        let mut terminal = self.terminal.lock();
        let Some(selection) = terminal.selection.as_mut() else {
            return false;
        };
        selection.include_all();
        !selection.is_empty()
    }

    pub fn selected_text(&self) -> Option<String> {
        self.terminal.lock().selection_to_string()
    }

    /// Send host clipboard text using the terminal's bracketed-paste mode when
    /// requested by the child application.
    pub fn paste_text(&self, text: &str) {
        let bracketed = self
            .terminal
            .lock()
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        self.send_bytes(encode_paste(text, bracketed));
    }

    pub fn send_mouse_report(
        &self,
        button: u8,
        position: Point<Pixels>,
        pressed: bool,
        modifiers: Modifiers,
    ) -> bool {
        let position = self.local_position(position);
        let terminal = self.terminal.lock();
        let mode = *terminal.mode();
        let reports_clicks = mode.contains(TermMode::MOUSE_REPORT_CLICK);
        let reports_motion = mode.contains(TermMode::MOUSE_MOTION);
        let is_motion = button == 32;
        if is_motion && !reports_motion || !is_motion && !reports_clicks {
            return false;
        }
        let mut code = button;
        if modifiers.shift {
            code = code.saturating_add(4);
        }
        if modifiers.alt {
            code = code.saturating_add(8);
        }
        if modifiers.control {
            code = code.saturating_add(16);
        }
        let geometry = self.geometry();
        let column = (f32::from(position.x) / geometry.cell_width)
            .floor()
            .max(0.)
            .min(f32::from(geometry.columns.saturating_sub(1)))
            as u16
            + 1;
        let row = (f32::from(position.y) / geometry.line_height)
            .floor()
            .max(0.)
            .min(f32::from(geometry.lines.saturating_sub(1)))
            as u16
            + 1;
        let column = column.min(223);
        let row = row.min(223);
        let bytes = if mode.contains(TermMode::SGR_MOUSE) {
            format!(
                "\x1b[<{};{};{}{}",
                code,
                column,
                row,
                if pressed { 'M' } else { 'm' }
            )
            .into_bytes()
        } else {
            vec![
                0x1b,
                b'[',
                b'M',
                code.saturating_add(32),
                column as u8 + 32,
                row as u8 + 32,
            ]
        };
        drop(terminal);
        self.send_bytes(bytes);
        true
    }

    pub fn drain_events(&self) -> Vec<TerminalEvent> {
        let Ok(receiver) = self.events.lock() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        loop {
            match receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        events
    }

    fn geometry(&self) -> TerminalGeometry {
        self.geometry
            .lock()
            .map(|geometry| *geometry)
            .unwrap_or_default()
    }

    fn line_delta(&self, pixels: f32) -> f32 {
        self.geometry().line_delta(pixels)
    }

    fn local_position(&self, position: Point<Pixels>) -> Point<Pixels> {
        let geometry = self.geometry();
        let (x, y) = geometry
            .local_position(f32::from(position.x), f32::from(position.y));
        point(px(x), px(y))
    }

    pub(crate) fn synchronize_viewport(
        &self,
        geometry: TerminalGeometry,
    ) -> (u64, TerminalSnapshot) {
        let (previous, grid_changed, pty_changed) = self
            .geometry
            .lock()
            .map(|mut current| {
                let previous = *current;
                let grid_changed = previous.grid_size_changed(geometry);
                let pty_changed = previous.pty_size_changed(geometry);
                *current = geometry;
                (previous, grid_changed, pty_changed)
            })
            .unwrap_or((geometry, false, false));
        let generation = if pty_changed {
            self.viewport_generation.fetch_add(1, Ordering::AcqRel) + 1
        } else {
            self.viewport_generation.load(Ordering::Acquire)
        };

        let snapshot = {
            let mut terminal = self.terminal.lock();
            if grid_changed {
                terminal.resize(TerminalDimensions {
                    columns: geometry.columns as usize,
                    lines: geometry.lines as usize,
                });
            }
            TerminalSnapshot::from_renderable(
                terminal.renderable_content(),
                terminal.columns(),
                terminal.screen_lines(),
            )
        };

        if pty_changed {
            log::debug!(
                "[agent_terminal] viewport resize generation={} grid={}x{} -> {}x{} cell_width={:.2} line_height={:.2}",
                generation,
                previous.columns,
                previous.lines,
                geometry.columns,
                geometry.lines,
                geometry.cell_width,
                geometry.line_height,
            );
            self.send_resize(geometry);
        }

        (generation, snapshot)
    }

    /// Search the bounded terminal grid without retaining a separate
    /// transcript. This includes scrollback and the active alternate screen,
    /// while preserving the same character filtering used for rendering.
    pub fn contains_text(&self, needle: &str) -> bool {
        let terminal = self.terminal.lock();
        grid_contains_text(terminal.grid(), needle)
    }
}

fn window_size_for_geometry(geometry: TerminalGeometry) -> WindowSize {
    WindowSize {
        num_lines: geometry.lines.max(1),
        num_cols: geometry.columns.max(2),
        cell_width: geometry.cell_width.round().max(1.) as u16,
        cell_height: geometry.line_height.round().max(1.) as u16,
    }
}

impl Drop for TerminalBackend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn viewport_point(
    position: Point<Pixels>,
    display_offset: usize,
    geometry: TerminalGeometry,
) -> TerminalPoint {
    let column = (f32::from(position.x) / geometry.cell_width)
        .floor()
        .max(0.)
        .min(f32::from(geometry.columns.saturating_sub(1)))
        as usize;
    let line = (f32::from(position.y) / geometry.line_height)
        .floor()
        .max(0.)
        .min(f32::from(geometry.lines.saturating_sub(1)))
        as usize;
    alacritty_terminal::term::viewport_to_point(
        display_offset,
        TerminalPoint::new(line, Column(column)),
    )
}

/// Build the program field consumed by `alacritty_terminal`'s PTY launcher.
///
/// ConPTY receives one command-line string from the terminal backend. Quoting
/// a Windows executable path that contains whitespace keeps an absolute path
/// override a single program token while leaving all user arguments as
/// independent values handled by the backend's escaping routine.
fn terminal_program(path: &Path) -> String {
    let value = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        if value.contains(' ') || value.contains('\t') {
            return format!("\"{value}\"");
        }
    }
    value
}

/// Convert a Windows extended-length path to the form accepted by `cmd.exe`
/// and other shell shims used by CLI agents. `std::fs::canonicalize` returns
/// paths with a `\\?\\` prefix on Windows, but `cmd.exe` treats that spelling
/// as an unsupported UNC working directory and silently falls back to
/// `C:\\Windows`. Keep non-verbatim and non-drive extended paths unchanged.
pub(crate) fn normalize_working_directory(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
            return std::path::PathBuf::from(format!("\\\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix("\\\\?\\") {
            let is_drive_path = rest.as_bytes().get(1) == Some(&b':');
            if is_drive_path {
                return std::path::PathBuf::from(rest);
            }
        }
    }
    path.to_path_buf()
}

fn grid_contains_text(grid: &Grid<Cell>, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut current_line = None;
    let mut row = String::new();
    let mut all_text = String::new();
    for indexed in
        grid.iter_from(TerminalPoint::new(grid.topmost_line(), Column(0)))
    {
        let line = indexed.point.line.0;
        if current_line != Some(line) {
            if current_line.is_some() {
                if row.contains(needle) {
                    return true;
                }
                all_text.push_str(&row);
            }
            row.clear();
            current_line = Some(line);
        }
        let cell = indexed.cell;
        if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            continue;
        }
        if cell.flags.contains(CellFlags::HIDDEN) {
            row.push(' ');
        } else {
            row.push(cell.c);
            if let Some(zerowidth) = cell.zerowidth() {
                row.extend(zerowidth.iter().copied());
            }
        }
    }
    if row.contains(needle) {
        return true;
    }
    all_text.push_str(&row);
    all_text.contains(needle)
}

fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if text.is_empty() {
        return Vec::new();
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut bytes = Vec::with_capacity(normalized.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(normalized.as_bytes());
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

fn encode_key(event: &KeyDownEvent) -> Option<Vec<u8>> {
    let key = event.keystroke.key.as_str();
    let modifiers = event.keystroke.modifiers;
    if modifiers.platform && !modifiers.control {
        return None;
    }
    if modifiers.control {
        if let Some(byte) = key.bytes().next().filter(|byte| byte.is_ascii()) {
            let byte = byte.to_ascii_lowercase();
            if (b'a'..=b'z').contains(&byte) {
                return Some(vec![byte - b'a' + 1]);
            }
            return match byte {
                b'[' => Some(vec![0x1b]),
                b'\\' => Some(vec![0x1c]),
                b']' => Some(vec![0x1d]),
                b'^' => Some(vec![0x1e]),
                b'_' => Some(vec![0x1f]),
                _ => None,
            };
        }
    }
    let special = match key {
        "enter" | "numpadenter" => "\r",
        "backspace" => "\x7f",
        "tab" => "\t",
        "escape" => "\x1b",
        "up" => "\x1b[A",
        "down" => "\x1b[B",
        "right" => "\x1b[C",
        "left" => "\x1b[D",
        "home" => "\x1b[H",
        "end" => "\x1b[F",
        "pageup" => "\x1b[5~",
        "pagedown" => "\x1b[6~",
        "insert" => "\x1b[2~",
        "delete" => "\x1b[3~",
        "f1" => "\x1bOP",
        "f2" => "\x1bOQ",
        "f3" => "\x1bOR",
        "f4" => "\x1bOS",
        "f5" => "\x1b[15~",
        "f6" => "\x1b[17~",
        "f7" => "\x1b[18~",
        "f8" => "\x1b[19~",
        "f9" => "\x1b[20~",
        "f10" => "\x1b[21~",
        "f11" => "\x1b[23~",
        "f12" => "\x1b[24~",
        _ => "",
    };
    if !special.is_empty() {
        return Some(special.as_bytes().to_vec());
    }
    let character = event
        .keystroke
        .key_char
        .as_deref()
        .filter(|text| !text.is_empty())
        .or_else(|| (key.chars().count() == 1).then_some(key));
    let character = character?;
    let mut bytes = Vec::new();
    if modifiers.alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(character.as_bytes());
    Some(bytes)
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::path::{Path, PathBuf};

    use super::geometry::TerminalGeometry;
    use super::model::TerminalSnapshot;
    use super::render::xterm_rgb;
    use super::{
        Cell, CellFlags, TerminalConfig, TerminalDimensions, encode_key,
        encode_paste, grid_contains_text, viewport_point,
        window_size_for_geometry,
    };
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Line, Point as TerminalPoint};
    use alacritty_terminal::term::Term;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
    use gpui::{KeyDownEvent, Keystroke, Modifiers, point, px};

    fn key(
        key: &str,
        character: Option<&str>,
        modifiers: Modifiers,
    ) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke {
                key: key.to_string(),
                key_char: character.map(str::to_string),
                modifiers,
            },
            is_held: false,
            prefer_character_input: true,
        }
    }

    #[test]
    fn encodes_text_and_control_keys() {
        assert_eq!(
            encode_key(&key("a", Some("a"), Modifiers::none())),
            Some(b"a".to_vec())
        );
        let modifiers = Modifiers {
            control: true,
            ..Modifiers::none()
        };
        assert_eq!(encode_key(&key("c", Some("c"), modifiers)), Some(vec![3]));
        assert_eq!(
            encode_key(&key("enter", None, Modifiers::none())),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_key(&key("up", None, Modifiers::none())),
            Some(b"\x1b[A".to_vec())
        );
    }

    #[test]
    fn encodes_bracketed_paste_without_shell_interpretation() {
        assert_eq!(encode_paste("a\r\nb\r", false), b"a\nb\n".to_vec());
        assert_eq!(
            encode_paste("a\r\nb", true),
            b"\x1b[200~a\nb\x1b[201~".to_vec()
        );
        assert!(encode_paste("", true).is_empty());
    }

    #[test]
    fn viewport_points_share_the_grid_and_clamp_edges() {
        let geometry =
            TerminalGeometry::from_bounds(10., 20., 80., 40., 8., 20., 1.);
        let point = viewport_point(point(px(999.), px(999.)), 0, geometry);
        assert_eq!(
            point,
            TerminalPoint::new(Line(1), alacritty_terminal::index::Column(9))
        );
    }

    #[test]
    fn pty_resize_request_matches_the_measured_geometry() {
        let geometry =
            TerminalGeometry::from_bounds(0., 0., 640., 320., 8.5, 19.5, 1.);
        let size = window_size_for_geometry(geometry);
        assert_eq!(size.num_cols, geometry.columns);
        assert_eq!(size.num_lines, geometry.lines);
        assert_eq!(size.cell_width, 9);
        assert_eq!(size.cell_height, 20);
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_verbatim_working_directories_for_cmd_shims() {
        assert_eq!(
            super::normalize_working_directory(Path::new(
                r"\\?\C:\Users\example\repo",
            )),
            PathBuf::from(r"C:\Users\example\repo")
        );
        assert_eq!(
            super::normalize_working_directory(Path::new(
                r"\\?\UNC\server\share\repo",
            )),
            PathBuf::from(r"\\server\share\repo")
        );
    }

    #[test]
    fn maps_xterm_indexed_colors() {
        assert_eq!(xterm_rgb(0), (0, 0, 0));
        assert_eq!(xterm_rgb(15), (255, 255, 255));
        assert_eq!(xterm_rgb(16), (0, 0, 0));
        assert_eq!(xterm_rgb(21), (0, 0, 255));
        assert_eq!(xterm_rgb(231), (255, 255, 255));
        assert_eq!(xterm_rgb(232), (8, 8, 8));
        assert_eq!(xterm_rgb(255), (238, 238, 238));
    }

    #[test]
    fn marker_search_handles_ansi_unicode_scrollback_and_alternate_screen() {
        let dimensions = TerminalDimensions {
            columns: 40,
            lines: 3,
        };
        let mut terminal = Term::new(
            TerminalConfig {
                scrolling_history: 32,
                ..TerminalConfig::default()
            },
            &dimensions,
            VoidListener,
        );
        let mut processor = Processor::<StdSyncHandler>::new();
        let unicode = "中文";
        let mut output = b"\x1b[31mold output\x1b[0m\r\n".to_vec();
        output.extend_from_slice(unicode.as_bytes());
        output.extend_from_slice(b"\r\nscrollback marker");
        processor.advance(&mut terminal, &output);
        assert!(grid_contains_text(&terminal.grid(), "scrollback marker"));
        assert!(grid_contains_text(&terminal.grid(), unicode));

        processor.advance(&mut terminal, b"\x1b[?1049h\x1b[2Jalternate marker");
        assert!(grid_contains_text(&terminal.grid(), "alternate marker"));
    }

    #[test]
    fn resizing_parser_grid_tracks_viewport_and_reflows_primary_screen() {
        let initial = TerminalDimensions {
            columns: 120,
            lines: 32,
        };
        let mut terminal = Term::new(
            TerminalConfig {
                scrolling_history: 64,
                ..TerminalConfig::default()
            },
            &initial,
            VoidListener,
        );
        let mut processor = Processor::<StdSyncHandler>::new();
        let mut output = b"prefix-".to_vec();
        output.extend(std::iter::repeat_n(b'x', 140));
        output.extend_from_slice(b"-suffix");
        processor.advance(&mut terminal, &output);

        terminal.resize(TerminalDimensions {
            columns: 60,
            lines: 16,
        });
        let compact = TerminalSnapshot::from_renderable(
            terminal.renderable_content(),
            terminal.columns(),
            terminal.screen_lines(),
        );
        assert_eq!((terminal.columns(), terminal.screen_lines()), (60, 16));
        assert_eq!((compact.columns, compact.screen_lines), (60, 16));
        assert!(compact.cells.iter().all(|cell| {
            cell.column < compact.columns
                && cell.line >= -(compact.screen_lines as i32)
                && cell.line < compact.screen_lines as i32
        }));
        assert!(all_grid_text(&terminal.grid()).contains("prefix-"));
        assert!(all_grid_text(&terminal.grid()).contains("-suffix"));

        terminal.resize(TerminalDimensions {
            columns: 160,
            lines: 40,
        });
        let expanded = TerminalSnapshot::from_renderable(
            terminal.renderable_content(),
            terminal.columns(),
            terminal.screen_lines(),
        );
        assert_eq!((expanded.columns, expanded.screen_lines), (160, 40));
        assert!(expanded.cells.iter().all(|cell| cell.column < 160));
        assert!(all_grid_text(&terminal.grid()).contains("prefix-"));
        assert!(all_grid_text(&terminal.grid()).contains("-suffix"));
    }

    #[test]
    fn resizing_alternate_screen_keeps_cells_inside_new_grid() {
        let initial = TerminalDimensions {
            columns: 120,
            lines: 32,
        };
        let mut terminal =
            Term::new(TerminalConfig::default(), &initial, VoidListener);
        let mut processor = Processor::<StdSyncHandler>::new();
        processor.advance(
            &mut terminal,
            b"\x1b[?1049h\x1b[2J\x1b[Halternate-screen",
        );

        terminal.resize(TerminalDimensions {
            columns: 48,
            lines: 12,
        });
        let snapshot = TerminalSnapshot::from_renderable(
            terminal.renderable_content(),
            terminal.columns(),
            terminal.screen_lines(),
        );
        assert!(snapshot.alternate_screen);
        assert_eq!((snapshot.columns, snapshot.screen_lines), (48, 12));
        assert!(snapshot.cells.iter().all(|cell| {
            cell.column < snapshot.columns
                && cell.line >= 0
                && cell.line < snapshot.screen_lines as i32
        }));
        assert!(all_grid_text(&terminal.grid()).contains("alternate-screen"));
    }

    fn all_grid_text(grid: &alacritty_terminal::grid::Grid<Cell>) -> String {
        let mut text = String::new();
        for line in grid.topmost_line().0..=grid.bottommost_line().0 {
            for column in 0..grid.columns() {
                let cell = &grid[Line(line)]
                    [alacritty_terminal::index::Column(column)];
                if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    continue;
                }
                if cell.flags.contains(CellFlags::HIDDEN) {
                    text.push(' ');
                } else {
                    text.push(cell.c);
                    if let Some(zerowidth) = cell.zerowidth() {
                        text.extend(zerowidth.iter().copied());
                    }
                }
            }
        }
        text
    }
}
