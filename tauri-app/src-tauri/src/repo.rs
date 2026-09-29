//! One open repository: its Git worker, its request counters, and the thread
//! that forwards worker events to the webview.
//!
//! The worker itself is the one from the core crate, so Git behavior is
//! identical to the reference application. This module only owns the plumbing:
//! it gives each repository an identity, converts the worker's internal events
//! into serializable payloads, and keeps monotonically increasing ids so the
//! webview can drop late answers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

use tauri::{AppHandle, Emitter};

use augur_core::config::LocationConfig;
use augur_core::diff::{DiffDocument, FileChange};
use augur_core::git::{
    FileStatus, GitError, GitEvent, GitHandle, GitRepo, LogScope, WorkingTreeAction,
};

use crate::events::{REPO_EVENT, RepoEvent, RepoEventEnvelope};

/// An open repository.
pub struct RepoSession {
    id: u64,
    path: String,
    location: LocationConfig,
    repo: GitRepo,
    handle: GitHandle,
    /// Identity of the most recent working-tree mutation.
    working_operation: AtomicU64,
    /// Identity of the most recent revision comparison.
    compare_request: AtomicU64,
}

impl RepoSession {
    /// Start the Git worker for `repo` and begin forwarding its events.
    ///
    /// Local paths are validated synchronously so a bad path fails in the
    /// caller's command instead of arriving later as an asynchronous failure.
    pub fn open(
        app: AppHandle,
        id: u64,
        path: String,
        location: LocationConfig,
        repo: GitRepo,
    ) -> Result<Self, GitError> {
        let (event_tx, event_rx) = mpsc::channel::<GitEvent>();
        let handle = augur_core::git::spawn_open(repo.clone(), event_tx)?;
        let session = Self {
            id,
            path,
            location,
            repo,
            handle,
            working_operation: AtomicU64::new(0),
            compare_request: AtomicU64::new(0),
        };
        spawn_forwarder(app, id, event_rx);
        Ok(session)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn location(&self) -> &LocationConfig {
        &self.location
    }

    /// The repository handle used for synchronous read-only probes.
    pub fn repo(&self) -> &GitRepo {
        &self.repo
    }

    pub fn handle(&self) -> &GitHandle {
        &self.handle
    }

    pub fn refresh(&self) {
        self.handle.refresh();
    }

    pub fn set_log_scope(&self, scope: LogScope) {
        self.handle.log_query(scope);
    }

    pub fn request_more_log_page(&self) {
        self.handle.more_log_page();
    }

    /// Ask for the file list and full message of one commit.
    pub fn select_commit(&self, oid: String) {
        self.handle.commit_numstat(oid.clone());
        self.handle.commit_message(oid);
    }

    pub fn request_commit_message(&self, oid: String) {
        self.handle.commit_message(oid);
    }

    pub fn commit_file_diff(&self, oid: String, merge_parent: Option<String>, file: FileChange) {
        self.handle.commit_file_diff(oid, merge_parent, file);
    }

    /// Request a working-tree diff using the caller's event-correlation id.
    pub fn working_tree_diff(
        &self,
        request_id: u64,
        kind: augur_core::git::WorkingTreeDiffKind,
        file: FileStatus,
    ) {
        self.handle.working_tree_file_diff(request_id, kind, file);
    }

    /// Apply a staged/working-tree mutation and return the result id.
    pub fn working_tree_operation(
        &self,
        action: WorkingTreeAction,
        scope: augur_core::git::WorkingTreeScope,
    ) -> u64 {
        let request_id = self.working_operation.fetch_add(1, Ordering::Relaxed) + 1;
        self.handle
            .working_tree_operation(request_id, action, scope);
        request_id
    }

    /// Start a revision comparison. The previous comparison is cancelled, so a
    /// slow answer can never overwrite a newer one.
    pub fn start_compare(&self, base: CompareRevisionArg, target: CompareRevisionArg) -> u64 {
        let request_id = self.compare_request.fetch_add(1, Ordering::Relaxed) + 1;
        self.handle
            .branch_compare(request_id, base.into(), target.into());
        request_id
    }

    pub fn cancel_compare(&self) {
        self.handle.cancel_branch_compare();
    }

    pub fn export_patch(
        &self,
        base: CompareRevisionArg,
        target: CompareRevisionArg,
        destination: std::path::PathBuf,
    ) -> u64 {
        let request_id = self.compare_request.fetch_add(1, Ordering::Relaxed) + 1;
        self.handle
            .branch_compare_patch(request_id, base.into(), target.into(), destination);
        request_id
    }

    /// Run a named operation built by [`crate::git_args::GitAction`].
    pub fn run(&self, label: impl Into<String>, args: Vec<String>, refresh_after_success: bool) {
        self.handle
            .run_with_refresh(label, args, refresh_after_success);
    }

    /// Stop the worker. Called when a tab closes.
    pub fn close(&self) {
        self.handle.close();
    }
}

/// A comparison endpoint as it arrives from the webview.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CompareRevisionArg {
    Local { name: String },
    Remote { name: String },
    Tag { name: String },
    Commit { name: String },
}

