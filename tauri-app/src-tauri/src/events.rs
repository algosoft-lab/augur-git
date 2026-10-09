//! The event protocol between the Git workers and the webview.
//!
//! Every event carries the repository it belongs to and, where a response can
//! arrive late, a request id. The webview discards results whose id is not the
//! newest one it asked for, so switching files or comparison endpoints quickly
//! can never show a stale diff.

use std::ops::Range;
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
/// Event used to select a section in the already-open settings window.
pub const SETTINGS_NAVIGATE_EVENT: &str = "augur://settings-navigate";
/// Event name used when a repository folder is dropped onto a window.
pub const DROP_EVENT: &str = "augur://drop-paths";
/// Event name used to show or hide the folder-drag prompt.
pub const DRAG_STATE_EVENT: &str = "augur://drag-state";

/// A parsed single-file diff, prepared for the viewer.
///
/// The rows the inline layout draws and the aligned rows the side-by-side layout
/// draws are both sent, because the pairing rule is implemented once in the core
/// crate and the webview only has to choose which list to mount. The
/// character-level ranges are sent with the same reasoning: computing them here
/// keeps one implementation and avoids a second round trip per file.
#[derive(Clone, Debug, Serialize)]
pub struct DiffPayload {
    pub path: String,
    pub language: Option<String>,
    pub rows: Vec<augur_core::diff::DiffRow>,
    pub aligned_rows: Vec<augur_core::diff::DiffRow>,
    pub old_source: Option<augur_core::diff::SourceText>,
    pub new_source: Option<augur_core::diff::SourceText>,
    /// Ranges into the old source, per line, for inline highlighting.
    pub inline_old: Vec<Vec<Range<usize>>>,
    /// Ranges into the new source, per line, for inline highlighting.
    pub inline_new: Vec<Vec<Range<usize>>>,
    pub binary: bool,
    /// Unified text for the clipboard.
    pub copy_text: String,
}

