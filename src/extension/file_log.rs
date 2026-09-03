//! Append-only file logging for trusted Lua extensions.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Bound one host request so a single extension call cannot monopolize the
/// extension queue with an unexpectedly large write.
pub(super) const MAX_EXTENSION_LOG_ENTRY_BYTES: usize = 1024 * 1024;

/// Errors returned by the extension file logger.
#[derive(Debug)]
pub(super) enum ExtensionFileLogError {
    InvalidPath(&'static str),
    EntryTooLarge {
        bytes: usize,
        max: usize,
    },
    LockPoisoned,
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl ExtensionFileLogError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath(_) => "invalid_log_path",
            Self::EntryTooLarge { .. } => "log_entry_too_large",
            Self::LockPoisoned => "log_logger_unavailable",
            Self::Io { .. } => "log_write_failed",
        }
    }
}

impl fmt::Display for ExtensionFileLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(reason) => {
                write!(formatter, "invalid extension log path: {reason}")
            }
            Self::EntryTooLarge { bytes, max } => write!(
                formatter,
                "extension log entry is {bytes} bytes; the maximum is {max} bytes"
            ),
            Self::LockPoisoned => {
                formatter.write_str("extension log service is unavailable")
            }
            Self::Io { operation, source } => {
                write!(
                    formatter,
                    "could not {operation} extension log: {source}"
                )
            }
        }
    }
}

/// Shared append service. A single lock keeps each request's bytes together
/// when several extension workers target the same file.
#[derive(Clone, Default)]
pub(super) struct ExtensionFileLogger {
    lock: Arc<Mutex<()>>,
}

impl ExtensionFileLogger {
    pub(super) fn append(
        &self,
        path: &str,
        content: &str,
    ) -> Result<usize, ExtensionFileLogError> {
        let path = validate(path, content)?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| ExtensionFileLogError::LockPoisoned)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                ExtensionFileLogError::Io {
                    operation: "create the extension log directory",
                    source,
                }
            })?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| ExtensionFileLogError::Io {
                operation: "open the extension log",
                source,
            })?;
        file.write_all(content.as_bytes()).map_err(|source| {
            ExtensionFileLogError::Io {
                operation: "append the extension log",
                source,
            }
        })?;
        file.flush().map_err(|source| ExtensionFileLogError::Io {
            operation: "flush the extension log",
            source,
        })?;
        Ok(content.len())
    }
}

fn validate(
    path: &str,
    content: &str,
) -> Result<PathBuf, ExtensionFileLogError> {
    if path.trim().is_empty() {
        return Err(ExtensionFileLogError::InvalidPath(
            "path must not be empty",
        ));
    }
    if path.as_bytes().contains(&0) {
        return Err(ExtensionFileLogError::InvalidPath(
            "path must not contain a NUL byte",
        ));
    }
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(ExtensionFileLogError::InvalidPath(
            "path must be absolute",
        ));
    }
    if content.len() > MAX_EXTENSION_LOG_ENTRY_BYTES {
        return Err(ExtensionFileLogError::EntryTooLarge {
            bytes: content.len(),
            max: MAX_EXTENSION_LOG_ENTRY_BYTES,
        });
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use super::*;

    fn temporary_root(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "augur-git-extension-log-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn append_creates_parent_and_preserves_existing_content() {
        let root = temporary_root("append");
        let path = root.join("nested").join("run.log");
        let logger = ExtensionFileLogger::default();

        assert_eq!(
            logger.append(path.to_str().unwrap(), "first\n").unwrap(),
            6
        );
        assert_eq!(logger.append(path.to_str().unwrap(), "second").unwrap(), 6);
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_relative_empty_and_oversized_requests() {
        let logger = ExtensionFileLogger::default();
        assert!(matches!(
            logger.append("relative.log", "text"),
            Err(ExtensionFileLogError::InvalidPath(_))
        ));
        assert!(matches!(
            logger.append("", "text"),
            Err(ExtensionFileLogError::InvalidPath(_))
        ));
        let oversized = "x".repeat(MAX_EXTENSION_LOG_ENTRY_BYTES + 1);
        let path = temporary_root("oversized").join("run.log");
        assert!(matches!(
            logger.append(path.to_str().unwrap(), &oversized),
            Err(ExtensionFileLogError::EntryTooLarge { .. })
        ));
    }

    #[test]
    fn concurrent_appends_keep_each_request_contiguous() {
        let root = temporary_root("concurrent");
        let path = root.join("run.log");
        let logger = ExtensionFileLogger::default();
        let mut workers = Vec::new();
        for index in 0..8 {
            let logger = logger.clone();
            let path = path.clone();
            workers.push(thread::spawn(move || {
                let line = format!("worker-{index}\n");
                for _ in 0..16 {
                    logger.append(path.to_str().unwrap(), &line).unwrap();
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }

        let contents = fs::read_to_string(&path).unwrap();
        let lines = contents.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 8 * 16);
        assert!(lines.iter().all(|line| {
            (0..8).any(|index| *line == format!("worker-{index}"))
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opening_a_directory_reports_a_structured_write_error() {
        let root = temporary_root("directory");
        fs::create_dir_all(&root).unwrap();
        let logger = ExtensionFileLogger::default();
        let error = logger.append(root.to_str().unwrap(), "text").unwrap_err();
        assert_eq!(error.code(), "log_write_failed");
        let _ = fs::remove_dir_all(root);
    }
}
