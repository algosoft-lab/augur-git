//! The event protocol between the Git workers and the webview.
//!
//! Every event carries the repository it belongs to and, where a response can
//! arrive late, a request id. The webview discards results whose id is not the
//! newest one it asked for, so switching files or comparison endpoints quickly
//! can never show a stale diff.

use std::path::PathBuf;

use serde::Serialize;

use augur_core::diff::{DiffDocument, FileChange};
use augur_core::git::{
    CommitMessage, FileStatus, GitError, RefsInfo, WorkingTreeAction, WorkingTreeDiffKind,
    WorkingTreeScopeKind,
};
use augur_core::graph::LogRow;

/// Event name used for every repository event.
pub const REPO_EVENT: &str = "augur://repo-event";
/// Event name used for application-level state changes.
pub const APP_EVENT: &str = "augur://app-event";
/// Event name used to hand CLI paths to a running instance.
pub const OPEN_PATHS_EVENT: &str = "augur://open-paths";
/// Event name used when a native menu item is activated.
pub const MENU_EVENT: &str = "augur://menu";
/// Event name used to tell a window it regained focus.
pub const WINDOW_FOCUS_EVENT: &str = "augur://window-focus";
/// Event name used when a repository folder is dropped onto a window.
pub const DROP_EVENT: &str = "augur://drop-paths";

/// One repository event, already resolved to plain data.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RepoEvent {
    Status {
        branch: String,
        head: Option<String>,
        upstream: Option<String>,
        ahead: usize,
        behind: usize,
        files: Vec<FileStatus>,
        branches: Vec<augur_core::git::BranchInfo>,
    },
    LogPage {
        rows: Vec<LogRow>,
        replace: bool,
        has_more: bool,
    },
    Refs {
        refs: RefsInfo,
    },
    CommitFiles {
        oid: String,
        files: Vec<FileChange>,
        merge_parent: Option<String>,
    },
    CommitMessage {
        oid: String,
        message: CommitMessage,
    },
    /// A commit diff, already parsed into rows plus both source texts.
    FileDiff {
        oid: String,
        file: FileChange,
        document: DiffDocument,
    },
    WorkingTreeFileDiff {
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
        document: DiffDocument,
    },
    WorkingTreeFileDiffError {
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
        detail: String,
    },
    WorkingTreeOperationFinished {
        request_id: u64,
        action: WorkingTreeAction,
        scope: WorkingTreeScopeKind,
        success: bool,
        detail: String,
    },
    BranchCompareFiles {
        request_id: u64,
        files: Vec<FileChange>,
    },
    BranchCompareFileDiff {
        request_id: u64,
        file: FileChange,
        document: DiffDocument,
    },
    BranchCompareError {
        request_id: u64,
        file: Option<FileChange>,
        detail: String,
    },
    BranchCompareFinished {
        request_id: u64,
    },
    BranchComparePatchExported {
        request_id: u64,
        destination: PathBuf,
        bytes: u64,
    },
    BranchComparePatchError {
        request_id: u64,
        detail: String,
    },
    CommandStarted {
        label: String,
        /// English in-progress verb for the status bar.
        verb: String,
    },
    CommandDone {
        label: String,
        success: bool,
        message: String,
    },
    /// The repository could not be opened. The session is dropped afterwards.
    OpenFailed {
        error: GitError,
    },
    StatusError {
        error: GitError,
    },
    Error {
        error: GitError,
    },
}

/// A repository event with the repository it belongs to.
///
/// The payload is an internally tagged enum, which `serde(flatten)` cannot
/// merge into a struct, so the two maps are combined by hand.
#[derive(Clone, Debug)]
pub struct RepoEventEnvelope {
    pub repo_id: u64,
    pub event: RepoEvent,
}

impl Serialize for RepoEventEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value =
            serde_json::to_value(&self.event).map_err(serde::ser::Error::custom)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("repoId".to_string(), serde_json::json!(self.repo_id));
        }
        value.serialize(serializer)
    }
}

/// Application-level notifications that are not tied to one repository.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AppEvent {
    /// A settings change was applied; every window must re-render.
    SettingsChanged,
    /// The tab list or active tab changed.
    WorkspaceChanged,
    /// A destructive or generated file operation needs the user's attention.
    Notice {
        level: String,
        message: String,
    },
}

/// Envelope for [`AppEvent`].
#[derive(Clone, Debug)]
pub struct AppEventEnvelope {
    pub event: AppEvent,
}

impl Serialize for AppEventEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_json::to_value(&self.event)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

/// Repository paths handed over by a second launch of the application.
#[derive(Clone, Debug, Serialize)]
pub struct OpenPathsPayload {
    pub paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_events_serialize_with_a_tagged_type() {
        let envelope = RepoEventEnvelope {
            repo_id: 7,
            event: RepoEvent::CommandDone {
                label: "push".into(),
                success: false,
                message: "rejected".into(),
            },
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["repoId"], 7);
        assert_eq!(json["type"], "commandDone");
        assert_eq!(json["label"], "push");
    }

    #[test]
    fn working_tree_kinds_use_camel_case() {
        let json = serde_json::to_value(WorkingTreeDiffKind::Unstaged).unwrap();
        assert_eq!(json, "unstaged");
    }
}
