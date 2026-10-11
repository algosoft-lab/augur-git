//! Global Git configuration access behind a fixed allow-list.
//!
//! The interface never sends arbitrary configuration keys: it names one of the
//! entries this module knows, and every read or write becomes a fixed argument
//! vector. The allow-list is deliberately small — the identity the commit flow
//! needs plus the few settings a fresh install is asked about most — so each
//! value can carry its own validation rule.
//!
//! Values travel as `Option<String>`: `None` means "unset the key", which is
//! the only way to return a global entry to Git's built-in behavior.

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde::{Deserialize, Serialize};

use super::GitError;
use super::location::GitRepo;
use crate::config::LocationConfig;

/// Every configuration entry the interface can read or write.
///
/// Parse is case-insensitive because Git treats the section and variable names
/// of a key as case-insensitive itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitConfigKey {
    UserName,
    UserEmail,
    InitDefaultBranch,
    CoreAutocrlf,
    CoreEditor,
}

impl GitConfigKey {
    /// Every entry, in settings-page order.
    pub const ALL: [GitConfigKey; 5] = [
        GitConfigKey::UserName,
        GitConfigKey::UserEmail,
        GitConfigKey::InitDefaultBranch,
        GitConfigKey::CoreAutocrlf,
        GitConfigKey::CoreEditor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            GitConfigKey::UserName => "user.name",
            GitConfigKey::UserEmail => "user.email",
            GitConfigKey::InitDefaultBranch => "init.defaultBranch",
            GitConfigKey::CoreAutocrlf => "core.autocrlf",
            GitConfigKey::CoreEditor => "core.editor",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        let normalized = key.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str().to_ascii_lowercase() == normalized)
    }
}

/// Which file a configuration command addresses.
///
/// Production always uses [`ConfigTarget::Global`]; tests use
/// [`ConfigTarget::File`] so the same code path runs against a throwaway file
/// instead of the user's real configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigTarget {
    Global,
    File(PathBuf),
}

impl ConfigTarget {
    /// The scope flags that select this target, inserted after `config`.
    fn args(&self) -> Vec<String> {
        match self {
            ConfigTarget::Global => vec!["--global".to_string()],
            ConfigTarget::File(path) => {
                vec!["--file".to_string(), path.to_string_lossy().into_owned()]
            }
        }
    }
}

/// One configuration write requested by the interface.
///
/// `key` stays a string on the wire and is validated against the allow-list
/// before anything runs; `value: None` unsets the entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConfigEntry {
    pub key: String,
    pub value: Option<String>,
}

/// The global values of every allow-listed entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConfigSnapshot {
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub init_default_branch: Option<String>,
    pub core_autocrlf: Option<String>,
    pub core_editor: Option<String>,
}

/// The identity Git would use for a commit in one repository, resolved through
/// every configuration scope (repository-local, global, system).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitIdentityProbe {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl CommitIdentityProbe {
    pub fn is_complete(&self) -> bool {
        self.name.is_some() && self.email.is_some()
    }
}

/// Why an entry was rejected before Git could run.
pub fn validate_entry(entry: &GitConfigEntry) -> Result<GitConfigKey, String> {
    let key = GitConfigKey::parse(&entry.key)
        .ok_or_else(|| format!("{} is not a configurable key", entry.key))?;
    let Some(value) = entry.value.as_deref() else {
        return Ok(key);
    };
    let value = value.trim();
    if value.is_empty() {
        // An empty value is treated as "unset" by the caller; a whitespace-only
        // string normalizes to the same thing.
        return Ok(key);
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{} contains control characters", key.as_str()));
    }
    match key {
        GitConfigKey::UserName | GitConfigKey::CoreEditor => {}
        GitConfigKey::UserEmail => {
            if !value.contains('@') || value.contains(char::is_whitespace) {
                return Err(format!("{} is not a valid email address", key.as_str()));
            }
        }
        GitConfigKey::InitDefaultBranch => {
            // Git's real branch-name rules are stricter; this rejects only what
            // would obviously fail, keeping the interface forgiving.
            if value.contains(char::is_whitespace)
                || value.starts_with('-')
                || value.contains("..")
                || value.ends_with(".lock")
                || value.ends_with('/')
                || value.starts_with('/')
            {
                return Err(format!("{} is not a valid branch name", key.as_str()));
            }
        }
        GitConfigKey::CoreAutocrlf => {
            if !matches!(
                value.to_ascii_lowercase().as_str(),
                "true" | "false" | "input"
            ) {
                return Err(format!("{} accepts true, false, or input", key.as_str()));
            }
        }
    }
    Ok(key)
}

