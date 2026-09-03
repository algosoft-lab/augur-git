//! Application diagnostics logging, routing records into functional files.

use std::backtrace::Backtrace;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use env_logger::{Builder, Env, Logger, Target, WriteStyle};
use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogCategory {
    App,
    Git,
    Agent,
    Extension,
    Terminal,
    System,
}

impl LogCategory {
    const ALL: [Self; 6] = [
        Self::App,
        Self::Git,
        Self::Agent,
        Self::Extension,
        Self::Terminal,
        Self::System,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Git => "git",
            Self::Agent => "agent",
            Self::Extension => "extension",
            Self::Terminal => "terminal",
            Self::System => "system",
        }
    }
}

struct LogRouter {
    summary: Logger,
    app: Logger,
    git: Logger,
    agent: Logger,
    extension: Logger,
    terminal: Logger,
    system: Logger,
}

impl LogRouter {
    fn new(root: Option<PathBuf>) -> Self {
        let root = root.as_deref();
        Self {
            summary: build_logger(log_path(root, None)),
            app: build_logger(log_path(root, Some(LogCategory::App))),
            git: build_logger(log_path(root, Some(LogCategory::Git))),
            agent: build_logger(log_path(root, Some(LogCategory::Agent))),
            extension: build_logger(log_path(
                root,
                Some(LogCategory::Extension),
            )),
            terminal: build_logger(log_path(root, Some(LogCategory::Terminal))),
            system: build_logger(log_path(root, Some(LogCategory::System))),
        }
    }

    fn logger(&self, category: LogCategory) -> &Logger {
        match category {
            LogCategory::App => &self.app,
            LogCategory::Git => &self.git,
            LogCategory::Agent => &self.agent,
            LogCategory::Extension => &self.extension,
            LogCategory::Terminal => &self.terminal,
            LogCategory::System => &self.system,
        }
    }

    fn max_level(&self) -> LevelFilter {
        LogCategory::ALL
            .into_iter()
            .map(|category| self.logger(category).filter())
            .chain(std::iter::once(self.summary.filter()))
            .max()
            .unwrap_or(LevelFilter::Off)
    }
}

impl Log for LogRouter {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let category = category_for_target(metadata.target());
        self.logger(category).enabled(metadata)
            || (is_summary_metadata(metadata) && self.summary.enabled(metadata))
    }

    fn log(&self, record: &Record<'_>) {
        self.logger(category_for_record(record)).log(record);
        if is_summary_record(record) {
            self.summary.log(record);
        }
    }

    fn flush(&self) {
        self.summary.flush();
        for category in LogCategory::ALL {
            self.logger(category).flush();
        }
    }
}

/// Install file-only diagnostic logging and the panic hook.
pub(crate) fn init() {
    let router = LogRouter::new(log_root());
    let max_level = router.max_level();
    if log::set_boxed_logger(Box::new(router)).is_ok() {
        log::set_max_level(max_level);
    }
    install_panic_hook();
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let backtrace = Backtrace::force_capture();
        log::error!("[panic] {panic_info}\n{backtrace}");
    }));
}

fn category_for_record(record: &Record<'_>) -> LogCategory {
    let message = record.args().to_string();
    category_from_message(&message)
        .unwrap_or_else(|| category_for_target(record.target()))
}

fn category_from_message(message: &str) -> Option<LogCategory> {
    if message.starts_with("[agent") {
        Some(LogCategory::Agent)
    } else if message.starts_with("[git")
        || message.starts_with("[branch_ops]")
        || message.starts_with("[commit_")
        || message.starts_with("[graph_perf]")
    {
        Some(LogCategory::Git)
    } else if message.starts_with("[extension")
        || message.starts_with("[extensions]")
    {
        Some(LogCategory::Extension)
    } else if message.starts_with("[terminal") {
        Some(LogCategory::Terminal)
    } else if message.starts_with("[app")
        || message.starts_with("[workspace")
        || message.starts_with("[settings]")
        || message.starts_with("[theme]")
        || message.starts_with("[config]")
        || message.starts_with("[ui_state]")
    {
        Some(LogCategory::App)
    } else {
        None
    }
}