impl From<CompareRevisionArg> for augur_core::git::CompareRevision {
    fn from(value: CompareRevisionArg) -> Self {
        use augur_core::git::{CompareRevision, CompareRevisionKind};
        match value {
            CompareRevisionArg::Local { name } => CompareRevision {
                full_name: format!("refs/heads/{name}"),
                name,
                kind: CompareRevisionKind::Local,
            },
            CompareRevisionArg::Remote { name } => CompareRevision {
                full_name: format!("refs/remotes/{name}"),
                name,
                kind: CompareRevisionKind::Remote,
            },
            CompareRevisionArg::Tag { name } => CompareRevision {
                full_name: format!("refs/tags/{name}"),
                name,
                kind: CompareRevisionKind::Tag,
            },
            CompareRevisionArg::Commit { name } => CompareRevision {
                full_name: name.clone(),
                name,
                kind: CompareRevisionKind::Commit,
            },
        }
    }
}

/// Start a thread that converts worker events into webview events.
fn spawn_forwarder(app: AppHandle, repo_id: u64, receiver: mpsc::Receiver<GitEvent>) {
    thread::Builder::new()
        .name(format!("augur-repo-{repo_id}-events"))
        .spawn(move || {
            while let Ok(event) = receiver.recv() {
                let Some(payload) = convert(repo_id, event) else {
                    continue;
                };
                if let Err(error) = app.emit(REPO_EVENT, payload) {
                    log::warn!("[events] failed to emit repository event: {error}");
                }
            }
            log::debug!("[events] repository {repo_id} worker finished");
        })
        .ok();
}