/// Build the command that reaches the target location's own git.
///
/// A location of `None` means the host git. WSL distros are reached through
/// their own git via `wsl.exe`, positioned at `/` because a global-scope
/// command needs no repository and `wsl.exe` without `--cd` would otherwise
/// start in a translated Windows path.
pub fn command_for_location(location: Option<&LocationConfig>) -> Result<Command, GitError> {
    match location {
        None | Some(LocationConfig::Local) => Ok(host_command()),
        Some(location) => Ok(location.to_repo("/")?.command()),
    }
}

fn host_command() -> Command {
    #[allow(unused_mut)] // mutated only behind `cfg(windows)`
    let mut command = Command::new("git");
    #[cfg(windows)]
    command.creation_flags(super::location::CREATE_NO_WINDOW);
    command
}

/// The argument vector for reading one entry from a target.
pub fn read_args(target: &ConfigTarget, key: GitConfigKey) -> Vec<String> {
    let mut args = vec!["config".to_string()];
    args.extend(target.args());
    args.push("--get".to_string());
    args.push(key.as_str().to_string());
    args
}

/// The argument vector for writing or unsetting one entry on a target.
pub fn write_args(target: &ConfigTarget, key: GitConfigKey, value: Option<&str>) -> Vec<String> {
    let mut args = vec!["config".to_string()];
    args.extend(target.args());
    match value {
        Some(value) => {
            args.push(key.as_str().to_string());
            args.push(value.to_string());
        }
        None => {
            args.push("--unset".to_string());
            args.push(key.as_str().to_string());
        }
    }
    args
}

/// Read every allow-listed entry from the global configuration.
pub fn read_global_all(location: Option<&LocationConfig>) -> GitConfigSnapshot {
    let mut snapshot = GitConfigSnapshot::default();
    for key in GitConfigKey::ALL {
        let value = read_global_one(location, key);
        snapshot.assign(key, value);
    }
    snapshot
}

/// Read one entry; `None` also covers a git that failed to run.
fn read_global_one(location: Option<&LocationConfig>, key: GitConfigKey) -> Option<String> {
    let mut command = command_for_location(location).ok()?;
    let output = command
        .args(read_args(&ConfigTarget::Global, key))
        .output()
        .ok()?;
    config_value(&output)
}

/// Interpret a `git config --get` result: exit 1 means the key is unset.
fn config_value(output: &Output) -> Option<String> {
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Write entries to the global configuration after validating all of them.
///
/// Nothing runs unless every entry validates, so a rejected write leaves the
/// configuration exactly as it was.
pub fn apply_global(
    location: Option<&LocationConfig>,
    entries: &[GitConfigEntry],
) -> Result<(), GitError> {
    let mut parsed: Vec<(GitConfigKey, Option<String>)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let key = validate_entry(entry)
            .map_err(|detail| GitError::new("err-git-config-value", detail))?;
        let value = entry
            .value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        parsed.push((key, value));
    }

    for (key, value) in parsed {
        let mut command = command_for_location(location)?;
        run_config(
            &mut command,
            &write_args(&ConfigTarget::Global, key, value.as_deref()),
        )?;
    }
    Ok(())
}