impl From<DiffDocument> for DiffPayload {
    fn from(document: DiffDocument) -> Self {
        // The derived values are read before the fields are moved out.
        let ranges = augur_core::diff::inline::inline_ranges(&document);
        let aligned_rows = document.aligned_rows();
        let copy_text = document.copy_text();
        Self {
            path: document.path,
            language: document.language,
            rows: document.rows,
            aligned_rows,
            old_source: document.old_source,
            new_source: document.new_source,
            inline_old: ranges.old,
            inline_new: ranges.new,
            binary: document.binary,
            copy_text,
        }
    }
}

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
        diff_stats: augur_core::git::WorkingTreeDiffStats,
        branches: Vec<augur_core::git::BranchInfo>,
    },
    LogPage {
        rows: Vec<LogRow>,
        replace: bool,
        #[serde(rename = "hasMore")]
        has_more: bool,
    },
    Refs {
        refs: RefsInfo,
    },
    CommitFiles {
        #[serde(rename = "requestId")]
        request_id: u64,
        oid: String,
        files: Vec<FileChange>,
        merge_parent: Option<String>,
    },
    CommitFilesError {
        #[serde(rename = "requestId")]
        request_id: u64,
        oid: String,
        error: GitError,
    },
    CommitMessage {
        oid: String,
        message: CommitMessage,
    },
    /// A commit diff, already parsed into rows plus both source texts.
    FileDiff {
        #[serde(rename = "requestId")]
        request_id: u64,
        oid: String,
        file: FileChange,
        document: DiffPayload,
    },
    FileDiffError {
        #[serde(rename = "requestId")]
        request_id: u64,
        oid: String,
        file: FileChange,
        error: GitError,
    },
    WorkingTreeFileDiff {
        #[serde(rename = "requestId")]
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
        document: DiffPayload,
    },
    WorkingTreeFileDiffError {
        #[serde(rename = "requestId")]
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
        detail: String,
    },
    WorkingTreeOperationFinished {
        #[serde(rename = "requestId")]
        request_id: u64,
        action: WorkingTreeAction,
        scope: WorkingTreeScopeKind,
        success: bool,
        detail: String,
    },
    BranchCompareFiles {
        #[serde(rename = "requestId")]
        request_id: u64,
        files: Vec<FileChange>,
    },
    BranchCompareFileDiff {
        #[serde(rename = "requestId")]
        request_id: u64,
        file: FileChange,
        document: DiffPayload,
    },
    BranchCompareError {
        #[serde(rename = "requestId")]
        request_id: u64,
        file: Option<FileChange>,
        detail: String,
    },
    BranchCompareFinished {
        #[serde(rename = "requestId")]
        request_id: u64,
    },
    BranchComparePatchExported {
        #[serde(rename = "requestId")]
        request_id: u64,
        destination: PathBuf,
        bytes: u64,
    },
    BranchComparePatchError {
        #[serde(rename = "requestId")]
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
        let mut value = serde_json::to_value(&self.event).map_err(serde::ser::Error::custom)?;
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
    Notice { level: String, message: String },
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

/// Drag state for a native folder drag over one webview.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragStatePayload {
    pub label: String,
    pub active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_state_payload_serializes_for_the_webview() {
        let payload = DragStatePayload {
            label: "main".into(),
            active: true,
        };
        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["label"], "main");
        assert_eq!(json["active"], true);
    }

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

    #[test]
    fn compare_events_serialize_request_ids_for_the_webview() {
        let envelope = RepoEventEnvelope {
            repo_id: 7,
            event: RepoEvent::BranchCompareFinished { request_id: 42 },
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["requestId"], 42);
        assert!(json.get("request_id").is_none());
    }

    #[test]
    fn commit_diff_errors_serialize_the_request_id_for_the_webview() {
        let envelope = RepoEventEnvelope {
            repo_id: 7,
            event: RepoEvent::CommitFilesError {
                request_id: 73,
                oid: "abc123".into(),
                error: GitError::new("err-numstat", "bad object"),
            },
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["type"], "commitFilesError");
        assert_eq!(json["requestId"], 73);
        assert!(json.get("request_id").is_none());
    }

    #[test]
    fn compare_file_diffs_carry_the_viewer_payload_for_the_webview() {
        let patch = "diff --git a/src/lib.rs b/src/lib.rs\n\
                     --- a/src/lib.rs\n\
                     +++ b/src/lib.rs\n\
                     @@ -1,1 +1,1 @@\n\
                     -old\n\
                     +new\n";
        let envelope = RepoEventEnvelope {
            repo_id: 7,
            event: RepoEvent::BranchCompareFileDiff {
                request_id: 42,
                file: FileChange {
                    path: "src/lib.rs".into(),
                    old_path: None,
                    new_path: "src/lib.rs".into(),
                    status: augur_core::diff::FileChangeStatus::Modified,
                    old_blob: None,
                    new_blob: None,
                    added: Some(1),
                    deleted: Some(1),
                },
                document: DiffDocument::from_patch(
                    "src/lib.rs",
                    patch,
                    Some("old\n".into()),
                    Some("new\n".into()),
                )
                .into(),
            },
        };
        let json = serde_json::to_value(&envelope).unwrap();
        let document = &json["document"];
        // The viewer indexes these directly; a missing field is a render crash,
        // not a degraded view.
        assert!(document["inline_old"].is_array());
        assert!(document["inline_new"].is_array());
        assert!(document["aligned_rows"].is_array());
        assert!(document["copy_text"].is_string());
        assert!(
            !document["rows"]
                .as_array()
                .expect("parsed diff rows")
                .is_empty()
        );
    }

    #[test]
    fn log_pages_serialize_the_more_flag_for_the_webview() {
        let json = serde_json::to_value(RepoEvent::LogPage {
            rows: Vec::new(),
            replace: true,
            has_more: false,
        })
        .unwrap();
        assert_eq!(json["hasMore"], false);
        assert!(json.get("has_more").is_none());
    }
}