fn category_for_target(target: &str) -> LogCategory {
    if !is_application_target(target) {
        return LogCategory::System;
    }

    if target == "augur_git"
        || target.starts_with("augur_git::workspace")
            && !target.starts_with("augur_git::workspace::agent_")
            && !target.starts_with("augur_git::workspace::extension")
            && !target.starts_with("augur_git::workspace::repo_tab")
            && !target.starts_with("augur_git::workspace::settings::agents")
    {
        return LogCategory::App;
    }
    if target.starts_with("augur_git::agent")
        || target.starts_with("augur_git::workspace::agent_")
        || target.starts_with("augur_git::workspace::settings::agents")
    {
        return LogCategory::Agent;
    }
    if target.starts_with("augur_git::extension")
        || target.starts_with("augur_git::workspace::extension")
    {
        return LogCategory::Extension;
    }
    if target.starts_with("augur_git::terminal") {
        return LogCategory::Terminal;
    }
    if target.starts_with("augur_git::git")
        || target.starts_with("augur_git::core::git")
        || target.starts_with("augur_git::workspace::repo_tab")
    {
        return LogCategory::Git;
    }

    LogCategory::App
}

fn is_application_target(target: &str) -> bool {
    target == "augur_git" || target.starts_with("augur_git::")
}

fn is_summary_metadata(metadata: &Metadata<'_>) -> bool {
    is_application_target(metadata.target())
        && matches!(metadata.level(), Level::Error | Level::Warn)
}

fn is_summary_record(record: &Record<'_>) -> bool {
    is_summary_metadata(record.metadata())
}

fn build_logger(path: Option<PathBuf>) -> Logger {
    let mut builder = Builder::from_env(
        Env::default().default_filter_or(default_log_filter()),
    );
    builder
        .target(Target::Pipe(Box::new(RotatingFileWriter::new(path))))
        .write_style(WriteStyle::Never);
    builder.build()
}

fn default_log_filter() -> &'static str {
    #[cfg(debug_assertions)]
    {
        "warn"
    }
    #[cfg(not(debug_assertions))]
    {
        "info"
    }
}

#[cfg(debug_assertions)]
const LOG_PREFIX: &str = "debug";

#[cfg(not(debug_assertions))]
const LOG_PREFIX: &str = "augur-git";

fn log_root() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    {
        Some(PathBuf::from("."))
    }
    #[cfg(not(debug_assertions))]
    {
        let root = dirs::data_local_dir()?.join("augur-git").join("logs");
        fs::create_dir_all(&root).ok()?;
        Some(root)
    }
}

fn log_path(
    root: Option<&Path>,
    category: Option<LogCategory>,
) -> Option<PathBuf> {
    let root = root?;
    let file_name = match category {
        Some(category) => format!("{LOG_PREFIX}-{}.log", category.slug()),
        None => format!("{LOG_PREFIX}.log"),
    };
    Some(root.join(file_name))
}

fn previous_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("debug.log");
    let previous_name = file_name
        .strip_suffix(".log")
        .map(|stem| format!("{stem}.previous.log"))
        .unwrap_or_else(|| format!("{file_name}.previous.log"));
    path.parent()
        .map(|parent| parent.join(&previous_name))
        .unwrap_or_else(|| PathBuf::from(previous_name))
}

/// Move the previous current file aside without making logging a startup dependency.
fn rotate_existing(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }
    let previous = previous_path(path);
    if previous.exists() && fs::remove_file(&previous).is_err() {
        return false;
    }
    fs::rename(path, previous).is_ok()
}

struct RotatingFileWriter {
    path: Option<PathBuf>,
    file: Option<File>,
    bytes: u64,
}

impl RotatingFileWriter {
    fn new(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self {
                path: None,
                file: None,
                bytes: 0,
            };
        };

        let starts_fresh = rotate_existing(&path);
        let (file, bytes) = open_log_file(&path, starts_fresh);
        let mut writer = Self {
            path: Some(path),
            file,
            bytes,
        };
        let header = format!(
            "--- session started: {} ---\n",
            Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
        );
        let _ = writer.write_all(header.as_bytes());
        writer
    }

    fn rotate_for_limit(&mut self) {
        let Some(path) = self.path.as_deref() else {
            return;
        };
        self.file.take();
        let starts_fresh = rotate_existing(path);
        let (file, bytes) = open_log_file(path, starts_fresh);
        self.file = file;
        self.bytes = bytes;
    }
}

impl Write for RotatingFileWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.file.is_none() {
            return Ok(buffer.len());
        }
        if self.bytes.saturating_add(buffer.len() as u64) > MAX_LOG_FILE_BYTES {
            self.rotate_for_limit();
        }
        let Some(file) = &mut self.file else {
            return Ok(buffer.len());
        };
        match file.write(buffer) {
            Ok(written) => {
                self.bytes = self.bytes.saturating_add(written as u64);
                Ok(written)
            }
            Err(error) => {
                self.file = None;
                Err(error)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = &mut self.file {
            file.flush()
        } else {
            Ok(())
        }
    }
}

