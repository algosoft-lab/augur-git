//! Platform-independent Git domain logic for Augur Git Tauri.
//!
//! This crate is a decoupled copy of the pure domain layer of the GPUI
//! application. It contains no user-interface code: every module here either
//! builds Git arguments, parses Git output, or validates persisted data. The
//! Tauri backend owns the application state and the webview owns rendering.
//!
//! The two applications are intentionally independent products. Nothing in
//! this crate reads or writes the GPUI application's configuration, and the
//! persisted schemas carry their own version markers so neither application
//! can interpret the other's files.

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
