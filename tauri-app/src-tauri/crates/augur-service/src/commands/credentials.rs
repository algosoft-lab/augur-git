//! HTTP/HTTPS Git credential commands.
//!
//! The vault owns the document and the secrets; these bodies validate what the
//! interface sends, run the one operation that leaves the process (a credential
//! test), and return the refreshed snapshot so a page never renders a state the
//! backend does not have.

use std::process::Stdio;
use std::time::{Duration, Instant};

use augur_core::git::GitRepo;
use serde::Serialize;

use crate::askpass::{CredentialLoginAnswer, CredentialLoginResult};
use crate::commands::repo::run_blocking_typed;
use crate::credentials::model::{
    CredentialInput, CredentialSettingsPatch, CredentialSnapshot, CredentialTest, ImportReport,
};
use crate::credentials::model::SecretStorage;
use crate::credentials::prompt::{RemoteTarget, parse_remote_url};
use crate::credentials::vault::now_seconds;
use crate::error::{CommandError, Result};
use crate::state::AppState;

/// How long a credential test may run before it is killed.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What the settings page loads on open.
pub fn snapshot(state: &AppState) -> CredentialSnapshot {
    state.credential_snapshot()
}

pub fn update_settings(
    state: &AppState,
    patch: CredentialSettingsPatch,
) -> Result<CredentialSnapshot> {
    state.update_credential_settings(patch)
}

pub fn save_entry(state: &AppState, input: CredentialInput) -> Result<CredentialSnapshot> {
    state
        .credentials()
        .save(input, now_seconds())
        .map_err(|detail| CommandError::new("err-credential-save", detail))
}

pub fn delete_entry(state: &AppState, id: String) -> Result<CredentialSnapshot> {
    state
        .credentials()
        .delete(&id)
        .map_err(|detail| CommandError::new("err-credential-delete", detail))
}

/// Answer the dialog's username/password form.
pub fn login_response(
    state: &AppState,
    request_id: String,
    answer: CredentialLoginAnswer,
) -> CredentialLoginResult {
    crate::askpass::respond_login(&request_id, answer, &state.credentials())
}

/// What the interface may say about a remote URL before using it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialLookup {
    /// Whether an entry would answer this remote.
    pub saved: bool,
    pub username: Option<String>,
    pub storage: Option<SecretStorage>,
}

/// Look up the account saved for a remote URL, without reading a secret.
///
/// The remote and clone dialogs use it to show that an address already has an
/// account, so the first fetch does not come as a surprise.
pub fn lookup(state: &AppState, url: String) -> CredentialLookup {
    let Some(parsed) = parse_remote_url(&url) else {
        return CredentialLookup {
            saved: false,
            username: None,
            storage: None,
        };
    };
    let target = RemoteTarget {
        protocol: parsed.protocol,
        host: parsed.host,
        path: parsed.path,
        username: parsed.username,
        repository: None,
    };
    match state.credentials().matching_entry(&target) {
        Some(entry) => CredentialLookup {
            saved: true,
            username: Some(entry.username),
            storage: Some(entry.storage),
        },
        None => CredentialLookup {
            saved: false,
            username: None,
            storage: None,
        },
    }
}

/// Result of importing Git's own plaintext credential file.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    #[serde(flatten)]
    pub report: ImportReport,
    /// Entries that could not be imported, with the reason.
    pub warnings: Vec<String>,
    pub snapshot: CredentialSnapshot,
}

/// Import `~/.git-credentials`, the file `git config credential.helper store`
/// maintains. The file is read, never modified or deleted.
pub fn import_git_store(state: &AppState) -> Result<ImportOutcome> {
    let path = git_store_path().ok_or_else(|| {
        CommandError::new(
            "err-credential-import",
            "The home directory is unknown, so there is no credential file to read",
        )
    })?;
    let text = std::fs::read_to_string(&path).map_err(|error| {
        CommandError::new(
            "err-credential-import",
            format!("{}: {error}", path.to_string_lossy()),
        )
    })?;
    let (mut report, warnings) = state
        .credentials()
        .import_git_credentials(&text, now_seconds());
    report.source = path.to_string_lossy().into_owned();
    Ok(ImportOutcome {
        report,
        warnings,
        snapshot: state.credential_snapshot(),
    })
}

fn git_store_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)?;
    Some(home.join(".git-credentials"))
}

/// Ask the remote whether the stored credential is accepted.
///
/// The test runs a real `git ls-remote` through the same askpass bridge the
/// application uses, so the answer is what Git itself would experience. Only
/// an entry that names a repository path can be tested: a host-wide entry has
/// no address to fetch.
pub async fn test_entry(state: &AppState, id: String) -> Result<CredentialTest> {
    let vault = state.credentials();
    let Some(entry) = vault.entry(&id) else {
        return Err(CommandError::new(
            "err-credential-missing",
            "That credential is no longer stored",
        ));
    };
    let Some(prefix) = entry.path_prefix.clone() else {
        return Ok(CredentialTest {
            ok: false,
            key: "credential-test-no-path".into(),
            detail: String::new(),
        });
    };
    if !vault.secret_present(&entry) {
        return Ok(CredentialTest {
            ok: false,
            key: "credential-test-no-secret".into(),
            detail: String::new(),
        });
    }
    let target = RemoteTarget {
        protocol: entry.protocol.clone(),
        host: entry.host.clone(),
        path: Some(prefix),
        username: Some(entry.username.clone()),
        repository: None,
    };
    let url = target.url();
    let outcome = run_blocking_typed(move || {
        let repo = GitRepo::local(".");
        let mut command = repo.command();
        command
            .args(["ls-remote", "--heads", &url])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok::<_, CommandError>(run_with_deadline(command, TEST_TIMEOUT))
    })
    .await?;
    match outcome {
        Ok(output) => {
            vault.mark_used(&id, now_seconds());
            Ok(CredentialTest {
                ok: true,
                key: "credential-test-ok".into(),
                detail: first_lines(&output),
            })
        }
        Err(detail) => {
            vault.mark_failed(&id, now_seconds());
            Ok(CredentialTest {
                ok: false,
                key: "credential-test-failed".into(),
                detail,
            })
        }
    }
}

/// Run a command, kill it at the deadline, and keep the interesting output.
fn run_with_deadline(
    mut command: std::process::Command,
    timeout: Duration,
) -> std::result::Result<String, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("git could not be started: {error}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| error.to_string())?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                if status.success() {
                    return Ok(first_lines(&stdout));
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(first_lines(if stderr.trim().is_empty() {
                    &stdout
                } else {
                    &stderr
                }));
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                return Err("The remote did not answer within 30 seconds".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// First few non-empty lines of Git's output, which is all a notice can show.
fn first_lines(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lookup_parses_only_http_addresses() {
        // Pure parsing is covered in the prompt module; this guards the shape
        // the dialogs rely on for an address they cannot match.
        assert!(parse_remote_url("ssh://git@example.com/repo.git").is_none());
    }

    #[test]
    fn only_the_first_lines_of_git_output_are_kept() {
        assert_eq!(first_lines("\n\na\nb\nc\nd\n"), "a\nb\nc");
        assert_eq!(first_lines("   \n\n"), "");
    }

    #[test]
    fn the_git_store_path_lives_in_the_home_directory() {
        // The variable is present on every supported platform in the test
        // environment, but the function must not panic when it is not.
        let path = git_store_path();
        if let Some(path) = path {
            assert!(path.ends_with(".git-credentials"));
        }
    }
}
