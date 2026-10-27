//! Server-side directory browsing for the web folder picker.
//!
//! A browser cannot hand the server a local path, so the picker walks the
//! server's filesystem through this endpoint. When the server was started
//! with `--repo-root`, every returned path stays inside that root.

use std::path::{Component, PathBuf};

use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::ServerContext;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseRequest {
    /// The directory to list. Empty means the browsing root.
    #[serde(default)]
    path: Option<String>,
}

/// List one directory of the server filesystem.
pub async fn browse(
    State(context): State<ServerContext>,
    Json(request): Json<BrowseRequest>,
) -> Response {
    let requested = request.path.as_deref().unwrap_or("");
    let root = context.repo_root.clone().unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/"))
        });

    let target = match requested {
        "" => root.clone(),
        relative => {
            // Only paths inside the browsing root are honored; an absolute
            // path outside it, or any `..` escape, is refused.
            let candidate = PathBuf::from(relative);
            let resolved = if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            };
            if !is_within(&root, &resolved) {
                return (axum::http::StatusCode::FORBIDDEN, "path is outside the browsing root")
                    .into_response();
            }
            resolved
        }
    };

    let entries = match std::fs::read_dir(&target) {
        Ok(entries) => {
            let mut listed: Vec<serde_json::Value> = entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let path = entry.path();
                    let is_dir = entry.file_type().ok()?.is_dir();
                    if !is_dir {
                        // The picker only needs directories.
                        return None;
                    }
                    // Hidden directories are noise in a picker.
                    if entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with('.'))
                    {
                        return None;
                    }
                    Some(json!({
                        "name": entry.file_name().to_string_lossy(),
                        "path": path.to_string_lossy(),
                    }))
                })
                .collect();
            listed.sort_by(|left, right| {
                left["name"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["name"].as_str().unwrap_or_default())
            });
            listed
        }
        Err(error) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "key": "err-browse",
                    "detail": error.to_string(),
                })),
            )
                .into_response();
        }
    };

    let parent = target
        .parent()
        .filter(|parent| is_within(&root, parent))
        .map(|parent| parent.to_string_lossy().into_owned());

    Json(json!({
        "path": target.to_string_lossy(),
        "root": root.to_string_lossy(),
        "parent": parent,
        "entries": entries,
    }))
    .into_response()
}

/// Whether `path` equals or lives under `root`, rejecting any `..` component
/// so a lexical prefix check stays safe.
fn is_within(root: &std::path::Path, path: &std::path::Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !clean.pop() {
                    return false;
                }
            }
            Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }
    clean.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment_rejects_lexical_escapes() {
        let root = std::path::Path::new("/srv/repos");
        assert!(is_within(root, std::path::Path::new("/srv/repos/a")));
        assert!(is_within(root, std::path::Path::new("/srv/repos")));
        assert!(is_within(root, std::path::Path::new("/srv/repos/a/../b")));
        assert!(!is_within(root, std::path::Path::new("/srv/repos/../etc")));
        assert!(!is_within(root, std::path::Path::new("/etc")));
        assert!(!is_within(root, std::path::Path::new("/srv/reposities")));
    }
}