fn run_config(command: &mut Command, args: &[String]) -> Result<(), GitError> {
    let output = command
        .args(args)
        .output()
        .map_err(|error| GitError::new("err-git-run", format!("git config: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    // `git config --unset` exits 5 when the key is already unset, which is the
    // state the write asked for.
    if output.status.code() == Some(5) && args.contains(&"--unset".to_string()) {
        return Ok(());
    }
    let detail = git_failure_message(&output);
    Err(GitError::new("err-git-config-write", detail))
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

/// Resolve the identity Git would use for a commit in this repository.
pub fn probe_commit_identity(repo: &GitRepo) -> CommitIdentityProbe {
    CommitIdentityProbe {
        name: read_effective(repo, GitConfigKey::UserName),
        email: read_effective(repo, GitConfigKey::UserEmail),
    }
}

/// Read one key resolved through every scope: no scope flags at all, so the
/// repository-local value wins when one exists.
fn read_effective(repo: &GitRepo, key: GitConfigKey) -> Option<String> {
    let output = repo
        .command()
        .arg("-C")
        .arg(repo.path())
        .arg("config")
        .arg("--get")
        .arg(key.as_str())
        .output()
        .ok()?;
    config_value(&output)
}

impl GitConfigSnapshot {
    fn assign(&mut self, key: GitConfigKey, value: Option<String>) {
        match key {
            GitConfigKey::UserName => self.user_name = value,
            GitConfigKey::UserEmail => self.user_email = value,
            GitConfigKey::InitDefaultBranch => self.init_default_branch = value,
            GitConfigKey::CoreAutocrlf => self.core_autocrlf = value,
            GitConfigKey::CoreEditor => self.core_editor = value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_allow_listed_keys_case_insensitively() {
        assert_eq!(
            GitConfigKey::parse("user.name"),
            Some(GitConfigKey::UserName)
        );
        assert_eq!(
            GitConfigKey::parse("USER.EMAIL"),
            Some(GitConfigKey::UserEmail)
        );
        assert_eq!(
            GitConfigKey::parse("init.defaultBranch"),
            Some(GitConfigKey::InitDefaultBranch)
        );
    }

    #[test]
    fn parse_rejects_keys_outside_the_allow_list() {
        assert_eq!(GitConfigKey::parse("core.repositoryformatversion"), None);
        assert_eq!(GitConfigKey::parse("remote.origin.url"), None);
        assert_eq!(GitConfigKey::parse(""), None);
    }

    #[test]
    fn read_args_are_exact() {
        let target = ConfigTarget::Global;
        assert_eq!(
            read_args(&target, GitConfigKey::UserName),
            vec!["config", "--global", "--get", "user.name"]
        );
        let target = ConfigTarget::File(PathBuf::from("cfg.ini"));
        assert_eq!(
            read_args(&target, GitConfigKey::UserEmail),
            vec!["config", "--file", "cfg.ini", "--get", "user.email"]
        );
    }

    #[test]
    fn write_args_are_exact() {
        let target = ConfigTarget::Global;
        assert_eq!(
            write_args(&target, GitConfigKey::UserName, Some("Augur")),
            vec!["config", "--global", "user.name", "Augur"]
        );
        assert_eq!(
            write_args(&target, GitConfigKey::UserEmail, None),
            vec!["config", "--global", "--unset", "user.email"]
        );
        let target = ConfigTarget::File(PathBuf::from("cfg.ini"));
        assert_eq!(
            write_args(&target, GitConfigKey::CoreAutocrlf, Some("input")),
            vec!["config", "--file", "cfg.ini", "core.autocrlf", "input"]
        );
    }

    #[test]
    fn validation_rejects_unknown_keys() {
        let entry = GitConfigEntry {
            key: "core.repositoryformatversion".to_string(),
            value: Some("1".to_string()),
        };
        assert!(validate_entry(&entry).is_err());
    }

    #[test]
    fn validation_accepts_unset_and_whitespace_only_values() {
        let unset = GitConfigEntry {
            key: "user.name".to_string(),
            value: None,
        };
        assert_eq!(validate_entry(&unset), Ok(GitConfigKey::UserName));
        let blank = GitConfigEntry {
            key: "user.name".to_string(),
            value: Some("   ".to_string()),
        };
        assert_eq!(validate_entry(&blank), Ok(GitConfigKey::UserName));
    }

    #[test]
    fn validation_rejects_identity_values_git_would_refuse() {
        let empty = GitConfigEntry {
            key: "user.email".to_string(),
            value: Some("not-an-email".to_string()),
        };
        assert!(validate_entry(&empty).is_err());
        let spaced = GitConfigEntry {
            key: "user.email".to_string(),
            value: Some("a b@c.d".to_string()),
        };
        assert!(validate_entry(&spaced).is_err());
        let newline = GitConfigEntry {
            key: "user.name".to_string(),
            value: Some("Ada\nLovelace".to_string()),
        };
        assert!(validate_entry(&newline).is_err());
    }

    #[test]
    fn validation_accepts_reasonable_identity_values() {
        let name = GitConfigEntry {
            key: "user.name".to_string(),
            value: Some("Ada Lovelace".to_string()),
        };
        assert_eq!(validate_entry(&name), Ok(GitConfigKey::UserName));
        let email = GitConfigEntry {
            key: "user.email".to_string(),
            value: Some("ada@example.com".to_string()),
        };
        assert_eq!(validate_entry(&email), Ok(GitConfigKey::UserEmail));
    }

    #[test]
    fn validation_constrains_autocrlf_to_gits_choices() {
        for accepted in ["true", "false", "input", "TRUE"] {
            let entry = GitConfigEntry {
                key: "core.autocrlf".to_string(),
                value: Some(accepted.to_string()),
            };
            assert_eq!(validate_entry(&entry), Ok(GitConfigKey::CoreAutocrlf));
        }
        let rejected = GitConfigEntry {
            key: "core.autocrlf".to_string(),
            value: Some("sometimes".to_string()),
        };
        assert!(validate_entry(&rejected).is_err());
    }

    #[test]
    fn validation_constrains_default_branch_names() {
        for rejected in ["..", "-branch", "head.lock", "a b", "trailing/"] {
            let entry = GitConfigEntry {
                key: "init.defaultBranch".to_string(),
                value: Some(rejected.to_string()),
            };
            assert!(
                validate_entry(&entry).is_err(),
                "{rejected} must be rejected"
            );
        }
        for accepted in ["main", "master", "feature/one", "2.0"] {
            let entry = GitConfigEntry {
                key: "init.defaultBranch".to_string(),
                value: Some(accepted.to_string()),
            };
            assert_eq!(
                validate_entry(&entry),
                Ok(GitConfigKey::InitDefaultBranch),
                "{accepted} must be accepted"
            );
        }
    }

    #[test]
    fn exit_code_one_from_get_means_unset() {
        #[cfg(windows)]
        let mut command = Command::new("cmd");
        #[cfg(windows)]
        command.args(["/C", "exit 1"]);
        #[cfg(not(windows))]
        let mut command = Command::new("sh");
        #[cfg(not(windows))]
        command.args(["-c", "exit 1"]);
        let output = command.output().unwrap();
        assert_eq!(config_value(&output), None);
    }

    #[test]
    fn identity_probe_reports_whatever_resolves() {
        let probe = CommitIdentityProbe {
            name: Some("Augur".to_string()),
            email: None,
        };
        assert!(!probe.is_complete());
        let probe = CommitIdentityProbe {
            name: Some("Augur".to_string()),
            email: Some("augur@example.com".to_string()),
        };
        assert!(probe.is_complete());
    }
}