fn open_log_file(path: &Path, truncate: bool) -> (Option<File>, u64) {
    let mut options = OpenOptions::new();
    options.create(true);
    if truncate {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    let Ok(file) = options.open(path) else {
        return (None, 0);
    };
    let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    (Some(file), bytes)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[test]
    fn routes_application_targets_by_function() {
        assert_eq!(category_for_target("augur_git"), LogCategory::App);
        assert_eq!(
            category_for_target("augur_git::workspace::settings::agents_view"),
            LogCategory::Agent
        );
        assert_eq!(
            category_for_target("augur_git::workspace::extension_runtime"),
            LogCategory::Extension
        );
        assert_eq!(
            category_for_target("augur_git::workspace::repo_tab"),
            LogCategory::Git
        );
        assert_eq!(
            category_for_target("augur_git::terminal::view"),
            LogCategory::Terminal
        );
        assert_eq!(
            category_for_target("augur_git::core::config"),
            LogCategory::App
        );
        assert_eq!(
            category_for_target("gpui_windows::window"),
            LogCategory::System
        );
    }

    #[test]
    fn known_message_prefixes_override_shared_modules() {
        assert_eq!(
            category_from_message("[agent_settings] add requested"),
            Some(LogCategory::Agent)
        );
        assert_eq!(
            category_from_message("[git_view] repository opened"),
            Some(LogCategory::Git)
        );
        assert_eq!(
            category_from_message("[extension_log] append failed"),
            Some(LogCategory::Extension)
        );
        assert_eq!(
            category_from_message("[workspace_tabs] tab activated"),
            Some(LogCategory::App)
        );
        assert_eq!(category_from_message("unclassified message"), None);
    }

    #[test]
    fn summary_only_contains_application_warnings_and_errors() {
        let app_error = Record::builder()
            .args(format_args!("error"))
            .level(Level::Error)
            .target("augur_git::workspace")
            .build();
        let app_info = Record::builder()
            .args(format_args!("info"))
            .level(Level::Info)
            .target("augur_git::workspace")
            .build();
        let system_error = Record::builder()
            .args(format_args!("error"))
            .level(Level::Error)
            .target("gpui::window")
            .build();

        assert!(is_summary_record(&app_error));
        assert!(!is_summary_record(&app_info));
        assert!(!is_summary_record(&system_error));
    }

    #[test]
    fn previous_paths_preserve_the_log_extension() {
        assert_eq!(
            previous_path(Path::new("debug.log")),
            PathBuf::from("debug.previous.log")
        );
        assert_eq!(
            previous_path(Path::new("logs/debug-agent.log")),
            PathBuf::from("logs/debug-agent.previous.log")
        );
    }

    #[test]
    fn startup_rotates_the_current_file() {
        let root = temporary_root("startup");
        fs::create_dir_all(&root).expect("temporary log root");
        let path = root.join("debug.log");
        fs::write(&path, "old session\n").expect("old log");

        let mut writer = RotatingFileWriter::new(Some(path.clone()));
        writer.write_all(b"new session\n").expect("new log");

        assert_eq!(
            fs::read_to_string(previous_path(&path)).expect("previous log"),
            "old session\n"
        );
        let current = fs::read_to_string(&path).expect("current log");
        assert!(current.contains("session started:"));
        assert!(current.ends_with("new session\n"));

        drop(writer);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn writer_rotates_before_exceeding_the_size_limit() {
        let root = temporary_root("limit");
        fs::create_dir_all(&root).expect("temporary log root");
        let path = root.join("debug.log");
        let mut writer = RotatingFileWriter::new(Some(path.clone()));
        let payload = vec![b'x'; MAX_LOG_FILE_BYTES as usize];

        writer.write_all(&payload).expect("large log record");
        writer.write_all(b"next\n").expect("next log record");

        let current = fs::read(&path).expect("current log");
        let previous = fs::read(previous_path(&path)).expect("previous log");
        assert!(current.ends_with(b"next\n"));
        assert!(previous.len() >= MAX_LOG_FILE_BYTES as usize);

        drop(writer);
        let _ = fs::remove_dir_all(root);
    }

    fn temporary_root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "augur-git-logging-{label}-{}-{id}",
            std::process::id()
        ))
    }
}
