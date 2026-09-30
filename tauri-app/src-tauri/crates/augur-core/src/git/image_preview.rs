//! Bounded reads for the image preview shown beside a file diff.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};
use std::process::Stdio;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::diff::{FileChange, FileChangeStatus};

use super::{FileStatus, GitRepo, RepoLocation, WorkingTreeDiffKind};

pub const MAX_IMAGE_PREVIEW_SIZE: usize = 10 * 1024 * 1024;

/// A change to preview. File metadata is resolved to trusted Git sources here,
/// rather than accepting arbitrary Git revisions or paths from the webview.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImagePreviewTarget {
    Change {
        file: FileChange,
    },
    WorkingTree {
        #[serde(rename = "diffKind")]
        diff_kind: WorkingTreeDiffKind,
        file: FileStatus,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct ImagePreview {
    pub old: ImagePreviewSide,
    pub new: ImagePreviewSide,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ImagePreviewSide {
    Absent,
    Available {
        #[serde(rename = "mimeType")]
        mime_type: String,
        data: String,
    },
    Unavailable {
        reason: ImagePreviewUnavailable,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImagePreviewUnavailable {
    Unsupported,
    TooLarge,
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SourceKind {
    Blob(Option<String>),
    Head,
    Index,
    Worktree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    path: String,
    kind: SourceKind,
}

/// Read and encode the two versions without blocking the caller's async task.
pub fn load(repo: &GitRepo, target: &ImagePreviewTarget) -> ImagePreview {
    let (old, new) = sources(target);
    ImagePreview {
        old: load_side(repo, old.as_ref()),
        new: load_side(repo, new.as_ref()),
    }
}

fn sources(target: &ImagePreviewTarget) -> (Option<Source>, Option<Source>) {
    match target {
        ImagePreviewTarget::Change { file } => {
            let old_path = file.old_path.as_deref().unwrap_or(&file.new_path);
            let old = (!matches!(file.status, FileChangeStatus::Added)).then(|| Source {
                path: old_path.to_string(),
                kind: SourceKind::Blob(file.old_blob.clone()),
            });
            let new = (!matches!(file.status, FileChangeStatus::Deleted)).then(|| Source {
                path: file.new_path.clone(),
                kind: SourceKind::Blob(file.new_blob.clone()),
            });
            (old, new)
        }
        ImagePreviewTarget::WorkingTree { diff_kind, file } => match diff_kind {
            WorkingTreeDiffKind::Staged => {
                let old_path = file.old_path.as_deref().unwrap_or(&file.path);
                let old = (file.index != 'A').then(|| Source {
                    path: old_path.to_string(),
                    kind: SourceKind::Head,
                });
                let new = (file.index != 'D').then(|| Source {
                    path: file.path.clone(),
                    kind: SourceKind::Index,
                });
                (old, new)
            }
            WorkingTreeDiffKind::Unstaged => {
                let old_path = if file.index == ' ' {
                    file.old_path.as_deref().unwrap_or(&file.path)
                } else {
                    &file.path
                };
                let old = (!file.is_untracked() && file.worktree != 'A').then(|| Source {
                    path: old_path.to_string(),
                    kind: SourceKind::Index,
                });
                let new = (file.worktree != 'D').then(|| Source {
                    path: file.path.clone(),
                    kind: SourceKind::Worktree,
                });
                (old, new)
            }
        },
    }
}

fn load_side(repo: &GitRepo, source: Option<&Source>) -> ImagePreviewSide {
    let Some(source) = source else {
        return ImagePreviewSide::Absent;
    };
    let Some(mime_type) = mime_type(&source.path) else {
        return ImagePreviewSide::Unavailable {
            reason: ImagePreviewUnavailable::Unsupported,
        };
    };
    let result = match &source.kind {
        SourceKind::Blob(Some(oid)) if valid_oid(oid) => read_git_blob(repo, oid),
        SourceKind::Blob(_) => Err(ImagePreviewUnavailable::Unreadable),
        SourceKind::Head => read_git_spec(repo, &source.path, true),
        SourceKind::Index => read_git_spec(repo, &source.path, false),
        SourceKind::Worktree => read_worktree(repo, &source.path),
    };
    match result {
        Ok(bytes) => ImagePreviewSide::Available {
            mime_type: mime_type.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
        Err(reason) => ImagePreviewSide::Unavailable { reason },
    }
}

fn mime_type(path: &str) -> Option<&'static str> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "ico" => Some("image/x-icon"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_relative_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        && !path
            .to_string_lossy()
            .bytes()
            .any(|byte| byte == 0 || byte < 0x20 || byte == 0x7f)
}

fn read_git_spec(
    repo: &GitRepo,
    path: &str,
    head: bool,
) -> Result<Vec<u8>, ImagePreviewUnavailable> {
    if !valid_relative_path(path) {
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    let spec = if head {
        format!("HEAD:{path}")
    } else {
        format!(":{path}")
    };
    read_git_blob(repo, &spec)
}

fn read_git_blob(repo: &GitRepo, object: &str) -> Result<Vec<u8>, ImagePreviewUnavailable> {
    let size = repo
        .command()
        .args(["--no-pager", "-C", repo.path(), "cat-file", "-s", object])
        .output()
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
    if !size.status.success() {
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    let size = String::from_utf8_lossy(&size.stdout)
        .trim()
        .parse::<usize>()
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
    if size > MAX_IMAGE_PREVIEW_SIZE {
        return Err(ImagePreviewUnavailable::TooLarge);
    }

    let mut child = repo
        .command()
        .args(["--no-pager", "-C", repo.path(), "cat-file", "blob", object])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
    let mut bytes = Vec::new();
    let read_result = child
        .stdout
        .take()
        .ok_or(ImagePreviewUnavailable::Unreadable)?
        .take((MAX_IMAGE_PREVIEW_SIZE + 1) as u64)
        .read_to_end(&mut bytes);
    if read_result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    if bytes.len() > MAX_IMAGE_PREVIEW_SIZE {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ImagePreviewUnavailable::TooLarge);
    }
    if !child
        .wait()
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?
        .success()
    {
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    Ok(bytes)
}

fn read_worktree(repo: &GitRepo, path: &str) -> Result<Vec<u8>, ImagePreviewUnavailable> {
    if !valid_relative_path(path) {
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    match repo.location() {
        RepoLocation::Local => {
            let root = Path::new(repo.path())
                .canonicalize()
                .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
            let full_path = root.join(path);
            let resolved = full_path
                .canonicalize()
                .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
            if !resolved.starts_with(&root) {
                return Err(ImagePreviewUnavailable::Unreadable);
            }
            let metadata =
                fs::metadata(&resolved).map_err(|_| ImagePreviewUnavailable::Unreadable)?;
            if metadata.len() > MAX_IMAGE_PREVIEW_SIZE as u64 {
                return Err(ImagePreviewUnavailable::TooLarge);
            }
            let file = File::open(resolved).map_err(|_| ImagePreviewUnavailable::Unreadable)?;
            read_limited(file)
        }
        RepoLocation::Wsl { .. } => {
            let root = run_location(repo, "realpath", &["-e", "--", repo.path()])?;
            let joined = Path::new(repo.path()).join(path);
            let joined = joined.to_string_lossy().into_owned();
            let resolved = run_location(repo, "realpath", &["-e", "--", &joined])?;
            let root = root.trim_end_matches('/');
            let prefix = format!("{root}/");
            if !resolved.starts_with(&prefix) {
                return Err(ImagePreviewUnavailable::Unreadable);
            }
            read_location_file(repo, path)
        }
    }
}

fn run_location(
    repo: &GitRepo,
    program: &str,
    args: &[&str],
) -> Result<String, ImagePreviewUnavailable> {
    let output = repo
        .command_in_location(program)
        .args(args)
        .output()
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
    if !output.status.success() {
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn read_location_file(repo: &GitRepo, path: &str) -> Result<Vec<u8>, ImagePreviewUnavailable> {
    let mut child = repo
        .command_in_location("cat")
        .args(["--", path])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
    let mut bytes = Vec::new();
    let read_result = child
        .stdout
        .take()
        .ok_or(ImagePreviewUnavailable::Unreadable)?
        .take((MAX_IMAGE_PREVIEW_SIZE + 1) as u64)
        .read_to_end(&mut bytes);
    if read_result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    if bytes.len() > MAX_IMAGE_PREVIEW_SIZE {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ImagePreviewUnavailable::TooLarge);
    }
    if !child
        .wait()
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?
        .success()
    {
        return Err(ImagePreviewUnavailable::Unreadable);
    }
    Ok(bytes)
}

fn read_limited(mut reader: impl Read) -> Result<Vec<u8>, ImagePreviewUnavailable> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((MAX_IMAGE_PREVIEW_SIZE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ImagePreviewUnavailable::Unreadable)?;
    if bytes.len() > MAX_IMAGE_PREVIEW_SIZE {
        return Err(ImagePreviewUnavailable::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn file_change(status: FileChangeStatus, old_path: Option<&str>, new_path: &str) -> FileChange {
        FileChange {
            path: new_path.to_string(),
            old_path: old_path.map(str::to_string),
            new_path: new_path.to_string(),
            status,
            old_blob: Some("a".repeat(40)),
            new_blob: Some("b".repeat(40)),
            added: None,
            deleted: None,
        }
    }

    #[test]
    fn change_sources_cover_additions_deletions_and_renames() {
        let added = ImagePreviewTarget::Change {
            file: file_change(FileChangeStatus::Added, None, "assets/new.png"),
        };
        let deleted = ImagePreviewTarget::Change {
            file: file_change(
                FileChangeStatus::Deleted,
                Some("assets/old.png"),
                "assets/old.png",
            ),
        };
        let renamed = ImagePreviewTarget::Change {
            file: file_change(
                FileChangeStatus::Renamed,
                Some("assets/old.png"),
                "assets/new.png",
            ),
        };

        let (old, new) = sources(&added);
        assert!(old.is_none());
        assert_eq!(new.unwrap().path, "assets/new.png");
        let (old, new) = sources(&deleted);
        assert_eq!(old.unwrap().path, "assets/old.png");
        assert!(new.is_none());
        let (old, new) = sources(&renamed);
        assert_eq!(old.unwrap().path, "assets/old.png");
        assert_eq!(new.unwrap().path, "assets/new.png");
    }

    #[test]
    fn working_tree_sources_match_staged_and_unstaged_diff_sides() {
        let staged_added = ImagePreviewTarget::WorkingTree {
            diff_kind: WorkingTreeDiffKind::Staged,
            file: FileStatus {
                index: 'A',
                worktree: ' ',
                path: "new.png".into(),
                old_path: None,
            },
        };
        let unstaged_added = ImagePreviewTarget::WorkingTree {
            diff_kind: WorkingTreeDiffKind::Unstaged,
            file: FileStatus {
                index: '?',
                worktree: '?',
                path: "new.png".into(),
                old_path: None,
            },
        };
        let staged_rename = ImagePreviewTarget::WorkingTree {
            diff_kind: WorkingTreeDiffKind::Staged,
            file: FileStatus {
                index: 'R',
                worktree: ' ',
                path: "new.png".into(),
                old_path: Some("old.png".into()),
            },
        };

        let (old, new) = sources(&staged_added);
        assert!(old.is_none());
        assert_eq!(new.unwrap().kind, SourceKind::Index);
        let (old, new) = sources(&unstaged_added);
        assert!(old.is_none());
        assert_eq!(new.unwrap().kind, SourceKind::Worktree);
        let (old, new) = sources(&staged_rename);
        assert_eq!(old.unwrap().path, "old.png");
        assert_eq!(new.unwrap().path, "new.png");
    }

    #[test]
    fn reads_working_tree_previews_from_frontend_payloads() {
        let repo = TestRepo::new();
        repo.write("modified.png", b"modified before");
        repo.write("staged.png", b"staged before");
        repo.git(&["add", "modified.png", "staged.png"]);
        repo.git(&["commit", "-m", "Add preview fixtures"]);

        repo.write("modified.png", b"modified after");
        repo.write("staged.png", b"staged after");
        repo.git(&["add", "staged.png"]);
        repo.write("untracked.png", b"untracked image");

        let repo_handle = GitRepo::local(repo.path.to_string_lossy());
        let cases = [
            (
                serde_json::json!({
                    "kind": "workingTree",
                    "diffKind": "unstaged",
                    "file": {
                        "index": " ",
                        "worktree": "M",
                        "path": "modified.png",
                        "old_path": null
                    }
                }),
                Some(b"modified before".as_slice()),
                Some(b"modified after".as_slice()),
            ),
            (
                serde_json::json!({
                    "kind": "workingTree",
                    "diffKind": "staged",
                    "file": {
                        "index": "M",
                        "worktree": " ",
                        "path": "staged.png",
                        "old_path": null
                    }
                }),
                Some(b"staged before".as_slice()),
                Some(b"staged after".as_slice()),
            ),
            (
                serde_json::json!({
                    "kind": "workingTree",
                    "diffKind": "unstaged",
                    "file": {
                        "index": "?",
                        "worktree": "?",
                        "path": "untracked.png",
                        "old_path": null
                    }
                }),
                None,
                Some(b"untracked image".as_slice()),
            ),
        ];

        for (payload, expected_old, expected_new) in cases {
            let target: ImagePreviewTarget = serde_json::from_value(payload).unwrap();
            let preview = load(&repo_handle, &target);
            match (expected_old, preview.old) {
                (Some(expected), ImagePreviewSide::Available { data, .. }) => assert_eq!(
                    data,
                    base64::engine::general_purpose::STANDARD.encode(expected)
                ),
                (None, ImagePreviewSide::Absent) => {}
                (expected, actual) => {
                    panic!("unexpected old image side: {expected:?} / {actual:?}")
                }
            }
            match (expected_new, preview.new) {
                (Some(expected), ImagePreviewSide::Available { data, .. }) => assert_eq!(
                    data,
                    base64::engine::general_purpose::STANDARD.encode(expected)
                ),
                (None, ImagePreviewSide::Absent) => {}
                (expected, actual) => {
                    panic!("unexpected new image side: {expected:?} / {actual:?}")
                }
            }
        }
    }

    #[test]
    fn preview_serializes_with_the_frontend_field_names() {
        let preview = ImagePreview {
            old: ImagePreviewSide::Available {
                mime_type: "image/png".into(),
                data: "aGVsbG8=".into(),
            },
            new: ImagePreviewSide::Unavailable {
                reason: ImagePreviewUnavailable::TooLarge,
            },
        };

        let value = serde_json::to_value(preview).unwrap();
        assert_eq!(value["old"]["status"], "available");
        assert_eq!(value["old"]["mimeType"], "image/png");
        assert_eq!(value["old"]["data"], "aGVsbG8=");
        assert_eq!(value["new"]["status"], "unavailable");
        assert_eq!(value["new"]["reason"], "tooLarge");
    }

    #[test]
    fn supported_extensions_are_case_insensitive_and_mime_types_are_specific() {
        assert_eq!(mime_type("icon.PNG"), Some("image/png"));
        assert_eq!(mime_type("photo.JPEG"), Some("image/jpeg"));
        assert_eq!(mime_type("favicon.ico"), Some("image/x-icon"));
        assert_eq!(mime_type("logo.SVG"), Some("image/svg+xml"));
        assert_eq!(mime_type("notes.txt"), None);
    }

    #[test]
    fn rejects_escape_paths_and_invalid_object_ids() {
        assert!(!valid_relative_path("../outside.png"));
        assert!(!valid_relative_path("/outside.png"));
        assert!(valid_relative_path("assets/inside.png"));
        assert!(!valid_oid("not-an-object"));
        assert!(valid_oid(&"a".repeat(40)));
    }

    #[test]
    fn bounded_reader_stops_after_the_supported_size_limit() {
        let bytes = vec![0; MAX_IMAGE_PREVIEW_SIZE + 1];
        assert!(matches!(
            read_limited(bytes.as_slice()),
            Err(ImagePreviewUnavailable::TooLarge)
        ));
    }

    #[test]
    fn reads_before_and_after_blobs_as_base64() {
        let repo = TestRepo::new();
        repo.write("art.png", b"old image");
        repo.git(&["add", "art.png"]);
        repo.git(&["commit", "-m", "Add image"]);
        let old_blob = repo.git(&["rev-parse", "HEAD:art.png"]);
        repo.write("art.png", b"new image");
        let new_blob = repo.git(&["hash-object", "-w", "art.png"]);
        let target = ImagePreviewTarget::Change {
            file: FileChange {
                path: "art.png".into(),
                old_path: None,
                new_path: "art.png".into(),
                status: FileChangeStatus::Modified,
                old_blob: Some(old_blob),
                new_blob: Some(new_blob),
                added: None,
                deleted: None,
            },
        };

        let preview = load(&GitRepo::local(repo.path.to_string_lossy()), &target);
        let ImagePreviewSide::Available { data: old, .. } = preview.old else {
            panic!("old image should be available");
        };
        let ImagePreviewSide::Available { data: new, .. } = preview.new else {
            panic!("new image should be available");
        };
        assert_eq!(
            old,
            base64::engine::general_purpose::STANDARD.encode(b"old image")
        );
        assert_eq!(
            new,
            base64::engine::general_purpose::STANDARD.encode(b"new image")
        );
    }

    #[test]
    fn reports_oversized_git_blobs_and_unsafe_worktree_paths() {
        let repo = TestRepo::new();
        repo.write("large.png", &vec![0; MAX_IMAGE_PREVIEW_SIZE + 1]);
        let oid = repo.git(&["hash-object", "-w", "large.png"]);
        let large = ImagePreviewTarget::Change {
            file: FileChange {
                path: "large.png".into(),
                old_path: None,
                new_path: "large.png".into(),
                status: FileChangeStatus::Added,
                old_blob: None,
                new_blob: Some(oid),
                added: None,
                deleted: None,
            },
        };
        let result = load(&GitRepo::local(repo.path.to_string_lossy()), &large);
        assert!(matches!(
            result.new,
            ImagePreviewSide::Unavailable {
                reason: ImagePreviewUnavailable::TooLarge
            }
        ));

        let unsafe_path = ImagePreviewTarget::WorkingTree {
            diff_kind: WorkingTreeDiffKind::Unstaged,
            file: FileStatus {
                index: '?',
                worktree: '?',
                path: "../outside.png".into(),
                old_path: None,
            },
        };
        let result = load(&GitRepo::local(repo.path.to_string_lossy()), &unsafe_path);
        assert!(matches!(
            result.new,
            ImagePreviewSide::Unavailable {
                reason: ImagePreviewUnavailable::Unreadable
            }
        ));
    }

    struct TestRepo {
        path: PathBuf,
    }

    impl TestRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!(
                "augur-image-preview-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create image preview repository");
            let repo = Self { path };
            repo.git(&["init", "-q"]);
            repo.git(&["config", "user.email", "test@example.com"]);
            repo.git(&["config", "user.name", "augur-git test"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }

        fn write(&self, path: &str, contents: &[u8]) {
            fs::write(self.path.join(path), contents).expect("write image fixture");
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.path)
                .args(args)
                .output()
                .expect("run git fixture command");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
