//! Embedded PTY-backed terminal used for visible external Agent diagnostics.
//!
//! The terminal owns no shell policy and does not interpret Agent output. It
//! provides a small GPUI view over `alacritty_terminal`'s cross-platform PTY
//! and ANSI state machine.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Point as TerminalPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config as TerminalConfig, Term, TermMode};
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, Rgb};
use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, v_flex};

use crate::agent::{AgentLaunchSpec, AgentTestDirectory};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalColor {
    Named(u16),
    Indexed(u8),
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Clone, Debug)]
struct TerminalCellSnapshot {
    character: char,
    zero_width: Vec<char>,
    foreground: TerminalColor,
    background: TerminalColor,
    flags: u16,
}

#[derive(Clone, Debug, Default)]
pub struct TerminalSnapshot {
    pub rows: Vec<String>,
    pub cursor: Option<(usize, usize)>,
    pub alternate_screen: bool,
    pub display_offset: usize,
    /// Visible row ranges covered by the current host-side selection.
    pub selection: Option<SelectionRange>,
    cell_rows: Vec<Vec<TerminalCellSnapshot>>,
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

        let mut options = PtyOptions {
            shell: Some(Shell::new(
                terminal_program(executable.as_path()),
                spec.args.clone(),
            )),
            working_directory: Some(working_directory.to_path_buf()),
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

    pub fn resize(
        &self,
        columns: u16,
        lines: u16,
        cell_width: u16,
        cell_height: u16,
    ) {
        let _ = self.sender.send(Msg::Resize(WindowSize {
            num_lines: lines.max(1),
            num_cols: columns.max(2),
            cell_width: cell_width.max(1),
            cell_height: cell_height.max(1),
        }));
    }

    pub fn scroll_lines(&self, lines: i32) {
        let mut terminal = self.terminal.lock();
        terminal.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
    }

    /// Start a simple host-side text selection when the Agent did not request
    /// mouse reporting. Coordinates are viewport-relative and clamped by the
    /// terminal state machine before being converted to grid coordinates.
    pub fn begin_selection(&self, position: Point<Pixels>) -> bool {
        let mut terminal = self.terminal.lock();
        if terminal.mode().contains(TermMode::MOUSE_MODE) {
            return false;
        }
        let point = viewport_point(position, terminal.grid().display_offset());
        terminal.selection =
            Some(Selection::new(SelectionType::Simple, point, Side::Left));
        true
    }

    pub fn update_selection(&self, position: Point<Pixels>) -> bool {
        let mut terminal = self.terminal.lock();
        if terminal.selection.is_none() {
            return false;
        }
        let point = viewport_point(position, terminal.grid().display_offset());
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
        let column = (f32::from(position.x) / f32::from(CELL_WIDTH))
            .floor()
            .max(0.) as u16
            + 1;
        let row = (f32::from(position.y) / f32::from(CELL_HEIGHT))
            .floor()
            .max(0.) as u16
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

    pub fn snapshot(&self) -> TerminalSnapshot {
        let terminal = self.terminal.lock();
        let content = terminal.renderable_content();
        let mut rows = Vec::<String>::new();
        let mut cell_rows = Vec::<Vec<TerminalCellSnapshot>>::new();
        let mut current_line = None;
        for indexed in content.display_iter {
            let line = indexed.point.line.0;
            if current_line != Some(line) {
                rows.push(String::new());
                cell_rows.push(Vec::new());
                current_line = Some(line);
            }
            let Some(row) = rows.last_mut() else {
                continue;
            };
            let Some(cells) = cell_rows.last_mut() else {
                continue;
            };
            let cell = indexed.cell;
            if cell.flags.intersects(
                alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER,
            ) {
                continue;
            }
            if cell
                .flags
                .contains(alacritty_terminal::term::cell::Flags::HIDDEN)
            {
                row.push(' ');
            } else {
                row.push(cell.c);
                if let Some(zerowidth) = cell.zerowidth() {
                    row.extend(zerowidth.iter().copied());
                }
            }
            cells.push(TerminalCellSnapshot {
                character: if cell.flags.contains(CellFlags::HIDDEN) {
                    ' '
                } else {
                    cell.c
                },
                zero_width: cell.zerowidth().unwrap_or_default().to_vec(),
                foreground: terminal_color(cell.fg),
                background: terminal_color(cell.bg),
                flags: cell.flags.bits(),
            });
        }
        let cursor = match content.cursor.shape {
            alacritty_terminal::vte::ansi::CursorShape::Hidden => None,
            _ => Some((
                (content.cursor.point.line.0 + content.display_offset as i32)
                    .max(0) as usize,
                content.cursor.point.column.0,
            )),
        };
        TerminalSnapshot {
            rows,
            cursor,
            alternate_screen: content.mode.contains(TermMode::ALT_SCREEN),
            display_offset: content.display_offset,
            selection: content.selection,
            cell_rows,
        }
    }

    /// Search the bounded terminal grid without retaining a separate
    /// transcript. This includes scrollback and the active alternate screen,
    /// while preserving the same character filtering used for rendering.
    pub fn contains_text(&self, needle: &str) -> bool {
        let terminal = self.terminal.lock();
        grid_contains_text(terminal.grid(), needle)
    }
}

impl Drop for TerminalBackend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// GPUI rendering and input bridge for one visible Agent test.
pub struct TerminalView {
    backend: Arc<TerminalBackend>,
    rows: Vec<String>,
    cell_rows: Vec<Vec<TerminalCellSnapshot>>,
    cursor: Option<(usize, usize)>,
    alternate_screen: bool,
    display_offset: usize,
    selection: Option<SelectionRange>,
    terminal_size: (u16, u16),
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
                let snapshot = backend_for_task.snapshot();
                let done = events.iter().any(|event| {
                    matches!(
                        event,
                        TerminalEvent::ChildExit(_) | TerminalEvent::Error(_)
                    )
                });
                view_entity.update(cx, |view, cx| {
                    view.apply(snapshot, events, cx);
                });
                if done {
                    break;
                }
            }
        });
        Self {
            backend,
            rows: Vec::new(),
            cell_rows: Vec::new(),
            cursor: None,
            alternate_screen: false,
            display_offset: 0,
            selection: None,
            terminal_size: (DEFAULT_COLUMNS as u16, DEFAULT_LINES as u16),
            focus_handle,
            _poll_task: Some(poll_task),
            child_exit: None,
            error: None,
        }
    }

    fn apply(
        &mut self,
        snapshot: TerminalSnapshot,
        events: Vec<TerminalEvent>,
        cx: &mut Context<Self>,
    ) {
        self.rows = snapshot.rows;
        self.cell_rows = snapshot.cell_rows;
        self.cursor = snapshot.cursor;
        self.alternate_screen = snapshot.alternate_screen;
        self.display_offset = snapshot.display_offset;
        self.selection = snapshot.selection;
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

    fn render_rows(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let mono = cx.theme().mono_font_family.clone();
        let rows = self.rows.iter().enumerate().map(|(index, row)| {
            let selected = self.selection.is_some_and(|selection| {
                let start = selection.start.line.0 + self.display_offset as i32;
                let end = selection.end.line.0 + self.display_offset as i32;
                let (start, end) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                (start..=end).contains(&(index as i32))
            });
            let cells = self.cell_rows.get(index).map(Vec::as_slice);
            let (text, highlights) = styled_row(cells, row, &colors);
            let highlights = if selected {
                highlights
                    .into_iter()
                    .map(|(range, mut style)| {
                        style.background_color = None;
                        (range, style)
                    })
                    .collect()
            } else {
                highlights
            };
            let mut text = if text.is_empty() { row.clone() } else { text };
            if self.cursor.is_some_and(|(line, _)| line == index) {
                text.push(' ');
            }
            let mut line = div()
                .id(SharedString::from(format!("agent-terminal-line-{index}")))
                .w_full()
                .h(px(CELL_HEIGHT as f32))
                .flex_shrink_0()
                .relative()
                .font_family(mono.clone())
                .text_size(px(13.))
                .text_color(colors.foreground)
                .whitespace_nowrap()
                .when(selected, |element| element.bg(colors.selection))
                .child(
                    StyledText::new(SharedString::from(text))
                        .with_highlights(highlights),
                );
            if let Some((_, column)) =
                self.cursor.filter(|(line, _)| *line == index)
            {
                line = line.child(
                    div()
                        .absolute()
                        .left(px(column as f32 * CELL_WIDTH as f32))
                        .top_0()
                        .w(px(CELL_WIDTH as f32))
                        .h(px(CELL_HEIGHT as f32))
                        .bg(colors.foreground.opacity(0.35)),
                );
            }
            line
        });
        v_flex()
            .w_full()
            .min_h_0()
            .font_family(mono.clone())
            .children(rows)
    }
}

