//! HTTP/HTTPS Git credential commands.
//!
//! The vault owns the document and the secrets; these bodies validate what the
//! interface sends, run the operations that inspect or test a repository's
//! credential configuration, and return refreshed snapshots so a page never
//! renders a state the backend does not have.

use std::process::Stdio;
use std::time::{Duration, Instant};

use augur_core::config::LocationConfig;
use augur_core::git::GitRepo;
use augur_core::git::repo_credentials::{
    self, CredentialHelperOrigin, GitCredentialFillResult, RepoCredentialMode,
    RepoLocalCredentialConfig, RepoLocalCredentialInput,
};
use serde::Serialize;

use crate::askpass::{CredentialLoginAnswer, CredentialLoginResult};
use crate::commands::repo::{repository, run_blocking, run_blocking_typed};
use crate::credentials::model::{
    CredentialEntryView, CredentialInput, CredentialScope, CredentialSettingsPatch,
    CredentialSnapshot, CredentialTest, ImportReport, RepositoryIdentity, SecretStorage,
};
use crate::credentials::prompt::{
    RemoteTarget, match_score, normalize_host, parse_remote_url, sanitize_remote_url,
};
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
    let snapshot = state
        .credentials()
        .save(input, now_seconds())
        .map_err(|detail| CommandError::new("err-credential-save", detail))?;
    state.sync_credential_runtime();
    Ok(snapshot)
}

