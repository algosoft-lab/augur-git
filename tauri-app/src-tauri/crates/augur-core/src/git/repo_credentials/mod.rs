//! Per-repository `.git/config` credential & identity configuration and diagnostics.
//!
//! This module reads, writes, and verifies repository-local Git configuration
//! (`.git/config`), supporting both:
//! - Strategy 1 (`use-http-path`): `credential.useHttpPath = true` + `credential.username`
//!   (and optional remote URL username embedding),
//! - Strategy 2 (`isolated-store` / `isolated-prompt`): resetting inherited
//!   helpers with local `credential.helper = ""` plus an optional dedicated
//!   `store --file=...` helper,
//! as well as repository-local `user.name` / `user.email` and
//! `git config --show-origin` / `git credential fill` verification.
//!
//! All commands invoke `repo.raw_config_command()` so Augur Git's ephemeral
//! `GIT_ASKPASS` / `GIT_CONFIG_*` bridges never skew on-disk diagnostics.

mod parse;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub use parse::*;
use parse::parse_http_url_parts;

use super::config_access::{GitConfigEntry, validate_entry};
use super::location::GitRepo;
use super::{GitError, RemoteEntry};

const FILL_TIMEOUT: Duration = Duration::from_secs(8);

/// One `credential.<url>.helper` or `credential.helper` entry observed across
/// Git configuration scopes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialHelperOrigin {
    pub scope: String,
    pub origin: String,
    pub key: String,
    pub value: String,
}

/// Strategy detected in or applied to `.git/config`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepoCredentialMode {
    Inherit,
    UseHttpPath,
    IsolatedStore,
    IsolatedPrompt,
    Custom,
}

/// How saving local config adjusts an HTTP(S) remote URL's embedded username.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteUsernameAction {
    #[default]
    Keep,
    Embed,
    Strip,
}

/// Snapshot of a repository's local `.git/config` credential & identity state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoLocalCredentialConfig {
    pub mode: RepoCredentialMode,
    pub use_http_path: Option<bool>,
    pub username: Option<String>,
    pub local_helpers: Vec<String>,
    pub store_file: Option<String>,
    pub suggested_store_file: String,
    pub effective_helpers: Vec<CredentialHelperOrigin>,
    pub local_user_name: Option<String>,
    pub local_user_email: Option<String>,
    pub effective_user_name: Option<String>,
    pub effective_user_email: Option<String>,
}

/// Desired local `.git/config` credential & identity state sent by the interface.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoLocalCredentialInput {
    pub mode: RepoCredentialMode,
    pub use_http_path: bool,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub store_file: Option<String>,
    #[serde(default)]
    pub store_password: Option<String>,
    #[serde(default)]
    pub target_remote_name: Option<String>,
    #[serde(default)]
    pub target_remote_url: Option<String>,
    #[serde(default)]
    pub remote_url_username_action: RemoteUsernameAction,
    #[serde(default)]
    pub local_user_name: Option<String>,
    #[serde(default)]
    pub local_user_email: Option<String>,
}

/// Outcome of running `git credential fill` non-interactively in the repo.
///
/// Passwords are never included in this payload: only whether a non-empty
/// password was returned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCredentialFillResult {
    pub ok: bool,
    pub username: Option<String>,
    pub password_present: bool,
    pub detail: String,
}