impl Render for TerminalView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.resize_to_window(window);
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let focus = self.focus_handle.clone();
        let backend = self.backend.clone();
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
                            f32::from(point.y) / CELL_HEIGHT as f32
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
                    .child(self.render_rows(cx)),
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

impl TerminalView {
    fn resize_to_window(&mut self, window: &Window) {
        let viewport = window.viewport_size();
        let columns = (f32::from(viewport.width) / f32::from(CELL_WIDTH))
            .floor()
            .max(2.) as u16;
        let lines = (f32::from(viewport.height) / f32::from(CELL_HEIGHT))
            .floor()
            .max(1.) as u16;
        if self.terminal_size == (columns, lines) {
            return;
        }
        self.terminal_size = (columns, lines);
        self.backend.resize(columns, lines, CELL_WIDTH, CELL_HEIGHT);
    }
}

fn viewport_point(
    position: Point<Pixels>,
    display_offset: usize,
) -> TerminalPoint {
    let column = (f32::from(position.x) / f32::from(CELL_WIDTH))
        .floor()
        .max(0.) as usize;
    let line = (f32::from(position.y) / f32::from(CELL_HEIGHT))
        .floor()
        .max(0.) as usize;
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

fn terminal_color(color: AnsiColor) -> TerminalColor {
    match color {
        AnsiColor::Named(named) => TerminalColor::Named(named as u16),
        AnsiColor::Indexed(index) => TerminalColor::Indexed(index),
        AnsiColor::Spec(Rgb { r, g, b }) => TerminalColor::Rgb { r, g, b },
    }
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

fn styled_row(
    cells: Option<&[TerminalCellSnapshot]>,
    fallback: &str,
    colors: &gpui_component::theme::ThemeColor,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let Some(cells) = cells else {
        return (String::new(), Vec::new());
    };

    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut current: Option<(usize, HighlightStyle)> = None;
    for cell in cells {
        let start = text.len();
        text.push(cell.character);
        text.extend(cell.zero_width.iter().copied());
        let style = terminal_cell_style(cell, colors);
        if let Some((run_start, run_style)) = current.take() {
            if run_style == style {
                current = Some((run_start, run_style));
            } else {
                highlights.push((run_start..start, run_style));
                current = Some((start, style));
            }
        } else {
            current = Some((start, style));
        }
    }
    if let Some((run_start, run_style)) = current {
        if run_start < text.len() {
            highlights.push((run_start..text.len(), run_style));
        }
    }
    if text.is_empty() {
        (fallback.to_string(), Vec::new())
    } else {
        (text, highlights)
    }
}

fn terminal_cell_style(
    cell: &TerminalCellSnapshot,
    colors: &gpui_component::theme::ThemeColor,
) -> HighlightStyle {
    let flags = CellFlags::from_bits_retain(cell.flags);
    let mut foreground = terminal_color_to_hsla(cell.foreground, colors);
    let mut background = terminal_color_to_hsla(cell.background, colors);
    if flags.contains(CellFlags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }
    HighlightStyle {
        color: Some(foreground),
        background_color: Some(background),
        font_weight: flags
            .contains(CellFlags::BOLD)
            .then_some(FontWeight::BOLD),
        font_style: flags
            .contains(CellFlags::ITALIC)
            .then_some(FontStyle::Italic),
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
        fade_out: flags.contains(CellFlags::DIM).then_some(0.4),
        ..HighlightStyle::default()
    }
}

fn terminal_color_to_hsla(
    color: TerminalColor,
    colors: &gpui_component::theme::ThemeColor,
) -> Hsla {
    match color {
        TerminalColor::Rgb { r, g, b } => rgb_color(r, g, b),
        TerminalColor::Indexed(index) => {
            let (r, g, b) = xterm_rgb(index);
            rgb_color(r, g, b)
        }
        TerminalColor::Named(index) => match index {
            256 | 267 | 268 => colors.foreground,
            257 => colors.background,
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

fn rgb_color(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: 1.,
    }
    .into()
}

fn xterm_rgb(index: u8) -> (u8, u8, u8) {
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
    use super::{
        TerminalConfig, TerminalDimensions, encode_key, encode_paste,
        grid_contains_text, xterm_rgb,
    };
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Term;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
    use gpui::{KeyDownEvent, Keystroke, Modifiers};

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
}
