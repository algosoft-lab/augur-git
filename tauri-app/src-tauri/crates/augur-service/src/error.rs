//! The error type every service command returns.
//!
//! The frontend localizes `key` and shows `detail`; the same structure is
//! returned by the HTTP API so both frontends render errors identically.

use serde::Serialize;

use augur_core::git::GitError;

/// Why a command was refused. The webview localizes `key` and shows `detail`.
#[derive(Clone, Debug, Serialize)]
pub struct CommandError {
    pub key: String,
    pub detail: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.key, self.detail)
    }
}

impl CommandError {
    pub fn new(key: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// The repository session is gone, usually because its tab closed.
    pub fn missing_repo(repo_id: u64) -> Self {
        Self::new(
            "err-repo-closed",
            format!("repository {repo_id} is no longer open"),
        )
    }
}

impl From<GitError> for CommandError {
    fn from(error: GitError) -> Self {
        Self {
            key: error.key.to_string(),
            detail: error.detail,
        }
    }
}

/// A bare detail string is Git's own explanation of a failed run; the key has
/// no placeholder, so the detail is the only useful part of the message.
impl From<String> for CommandError {
    fn from(detail: String) -> Self {
        Self::new("err-git-run", detail)
    }
}

/// The result type every service command returns.
pub type Result<T> = std::result::Result<T, CommandError>;