/// Convert one worker event. `None` means the event is intentionally dropped
/// because nothing in this application renders it.
fn convert(repo_id: u64, event: GitEvent) -> Option<RepoEventEnvelope> {
    use GitEvent as E;
    let event = match event {
        E::Status {
            branch,
            head,
            upstream,
            ahead,
            behind,
            files,
            branches,
        } => RepoEvent::Status {
            branch,
            head,
            upstream,
            ahead,
            behind,
            files,
            branches,
        },
        E::LogPage {
            rows,
            replace,
            has_more,
        } => RepoEvent::LogPage {
            rows,
            replace,
            has_more,
        },
        E::Refs(refs) => RepoEvent::Refs { refs },
        E::CommitFiles {
            oid,
            files,
            merge_parent,
        } => RepoEvent::CommitFiles {
            oid,
            files,
            merge_parent,
        },
        E::CommitMessage { oid, message } => RepoEvent::CommitMessage { oid, message },
        E::CommitFileDiff {
            oid,
            file,
            patch,
            old_source,
            new_source,
        } => {
            let label = display_path(&file);
            RepoEvent::FileDiff {
                oid,
                file,
                document: DiffDocument::from_patch(label, &patch, old_source, new_source).into(),
            }
        }
        E::WorkingTreeFileDiff {
            request_id,
            kind,
            file,
            patch,
            old_source,
            new_source,
        } => RepoEvent::WorkingTreeFileDiff {
            request_id,
            kind,
            file: file.clone(),
            document: DiffDocument::from_patch(file.path.clone(), &patch, old_source, new_source)
                .into(),
        },
        E::WorkingTreeFileDiffError {
            request_id,
            kind,
            file,
            detail,
        } => RepoEvent::WorkingTreeFileDiffError {
            request_id,
            kind,
            file,
            detail,
        },
        E::BranchCompareFiles { request_id, files } => {
            RepoEvent::BranchCompareFiles { request_id, files }
        }
        E::BranchCompareFileDiff {
            request_id,
            file,
            patch,
            old_source,
            new_source,
        } => {
            let label = display_path(&file);
            RepoEvent::BranchCompareFileDiff {
                request_id,
                file,
                document: DiffDocument::from_patch(label, &patch, old_source, new_source).into(),
            }
        }
        E::BranchCompareError {
            request_id,
            file,
            detail,
        } => RepoEvent::BranchCompareError {
            request_id,
            file,
            detail,
        },
        E::BranchCompareFinished { request_id } => RepoEvent::BranchCompareFinished { request_id },
        E::BranchComparePatchExported {
            request_id,
            destination,
            bytes,
        } => RepoEvent::BranchComparePatchExported {
            request_id,
            destination,
            bytes,
        },
        E::BranchComparePatchError { request_id, detail } => {
            RepoEvent::BranchComparePatchError { request_id, detail }
        }
        E::WorkingTreeOperationFinished {
            request_id,
            action,
            scope,
            success,
            detail,
        } => RepoEvent::WorkingTreeOperationFinished {
            request_id,
            action,
            scope,
            success,
            detail,
        },
        E::CommandStarted { label, subcommand } => RepoEvent::CommandStarted {
            verb: augur_core::git::progress_verb(&subcommand).to_string(),
            label,
        },
        E::CommandDone {
            label,
            success,
            message,
        } => RepoEvent::CommandDone {
            label,
            success,
            message,
        },
        E::StatusError(error) => RepoEvent::StatusError { error },
        E::OpenFailed(error) => RepoEvent::OpenFailed { error },
        E::Error(error) => RepoEvent::Error { error },
    };
    Some(RepoEventEnvelope { repo_id, event })
}

/// The label the file list shows. Git operations must use the structured
/// paths, never this display form.
fn display_path(file: &FileChange) -> String {
    file.path.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use augur_core::git::CompareRevisionKind;

    #[test]
    fn local_revisions_use_a_fully_qualified_ref() {
        let revision: augur_core::git::CompareRevision = CompareRevisionArg::Local {
            name: "main".into(),
        }
        .into();
        assert_eq!(revision.full_name, "refs/heads/main");
        assert_eq!(revision.kind, CompareRevisionKind::Local);
    }

    #[test]
    fn remote_and_tag_revisions_are_qualified_per_kind() {
        let remote: augur_core::git::CompareRevision = CompareRevisionArg::Remote {
            name: "origin/topic".into(),
        }
        .into();
        assert_eq!(remote.full_name, "refs/remotes/origin/topic");
        let tag: augur_core::git::CompareRevision =
            CompareRevisionArg::Tag { name: "v1".into() }.into();
        assert_eq!(tag.full_name, "refs/tags/v1");
    }

    #[test]
    fn commit_revisions_pass_the_object_id_through() {
        let commit: augur_core::git::CompareRevision = CompareRevisionArg::Commit {
            name: "abc1234".into(),
        }
        .into();
        assert_eq!(commit.full_name, "abc1234");
        assert_eq!(commit.kind, CompareRevisionKind::Commit);
    }
}
