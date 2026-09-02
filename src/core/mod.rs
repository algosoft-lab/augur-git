//! Core services: Git commands, commit-graph calculations, configuration
//! persistence, build metadata, and localized copy.

pub mod build_info;
pub mod commit_diff;
pub mod commit_search;
pub mod config;
pub mod diff;
pub mod extension;
pub mod git;
pub mod graph;
pub mod i18n;
pub mod refs;
