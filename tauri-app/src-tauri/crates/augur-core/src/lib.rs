//! Platform-independent Git domain logic for Augur Git Tauri.
//!
//! This standalone crate contains no user-interface code: its modules build
//! Git arguments, parse Git output, and validate this application's persisted
//! data. The Tauri backend owns application state and the webview owns
//! rendering.

pub mod build_info;
pub mod cli;
pub mod commit_diff;
pub mod commit_search;
pub mod config;
pub mod diff;
pub mod git;
pub mod graph;
pub mod i18n;
pub mod keymap;
pub mod paths;
pub mod refs;
pub mod shell_install;

/// Git operations, diff parsing, and the plain-Git state probes that the
/// repository layer coordinates.
pub mod operations {
    pub use crate::git::operation_probe;
}