/// List raw remotes (`remote.<name>.url` without `insteadOf` rewriting).
pub fn list_raw_remotes(repo: &GitRepo) -> Vec<RemoteEntry> {
    let Ok(output) = repo
        .raw_config_command()
        .args([
            "-C",
            repo.path(),
            "config",
            "--null",
            "--get-regexp",
            r"^remote\..*\.(url|pushurl)$",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    super::remote_config::parse(&text)
}

/// Read the repository's local credential configuration and inherited helpers.
pub fn read_repo_local_config(repo: &GitRepo) -> Result<RepoLocalCredentialConfig, GitError> {
    let use_http_path = read_local_bool(repo, "credential.useHttpPath");
    let username = read_local_one(repo, "credential.username");
    let local_helpers = read_local_all(repo, "credential.helper");
    let (mode, store_file) =
        classify_mode(use_http_path, username.as_deref(), &local_helpers);
    let effective_helpers = read_effective_helpers(repo);
    let local_user_name = read_local_one(repo, "user.name");
    let local_user_email = read_local_one(repo, "user.email");
    let effective_user_name = read_effective_one(repo, "user.name");
    let effective_user_email = read_effective_one(repo, "user.email");

    Ok(RepoLocalCredentialConfig {
        mode,
        use_http_path,
        username,
        local_helpers,
        store_file,
        suggested_store_file: suggested_store_file_path(repo),
        effective_helpers,
        local_user_name,
        local_user_email,
        effective_user_name,
        effective_user_email,
    })
}

/// Validate and apply local `.git/config` credential & identity settings.
///
/// Returns `Ok(true)` when a remote URL was modified so callers can trigger a
/// repository refs refresh.
pub fn apply_repo_local_config(
    repo: &GitRepo,
    input: &RepoLocalCredentialInput,
) -> Result<bool, GitError> {
    let username = clean_opt(input.username.as_deref());
    let store_file = clean_opt(input.store_file.as_deref());
    let store_password = input
        .store_password
        .as_deref()
        .filter(|value| !value.is_empty());
    let local_user_name = clean_opt(input.local_user_name.as_deref());
    let local_user_email = clean_opt(input.local_user_email.as_deref());

    reject_control_chars("credential.username", username.as_deref())?;
    reject_control_chars("credential.helper", store_file.as_deref())?;
    reject_control_chars("password", store_password)?;
    validate_entry(&GitConfigEntry {
        key: "user.name".into(),
        value: local_user_name.clone(),
    })
    .map_err(|detail| GitError::new("err-git-config-value", detail))?;
    validate_entry(&GitConfigEntry {
        key: "user.email".into(),
        value: local_user_email.clone(),
    })
    .map_err(|detail| GitError::new("err-git-config-value", detail))?;

    let expanded_store_file = if input.mode == RepoCredentialMode::IsolatedStore {
        let Some(raw_path) = store_file.as_deref() else {
            return Err(GitError::new(
                "err-git-config-value",
                "A credential store file path is required",
            ));
        };
        Some(expand_store_path(raw_path, repo))
    } else {
        None
    };

    if store_password.is_some() && input.mode == RepoCredentialMode::IsolatedStore {
        if username.is_none() {
            return Err(GitError::new(
                "err-git-config-value",
                "A username is required when saving a password into the store file",
            ));
        }
        let Some(url) = input.target_remote_url.as_deref() else {
            return Err(GitError::new(
                "err-git-config-value",
                "An HTTP(S) remote URL is required when saving a password into the store file",
            ));
        };
        if parse_http_url_parts(url).is_none() {
            return Err(GitError::new(
                "err-git-config-value",
                "Only HTTP/HTTPS remote URLs can be saved into the credential store file",
            ));
        }
    }

    let remote_update = if input.remote_url_username_action != RemoteUsernameAction::Keep {
        let remote_name = clean_opt(input.target_remote_name.as_deref()).ok_or_else(|| {
            GitError::new("err-git-config-value", "Select a remote before updating its URL")
        })?;
        let remotes = list_raw_remotes(repo);
        let remote = remotes
            .into_iter()
            .find(|entry| entry.name == remote_name)
            .ok_or_else(|| {
                GitError::new(
                    "err-git-config-value",
                    format!("Remote '{remote_name}' no longer exists"),
                )
            })?;
        let next_url = rewrite_http_remote_url_username(
            &remote.url,
            input.remote_url_username_action,
            username.as_deref(),
        )
        .map_err(|detail| GitError::new("err-git-config-value", detail))?;
        next_url.map(|url| (remote_name, url))
    } else {
        None
    };

    match input.mode {
        RepoCredentialMode::Inherit => {
            unset_local_all(repo, "credential.helper")?;
            unset_local_all(repo, "credential.useHttpPath")?;
            unset_local_all(repo, "credential.username")?;
        }
        RepoCredentialMode::UseHttpPath => {
            unset_local_all(repo, "credential.helper")?;
            set_local(repo, "credential.useHttpPath", "true")?;
            set_or_unset_local(repo, "credential.username", username.as_deref())?;
        }
        RepoCredentialMode::IsolatedStore => {
            let store_path = expanded_store_file
                .as_deref()
                .expect("validated above for IsolatedStore");
            replace_local_all(repo, "credential.helper", "")?;
            add_local(repo, "credential.helper", &format_store_helper_value(store_path))?;
            if input.use_http_path {
                set_local(repo, "credential.useHttpPath", "true")?;
            } else {
                unset_local_all(repo, "credential.useHttpPath")?;
            }
            set_or_unset_local(repo, "credential.username", username.as_deref())?;

            if let (Some(password), Some(user), Some(url)) = (
                store_password,
                username.as_deref(),
                input.target_remote_url.as_deref(),
            ) {
                write_store_file_secret(repo, store_path, url, user, password, input.use_http_path)?;
            }
        }
        RepoCredentialMode::IsolatedPrompt => {
            replace_local_all(repo, "credential.helper", "")?;
            if input.use_http_path {
                set_local(repo, "credential.useHttpPath", "true")?;
            } else {
                unset_local_all(repo, "credential.useHttpPath")?;
            }
            set_or_unset_local(repo, "credential.username", username.as_deref())?;
        }
        RepoCredentialMode::Custom => {
            if input.use_http_path {
                set_local(repo, "credential.useHttpPath", "true")?;
            } else {
                unset_local_all(repo, "credential.useHttpPath")?;
            }
            set_or_unset_local(repo, "credential.username", username.as_deref())?;
        }
    }

    set_or_unset_local(repo, "user.name", local_user_name.as_deref())?;
    set_or_unset_local(repo, "user.email", local_user_email.as_deref())?;

    let mut remote_changed = false;
    if let Some((remote_name, new_url)) = remote_update {
        run_repo_git(repo, &["remote", "set-url", &remote_name, &new_url])?;
        remote_changed = true;
    }

    Ok(remote_changed)
}

/// Run `git credential fill` non-interactively against the repository's real
/// Git configuration (without Augur's askpass bridge) to verify what native Git
/// helpers resolve.
pub fn verify_git_credential_fill(
    repo: &GitRepo,
    protocol: &str,
    host: &str,
    path: Option<&str>,
    username: Option<&str>,
) -> GitCredentialFillResult {
    let mut payload = format!("protocol={protocol}\nhost={host}\n");
    if let Some(path) = path.filter(|value| !value.is_empty()) {
        payload.push_str(&format!("path={path}\n"));
    }
    if let Some(username) = username.filter(|value| !value.is_empty()) {
        payload.push_str(&format!("username={username}\n"));
    }
    payload.push('\n');

    let mut command = repo.raw_config_command();
    command
        .args(["-C", repo.path(), "credential", "fill"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("GIT_ASKPASS_REQUIRE", "never")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    match run_with_stdin_timeout(command, &payload, FILL_TIMEOUT) {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let (resolved_user, password_present) = parse_credential_fill_output(&stdout);
            if output.status.success() {
                GitCredentialFillResult {
                    ok: password_present || resolved_user.is_some(),
                    username: resolved_user,
                    password_present,
                    detail: stderr.trim().lines().next().unwrap_or_default().to_string(),
                }
            } else {
                GitCredentialFillResult {
                    ok: false,
                    username: resolved_user,
                    password_present,
                    detail: git_failure_message(&output),
                }
            }
        }
        Err(detail) => GitCredentialFillResult {
            ok: false,
            username: None,
            password_present: false,
            detail,
        },
    }
}

fn clean_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn reject_control_chars(label: &str, value: Option<&str>) -> Result<(), GitError> {
    if let Some(value) = value
        && value.chars().any(char::is_control)
    {
        return Err(GitError::new(
            "err-git-config-value",
            format!("{label} contains control characters"),
        ));
    }
    Ok(())
}

fn expand_store_path(raw: &str, repo: &GitRepo) -> String {
    let trimmed = raw.trim();
    if !repo.is_wsl()
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        if trimmed == "~" {
            return PathBuf::from(home).to_string_lossy().into_owned();
        }
        if let Some(rest) = trimmed
            .strip_prefix("~/")
            .or_else(|| trimmed.strip_prefix("~\\"))
        {
            return PathBuf::from(home).join(rest).to_string_lossy().into_owned();
        }
    }
    trimmed.to_string()
}

fn write_store_file_secret(
    repo: &GitRepo,
    store_path: &str,
    remote_url: &str,
    username: &str,
    password: &str,
    include_path: bool,
) -> Result<(), GitError> {
    let Some((protocol, host, path)) = parse_http_url_parts(remote_url) else {
        return Err(GitError::new(
            "err-git-config-value",
            "Only HTTP/HTTPS remotes can be stored",
        ));
    };
    if !repo.is_wsl()
        && let Some(parent) = Path::new(store_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            GitError::new(
                "err-git-config-write",
                format!("Could not create store directory: {error}"),
            )
        })?;
    }

    let mut input = format!("protocol={protocol}\nhost={host}\n");
    if include_path && let Some(path) = path {
        input.push_str(&format!("path={path}\n"));
    }
    input.push_str(&format!("username={username}\npassword={password}\n\n"));

    let mut command = repo.raw_config_command();
    command
        .args(["-C", repo.path(), "credential-store", "--file", store_path, "store"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_with_stdin_timeout(command, &input, FILL_TIMEOUT)
        .map_err(|detail| GitError::new("err-git-config-write", detail))?;
    if !output.status.success() {
        return Err(GitError::new(
            "err-git-config-write",
            git_failure_message(&output),
        ));
    }
    #[cfg(unix)]
    if !repo.is_wsl() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(store_path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn read_local_one(repo: &GitRepo, key: &str) -> Option<String> {
    let output = repo
        .raw_config_command()
        .args(["-C", repo.path(), "config", "--local", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn read_local_bool(repo: &GitRepo, key: &str) -> Option<bool> {
    let output = repo
        .raw_config_command()
        .args(["-C", repo.path(), "config", "--local", "--bool", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn read_local_all(repo: &GitRepo, key: &str) -> Vec<String> {
    let Ok(output) = repo
        .raw_config_command()
        .args(["-C", repo.path(), "config", "--local", "--null", "--get-all", key])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut values: Vec<&str> = text.split('\0').collect();
    if values.last() == Some(&"") {
        values.pop();
    }
    values.into_iter().map(str::to_string).collect()
}

fn read_effective_one(repo: &GitRepo, key: &str) -> Option<String> {
    let output = repo
        .raw_config_command()
        .args(["-C", repo.path(), "config", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn read_effective_helpers(repo: &GitRepo) -> Vec<CredentialHelperOrigin> {
    if let Ok(output) = repo
        .raw_config_command()
        .args([
            "-C",
            repo.path(),
            "config",
            "--show-origin",
            "--show-scope",
            "--null",
            "--get-regexp",
            r"^credential(\..+)?\.helper$",
        ])
        .output()
    {
        if output.status.success() {
            return parse_origin_helpers(&String::from_utf8_lossy(&output.stdout), true);
        }
        // Exit 1 means no matching keys; 129 means older git lacks --show-scope.
        if output.status.code() != Some(129) {
            return Vec::new();
        }
    }
    let Ok(output) = repo
        .raw_config_command()
        .args([
            "-C",
            repo.path(),
            "config",
            "--show-origin",
            "--null",
            "--get-regexp",
            r"^credential(\..+)?\.helper$",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_origin_helpers(&String::from_utf8_lossy(&output.stdout), false)
}

fn set_local(repo: &GitRepo, key: &str, value: &str) -> Result<(), GitError> {
    run_repo_git(repo, &["config", "--local", key, value])
}

fn replace_local_all(repo: &GitRepo, key: &str, value: &str) -> Result<(), GitError> {
    run_repo_git(repo, &["config", "--local", "--replace-all", key, value])
}

fn add_local(repo: &GitRepo, key: &str, value: &str) -> Result<(), GitError> {
    run_repo_git(repo, &["config", "--local", "--add", key, value])
}

fn unset_local_all(repo: &GitRepo, key: &str) -> Result<(), GitError> {
    let output = repo
        .raw_config_command()
        .args(["-C", repo.path(), "config", "--local", "--unset-all", key])
        .output()
        .map_err(|error| GitError::new("err-git-run", format!("git config: {error}")))?;
    if output.status.success() || output.status.code() == Some(5) {
        return Ok(());
    }
    Err(GitError::new(
        "err-git-config-write",
        git_failure_message(&output),
    ))
}

fn set_or_unset_local(repo: &GitRepo, key: &str, value: Option<&str>) -> Result<(), GitError> {
    match value {
        Some(value) if !value.is_empty() => set_local(repo, key, value),
        _ => unset_local_all(repo, key),
    }
}

fn run_repo_git(repo: &GitRepo, args: &[&str]) -> Result<(), GitError> {
    let mut command = repo.raw_config_command();
    command.arg("-C").arg(repo.path()).args(args);
    let output = command
        .output()
        .map_err(|error| GitError::new("err-git-run", format!("git: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(GitError::new(
        "err-git-config-write",
        git_failure_message(&output),
    ))
}

fn run_with_stdin_timeout(
    mut command: Command,
    stdin_text: &str,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("git could not be started: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_text.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|error| error.to_string()),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                return Err("Git credential command timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn git_failure_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = stderr.trim();
    if !message.is_empty() {
        return message.to_string();
    }
    let message = stdout.trim();
    if !message.is_empty() {
        return message.to_string();
    }
    format!("git exited with {:?}", output.status.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_repo() -> (GitRepo, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "augur-repo-creds-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success());
        (GitRepo::local(dir.to_string_lossy().into_owned()), dir)
    }

    #[test]
    fn apply_and_read_local_strategies_round_trip_with_real_git() {
        let (repo, dir) = temp_repo();
        Command::new("git")
            .args([
                "-C",
                repo.path(),
                "remote",
                "add",
                "origin",
                "https://example.invalid/org/repo.git",
            ])
            .status()
            .unwrap();

        let store_path = dir.join("creds with spaces.txt");
        let changed = apply_repo_local_config(
            &repo,
            &RepoLocalCredentialInput {
                mode: RepoCredentialMode::IsolatedStore,
                use_http_path: true,
                username: Some("special_user".into()),
                store_file: Some(store_path.to_string_lossy().into_owned()),
                store_password: Some("s3cret:pw".into()),
                target_remote_name: Some("origin".into()),
                target_remote_url: Some("https://example.invalid/org/repo.git".into()),
                remote_url_username_action: RemoteUsernameAction::Embed,
                local_user_name: Some("Work Bot".into()),
                local_user_email: Some("bot@work.invalid".into()),
            },
        )
        .unwrap();
        assert!(changed);

        let snapshot = read_repo_local_config(&repo).unwrap();
        assert_eq!(snapshot.mode, RepoCredentialMode::IsolatedStore);
        assert_eq!(snapshot.use_http_path, Some(true));
        assert_eq!(snapshot.username.as_deref(), Some("special_user"));
        assert_eq!(snapshot.store_file, Some(store_path.to_string_lossy().into_owned()));
        assert_eq!(snapshot.local_user_name.as_deref(), Some("Work Bot"));
        assert_eq!(snapshot.local_user_email.as_deref(), Some("bot@work.invalid"));
        assert_eq!(snapshot.local_helpers.first().map(String::as_str), Some(""));

        let remotes = list_raw_remotes(&repo);
        assert_eq!(
            remotes[0].url,
            "https://special_user@example.invalid/org/repo.git"
        );

        let fill = verify_git_credential_fill(
            &repo,
            "https",
            "example.invalid",
            Some("org/repo.git"),
            Some("special_user"),
        );
        assert!(fill.ok, "{fill:?}");
        assert_eq!(fill.username.as_deref(), Some("special_user"));
        assert!(fill.password_present);

        // Reset back to inherit + strip remote username.
        let changed = apply_repo_local_config(
            &repo,
            &RepoLocalCredentialInput {
                mode: RepoCredentialMode::Inherit,
                use_http_path: false,
                username: None,
                store_file: None,
                store_password: None,
                target_remote_name: Some("origin".into()),
                target_remote_url: None,
                remote_url_username_action: RemoteUsernameAction::Strip,
                local_user_name: None,
                local_user_email: None,
            },
        )
        .unwrap();
        assert!(changed);
        let reset_snapshot = read_repo_local_config(&repo).unwrap();
        assert_eq!(reset_snapshot.mode, RepoCredentialMode::Inherit);
        assert_eq!(reset_snapshot.local_helpers, Vec::<String>::new());
        assert_eq!(list_raw_remotes(&repo)[0].url, "https://example.invalid/org/repo.git");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