pub fn delete_entry(state: &AppState, id: String) -> Result<CredentialSnapshot> {
    let snapshot = state
        .credentials()
        .delete(&id)
        .map_err(|detail| CommandError::new("err-credential-delete", detail))?;
    state.sync_credential_runtime();
    Ok(snapshot)
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
pub fn lookup(state: &AppState, url: String, repo_id: Option<u64>) -> CredentialLookup {
    let Some(parsed) = parse_remote_url(&url) else {
        return CredentialLookup {
            saved: false,
            username: None,
            storage: None,
        };
    };
    let repository_identity = repo_id.and_then(|id| {
        state.with_repo(id, |session| RepositoryIdentity {
            location: session.location().clone(),
            path: session.path().to_string(),
        })
    });
    let target = RemoteTarget {
        protocol: parsed.protocol,
        host: parsed.host,
        path: parsed.path,
        username: parsed.username,
        repository: repository_identity,
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
    state.sync_credential_runtime();
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
    let repo = match &entry.scope {
        CredentialScope::Repository { location, path } => location.to_repo(path.clone())?,
        CredentialScope::Global => GitRepo::local("."),
    };
    let target = RemoteTarget {
        protocol: entry.protocol.clone(),
        host: entry.host.clone(),
        path: Some(prefix.clone()),
        username: Some(entry.username.clone()),
        repository: None,
    };
    let url = target.url();
    let host_url = format!("{}://{}", entry.protocol, entry.host);
    let trimmed_prefix = prefix.trim_matches('/');
    let bare_prefix = trimmed_prefix.strip_suffix(".git").unwrap_or(trimmed_prefix);
    let mut extra_configs = vec![(format!("credential.{host_url}.useHttpPath"), "true".into())];
    for candidate in [
        format!("{host_url}/{bare_prefix}"),
        format!("{host_url}/{bare_prefix}.git"),
    ] {
        extra_configs.push((format!("credential.{candidate}.helper"), String::new()));
        extra_configs.push((
            format!("credential.{candidate}.username"),
            entry.username.clone(),
        ));
    }

    let outcome = run_blocking_typed(move || {
        let mut command = repo.command();
        command.env(
            "AUGUR_GIT_OP_ID",
            format!("probe-{}", uuid::Uuid::new_v4().simple()),
        );
        augur_core::git::location::append_git_configs(&mut command, &extra_configs);
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

/// Summary of one configured remote for the Repository Credentials dialog.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRemoteCredentialInfo {
    pub name: String,
    pub url: String,
    pub is_http: bool,
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub url_username: Option<String>,
    pub matched_vault_entry_id: Option<String>,
    pub matched_vault_scope: Option<String>,
    pub matched_vault_username: Option<String>,
    pub has_url_helper_conflict: bool,
}

/// Full per-repository credential state rendered by the modal dialog.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCredentialSnapshot {
    pub repo_id: u64,
    pub repo_path: String,
    pub location: LocationConfig,
    pub is_wsl: bool,
    pub remotes: Vec<RepoRemoteCredentialInfo>,
    pub vault_entries: Vec<CredentialEntryView>,
    pub fallback_vault_entries: Vec<CredentialEntryView>,
    pub local_config: RepoLocalCredentialConfig,
    pub vault: CredentialSnapshot,
}

/// Verification result for one remote inside a repository.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCredentialVerification {
    pub remote_name: String,
    pub remote_url: String,
    pub augur_source: String,
    pub augur_username: Option<String>,
    pub augur_secret_present: bool,
    pub augur_failing: bool,
    pub isolates_global_helpers: bool,
    pub git_fill: GitCredentialFillResult,
    pub url_username: Option<String>,
    pub warning_keys: Vec<String>,
}

/// Load repository remotes, repository-scoped vault entries, and `.git/config`
/// credential/identity settings.
pub async fn repo_snapshot(state: &AppState, repo_id: u64) -> Result<RepoCredentialSnapshot> {
    let (repo, repo_path, location) = state
        .with_repo(repo_id, |session| {
            (
                session.repo().clone(),
                session.path().to_string(),
                session.location().clone(),
            )
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;

    let is_wsl = repo.is_wsl();
    let repo_for_git = repo.clone();
    let (raw_remotes, local_config) = run_blocking_typed(move || {
        let remotes = repo_credentials::list_raw_remotes(&repo_for_git);
        let local = repo_credentials::read_repo_local_config(&repo_for_git)?;
        Ok::<_, CommandError>((remotes, local))
    })
    .await?;

    let vault = state.credentials();
    let vault_snapshot = vault.snapshot();
    let repo_identity = RepositoryIdentity {
        location: location.clone(),
        path: repo_path.clone(),
    };

    let mut http_targets: Vec<RemoteTarget> = Vec::new();
    let remotes: Vec<RepoRemoteCredentialInfo> = raw_remotes
        .into_iter()
        .map(|remote| {
            let safe_url = sanitize_remote_url(&remote.url);
            let Some(parsed) = parse_remote_url(&remote.url) else {
                return RepoRemoteCredentialInfo {
                    name: remote.name,
                    url: safe_url,
                    is_http: false,
                    protocol: None,
                    host: None,
                    path: None,
                    url_username: None,
                    matched_vault_entry_id: None,
                    matched_vault_scope: None,
                    matched_vault_username: None,
                    has_url_helper_conflict: false,
                };
            };
            let target = RemoteTarget {
                protocol: parsed.protocol.clone(),
                host: parsed.host.clone(),
                path: parsed.path.clone(),
                username: parsed.username.clone(),
                repository: Some(repo_identity.clone()),
            };
            http_targets.push(RemoteTarget {
                username: None,
                ..target.clone()
            });
            let matched = if !is_wsl {
                vault.matching_entry(&target)
            } else {
                None
            };
            let conflict = has_url_helper_conflict(
                &local_config.effective_helpers,
                &parsed.protocol,
                &parsed.host,
                parsed.path.as_deref(),
            );
            RepoRemoteCredentialInfo {
                name: remote.name,
                url: safe_url,
                is_http: true,
                protocol: Some(parsed.protocol),
                host: Some(parsed.host),
                path: parsed.path,
                url_username: parsed.username,
                matched_vault_entry_id: matched.as_ref().map(|entry| entry.id.clone()),
                matched_vault_scope: matched.as_ref().map(|entry| {
                    if entry.scope.is_global() {
                        "global".into()
                    } else {
                        "repository".into()
                    }
                }),
                matched_vault_username: matched.map(|entry| entry.username),
                has_url_helper_conflict: conflict,
            }
        })
        .collect();

    let vault_entries: Vec<CredentialEntryView> = vault_snapshot
        .entries
        .iter()
        .filter(|view| match &view.entry.scope {
            CredentialScope::Repository {
                location: entry_location,
                path: entry_path,
            } => *entry_location == location && *entry_path == repo_path,
            CredentialScope::Global => false,
        })
        .cloned()
        .collect();

    let fallback_vault_entries: Vec<CredentialEntryView> = vault_snapshot
        .entries
        .iter()
        .filter(|view| {
            view.entry.scope.is_global()
                && http_targets
                    .iter()
                    .any(|target| match_score(&view.entry, target).is_some())
        })
        .cloned()
        .collect();

    Ok(RepoCredentialSnapshot {
        repo_id,
        repo_path,
        location,
        is_wsl,
        remotes,
        vault_entries,
        fallback_vault_entries,
        local_config,
        vault: vault_snapshot,
    })
}

/// Apply `.git/config` credential & identity changes for a repository and return
/// the refreshed snapshot.
pub async fn save_repo_config(
    state: &AppState,
    repo_id: u64,
    input: RepoLocalCredentialInput,
) -> Result<RepoCredentialSnapshot> {
    let repo = repository(state, repo_id)?;
    let remote_changed = run_blocking_typed(move || {
        repo_credentials::apply_repo_local_config(&repo, &input).map_err(CommandError::from)
    })
    .await?;
    if remote_changed {
        let _ = state.with_repo(repo_id, |session| session.refresh());
    }
    repo_snapshot(state, repo_id).await
}

/// Verify both Augur's vault resolution and native `git credential fill` for
/// one HTTP(S) remote of a repository.
pub async fn verify_repo_credential(
    state: &AppState,
    repo_id: u64,
    remote_name: String,
) -> Result<RepoCredentialVerification> {
    let (repo, repo_path, location) = state
        .with_repo(repo_id, |session| {
            (
                session.repo().clone(),
                session.path().to_string(),
                session.location().clone(),
            )
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;

    let is_wsl = repo.is_wsl();
    let repo_for_lookup = repo.clone();
    let remote_for_lookup = remote_name.clone();
    let (remote, local_config) = run_blocking_typed(move || {
        let remotes = repo_credentials::list_raw_remotes(&repo_for_lookup);
        let remote = remotes
            .into_iter()
            .find(|entry| entry.name == remote_for_lookup)
            .ok_or_else(|| {
                augur_core::git::GitError::new(
                    "err-remote-missing",
                    format!("Remote '{remote_for_lookup}' no longer exists"),
                )
            })?;
        let local = repo_credentials::read_repo_local_config(&repo_for_lookup)?;
        Ok::<_, CommandError>((remote, local))
    })
    .await?;

    let Some(parsed) = parse_remote_url(&remote.url) else {
        return Err(CommandError::new(
            "err-git-config-value",
            "Only HTTP/HTTPS remotes support credential verification",
        ));
    };

    let vault = state.credentials();
    let repo_identity = RepositoryIdentity {
        location,
        path: repo_path,
    };
    let target = RemoteTarget {
        protocol: parsed.protocol.clone(),
        host: parsed.host.clone(),
        path: parsed.path.clone(),
        username: parsed.username.clone(),
        repository: Some(repo_identity.clone()),
    };

    let mut warning_keys: Vec<String> = Vec::new();
    let (augur_source, augur_username, augur_secret_present, augur_failing) = if !is_wsl {
        if let Some(entry) = vault.matching_entry(&target) {
            let source = if entry.scope.is_global() {
                "global-vault"
            } else {
                "repository-vault"
            };
            let secret_present = vault.secret_present(&entry);
            let failing = entry
                .last_failure_at
                .is_some_and(|failed| failed > entry.updated_at);
            (
                source.to_string(),
                Some(entry.username),
                secret_present,
                failing,
            )
        } else {
            let target_no_user = RemoteTarget {
                username: None,
                ..target.clone()
            };
            if let Some(entry) = vault.matching_entry(&target_no_user)
                && !entry.scope.is_global()
                && parsed.username.as_deref() != Some(entry.username.as_str())
            {
                warning_keys.push("repo-creds-warn-url-username-mismatch".into());
            }
            ("none".to_string(), None, false, false)
        }
    } else {
        ("none".to_string(), None, false, false)
    };

    let protocol = parsed.protocol.clone();
    let host = parsed.host.clone();
    let path = parsed.path.clone();
    let url_username = parsed.username.clone();
    let git_fill = run_blocking(move || {
        Ok(repo_credentials::verify_git_credential_fill(
            &repo,
            &protocol,
            &host,
            path.as_deref(),
            url_username.as_deref(),
        ))
    })
    .await?;

    let local_isolates = matches!(
        local_config.mode,
        RepoCredentialMode::IsolatedStore | RepoCredentialMode::IsolatedPrompt
    ) || local_config.local_helpers.iter().any(|value| value.is_empty());
    let vault_isolates =
        augur_source == "repository-vault" && vault.auto_fill() && augur_secret_present && !is_wsl;
    let isolates_global_helpers = local_isolates || vault_isolates;

    if has_url_helper_conflict(
        &local_config.effective_helpers,
        &parsed.protocol,
        &parsed.host,
        parsed.path.as_deref(),
    ) && !isolates_global_helpers
    {
        warning_keys.push("repo-creds-warn-global-url-helper".into());
    }
    if augur_source == "none" && !git_fill.ok {
        warning_keys.push("repo-creds-warn-no-credentials".into());
    }

    Ok(RepoCredentialVerification {
        remote_name,
        remote_url: sanitize_remote_url(&remote.url),
        augur_source,
        augur_username,
        augur_secret_present,
        augur_failing,
        isolates_global_helpers,
        git_fill,
        url_username: parsed.username,
        warning_keys,
    })
}

/// Test live authentication against one configured remote using `git ls-remote`
/// in non-interactive probe mode.
pub async fn test_repo_remote(
    state: &AppState,
    repo_id: u64,
    remote_name: String,
) -> Result<CredentialTest> {
    let repo = repository(state, repo_id)?;
    let repo_for_check = repo.clone();
    let name_for_check = remote_name.clone();
    let remote = run_blocking_typed(move || {
        repo_credentials::list_raw_remotes(&repo_for_check)
            .into_iter()
            .find(|entry| entry.name == name_for_check)
            .ok_or_else(|| {
                CommandError::from(augur_core::git::GitError::new(
                    "err-remote-missing",
                    format!("Remote '{name_for_check}' no longer exists"),
                ))
            })
    })
    .await?;

    if parse_remote_url(&remote.url).is_none() {
        return Err(CommandError::new(
            "err-git-config-value",
            "Only HTTP/HTTPS remotes can be tested here",
        ));
    }

    let outcome = run_blocking_typed(move || {
        let mut command = repo.command();
        command
            .env(
                "AUGUR_GIT_OP_ID",
                format!("probe-{}", uuid::Uuid::new_v4().simple()),
            )
            .args(["-C", repo.path(), "ls-remote", "--heads", &remote_name])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok::<_, CommandError>(run_with_deadline(command, TEST_TIMEOUT))
    })
    .await?;

    match outcome {
        Ok(output) => Ok(CredentialTest {
            ok: true,
            key: "credential-test-ok".into(),
            detail: first_lines(&output),
        }),
        Err(detail) => Ok(CredentialTest {
            ok: false,
            key: "credential-test-failed".into(),
            detail,
        }),
    }
}

fn has_url_helper_conflict(
    helpers: &[CredentialHelperOrigin],
    protocol: &str,
    host: &str,
    path: Option<&str>,
) -> bool {
    helpers.iter().any(|entry| {
        if entry.scope == "local" || entry.value.is_empty() {
            return false;
        }
        let Some(pattern) = entry
            .key
            .strip_prefix("credential.")
            .and_then(|rest| rest.strip_suffix(".helper"))
        else {
            return false;
        };
        url_pattern_matches_remote(pattern, protocol, host, path)
    })
}

fn url_pattern_matches_remote(
    pattern: &str,
    protocol: &str,
    host: &str,
    path: Option<&str>,
) -> bool {
    let Some(parsed) = parse_remote_url(pattern) else {
        // Handle scheme-less or host-only pattern if ever written as credential.host.helper.
        return normalize_host(pattern).is_some_and(|p| p == host);
    };
    if !parsed.protocol.eq_ignore_ascii_case(protocol) || parsed.host != host {
        return false;
    }
    match parsed.path {
        None => true,
        Some(pattern_path) => {
            let Some(remote_path) = path else {
                return false;
            };
            let pat = pattern_path.trim_matches('/');
            let rem = remote_path.trim_matches('/');
            rem == pat || rem.starts_with(&format!("{pat}/"))
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

    #[test]
    fn url_helper_conflict_detects_global_url_patterns() {
        let helpers = vec![
            CredentialHelperOrigin {
                scope: "global".into(),
                origin: "file:/Users/dev/.gitconfig".into(),
                key: "credential.helper".into(),
                value: "osxkeychain".into(),
            },
            CredentialHelperOrigin {
                scope: "global".into(),
                origin: "file:/Users/dev/.gitconfig".into(),
                key: "credential.https://github.com.helper".into(),
                value: "!gh auth git-credential".into(),
            },
        ];
        assert!(has_url_helper_conflict(
            &helpers,
            "https",
            "github.com",
            Some("org/repo.git")
        ));
        assert!(!has_url_helper_conflict(
            &helpers,
            "https",
            "gitlab.com",
            Some("org/repo.git")
        ));
    }
}
