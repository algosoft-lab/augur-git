//! External coding-agent profiles, task context, and safe launch arguments.
//!
//! This module deliberately contains no provider SDKs or agent logic. It only
//! prepares a structured PTY launch request for a user-installed CLI agent.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::{Deserialize, Serialize};

const TASK_FILE_ENV: &str = "AUGUR_GIT_TASK_FILE";
const TASK_DIRECTORY: &str = "agent-tasks";
const BOOTSTRAP_PROMPT: &str = "Read the complete Augur Git task from the file path in AUGUR_GIT_TASK_FILE, follow it, and keep this interactive session open for follow-up questions.";
const STALE_TASK_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Built-in providers supported by the first-party UI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuiltInAgent {
    #[serde(alias = "Codex")]
    Codex,
    #[serde(alias = "ClaudeCode")]
    ClaudeCode,
    #[serde(alias = "OpenCode")]
    OpenCode,
}

impl BuiltInAgent {
    pub const ALL: [Self; 3] = [Self::Codex, Self::ClaudeCode, Self::OpenCode];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::OpenCode => "opencode",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::OpenCode => "OpenCode",
        }
    }

    pub const fn executable(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
            Self::OpenCode => "opencode",
        }
    }

    pub fn prompt_mode(self) -> PromptMode {
        match self {
            Self::OpenCode => PromptMode::Flag("--prompt".to_string()),
            Self::Codex | Self::ClaudeCode => PromptMode::TrailingArgument,
        }
    }
}

/// Position used for the fixed bootstrap prompt in a profile's argv.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptMode {
    #[serde(alias = "TrailingArgument")]
    TrailingArgument,
    #[serde(alias = "Flag")]
    Flag(String),
}

impl Default for PromptMode {
    fn default() -> Self {
        Self::TrailingArgument
    }
}

/// A user-defined external agent command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustomAgentProfile {
    pub id: String,
    pub name: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub prompt_mode: PromptMode,
}

impl CustomAgentProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("profile id cannot be empty".to_string());
        }
        if self.id.chars().any(char::is_control) {
            return Err(
                "profile id cannot contain control characters".to_string()
            );
        }
        if self.name.trim().is_empty() {
            return Err("profile name cannot be empty".to_string());
        }
        if self.name.chars().any(char::is_control) {
            return Err(
                "profile name cannot contain control characters".to_string()
            );
        }
        if self.executable.as_os_str().is_empty() {
            return Err("profile executable cannot be empty".to_string());
        }
        if self
            .executable
            .to_string_lossy()
            .chars()
            .any(char::is_control)
        {
            return Err("profile executable cannot contain control characters"
                .to_string());
        }
        if self
            .args
            .iter()
            .chain(match &self.prompt_mode {
                PromptMode::TrailingArgument => None,
                PromptMode::Flag(flag) => Some(flag),
            })
            .any(|arg| arg.chars().any(char::is_control))
        {
            return Err("profile arguments cannot contain control characters"
                .to_string());
        }
        if let PromptMode::Flag(flag) = &self.prompt_mode {
            if flag.trim().is_empty() {
                return Err("prompt flag cannot be empty".to_string());
            }
        }
        Ok(())
    }
}

/// Persisted agent-related preferences. Missing fields deserialize to the
/// built-in defaults so older config files remain valid.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct AgentSettings {
    pub default_profile_id: Option<String>,
    pub executable_overrides: HashMap<BuiltInAgent, PathBuf>,
    pub custom_profiles: Vec<CustomAgentProfile>,
}

impl AgentSettings {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for profile in &self.custom_profiles {
            if let Err(error) = profile.validate() {
                errors.push(format!("{}: {error}", profile.id));
            }
            if BuiltInAgent::ALL
                .iter()
                .any(|agent| agent.id() == profile.id)
            {
                errors.push(format!(
                    "{}: id conflicts with a built-in profile",
                    profile.id
                ));
            }
            if !ids.insert(profile.id.clone()) {
                errors.push(format!("{}: duplicate profile id", profile.id));
            }
        }
        for (agent, path) in &self.executable_overrides {
            if path.as_os_str().is_empty()
                || path.to_string_lossy().chars().any(char::is_control)
            {
                errors.push(format!(
                    "{}: invalid executable override",
                    agent.id()
                ));
            }
        }
        if let Some(default_profile_id) = self.default_profile_id.as_deref()
            && self.profile(default_profile_id).is_none()
        {
            errors.push(format!(
                "{default_profile_id}: default profile is not valid"
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn default_profile_id(&self) -> String {
        self.default_profile_id
            .clone()
            .unwrap_or_else(|| BuiltInAgent::Codex.id().to_string())
    }

    pub fn valid_custom_profiles(
        &self,
    ) -> impl Iterator<Item = &CustomAgentProfile> {
        self.custom_profiles.iter().filter(|profile| {
            profile.validate().is_ok()
                && !BuiltInAgent::ALL
                    .iter()
                    .any(|agent| agent.id() == profile.id)
        })
    }

    pub fn profile(&self, id: &str) -> Option<ResolvedAgentProfile> {
        if let Some(agent) = BuiltInAgent::ALL
            .iter()
            .copied()
            .find(|agent| agent.id() == id)
        {
            let executable = self
                .executable_overrides
                .get(&agent)
                .cloned()
                .unwrap_or_else(|| PathBuf::from(agent.executable()));
            if executable.as_os_str().is_empty()
                || executable.to_string_lossy().chars().any(char::is_control)
            {
                return None;
            }
            return Some(ResolvedAgentProfile {
                id: agent.id().to_string(),
                name: agent.display_name().to_string(),
                executable,
                args: Vec::new(),
                prompt_mode: agent.prompt_mode(),
            });
        }

        self.custom_profiles
            .iter()
            .find(|profile| profile.id == id && profile.validate().is_ok())
            .map(|profile| ResolvedAgentProfile {
                id: profile.id.clone(),
                name: profile.name.clone(),
                executable: profile.executable.clone(),
                args: profile.args.clone(),
                prompt_mode: profile.prompt_mode.clone(),
            })
    }
}

/// A validated profile ready to be launched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAgentProfile {
    pub id: String,
    pub name: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub prompt_mode: PromptMode,
}

/// A structured command specification consumed by the terminal backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub task_file: PathBuf,
}

/// Lifecycle status surfaced by the session tab. The terminal intentionally
/// does not infer provider-specific semantic states.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentSessionState {
    Starting,
    Running { last_activity: Instant },
    Exited { code: Option<i32> },
    Failed { summary: String },
}

impl ResolvedAgentProfile {
    pub fn launch_spec(&self, task_file: PathBuf) -> AgentLaunchSpec {
        let mut args = self.args.clone();
        match &self.prompt_mode {
            PromptMode::TrailingArgument => {
                args.push(BOOTSTRAP_PROMPT.to_string())
            }
            PromptMode::Flag(flag) => {
                args.push(flag.clone());
                args.push(BOOTSTRAP_PROMPT.to_string());
            }
        }
        AgentLaunchSpec {
            executable: self.executable.clone(),
            args,
            task_file,
        }
    }
}

/// Context references captured from Augur's review panels.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewContext {
    pub branch: String,
    #[serde(default)]
    pub selection: ReviewSelection,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReviewSelection {
    #[default]
    None,
    WorkingTreeFile {
        staged: bool,
        path: String,
    },
    Commit {
        oid: String,
    },
    CommitFile {
        oid: String,
        path: String,
    },
    Comparison {
        base: String,
        target: String,
        path: Option<String>,
    },
}

/// Build the small, reference-only task document passed to an external agent.
pub fn task_document(request: &str, context: &ReviewContext) -> String {
    let mut document = String::new();
    document.push_str("# Augur Git task\n\n");
    document.push_str("## User request\n\n");
    document.push_str(request.trim());
    document.push_str("\n\n## Review context\n\n");
    if context.branch.trim().is_empty() {
        document.push_str("- Branch: (detached or unavailable)\n");
    } else {
        document.push_str("- Branch: ");
        document.push_str(context.branch.trim());
        document.push('\n');
    }
    match &context.selection {
        ReviewSelection::None => document.push_str("- Selection: none\n"),
        ReviewSelection::WorkingTreeFile { staged, path } => {
            document.push_str("- Working-tree file (staged=");
            document.push_str(&staged.to_string());
            document.push_str("): ");
            document.push_str(path);
            document.push('\n');
        }
        ReviewSelection::Commit { oid } => {
            document.push_str("- Commit: ");
            document.push_str(oid);
            document.push('\n');
        }
        ReviewSelection::CommitFile { oid, path } => {
            document.push_str("- Commit: ");
            document.push_str(oid);
            document.push_str("\n- Commit file: ");
            document.push_str(path);
            document.push('\n');
        }
        ReviewSelection::Comparison { base, target, path } => {
            document.push_str("- Comparison: ");
            document.push_str(base);
            document.push_str(" -> ");
            document.push_str(target);
            document.push('\n');
            if let Some(path) = path {
                document.push_str("- Comparison file: ");
                document.push_str(path);
                document.push('\n');
            }
        }
    }
    document.push_str("\nUse the repository working tree as the source of truth. The references above are hints; inspect files and Git state yourself before making changes.\n");
    document
}

/// A task file whose lifecycle is tied to one Agent session.
#[derive(Debug)]
pub struct TaskFile {
    path: PathBuf,
}

impl TaskFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TaskFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Private app-owned task storage. The directory is intentionally outside the
/// repository so task files never appear in Git status.
#[derive(Clone, Debug)]
pub struct TaskStore {
    root: PathBuf,
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new(default_task_root())
    }
}

impl TaskStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn write(&self, document: &str) -> anyhow::Result<TaskFile> {
        fs::create_dir_all(&self.root)?;
        set_private_directory_permissions(&self.root);

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let filename =
            format!("task-{}-{}-{}.md", std::process::id(), timestamp, counter);
        let path = self.root.join(filename);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        if let Err(error) = file.write_all(document.as_bytes()) {
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        if let Err(error) = file.flush() {
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        set_private_file_permissions(&path);
        Ok(TaskFile { path })
    }

    /// Remove only stale files in this app-owned directory whose names match
    /// the generated task-file pattern.
    pub fn cleanup_stale(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let valid_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(valid_task_filename);
            if !valid_name {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > STALE_TASK_AGE);
            if stale {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn valid_task_filename(name: &str) -> bool {
    let Some(name) = name.strip_prefix("task-") else {
        return false;
    };
    let Some(name) = name.strip_suffix(".md") else {
        return false;
    };
    let mut parts = name.split('-');
    let Some(pid) = parts.next() else {
        return false;
    };
    let Some(timestamp) = parts.next() else {
        return false;
    };
    let Some(counter) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !pid.is_empty()
        && !timestamp.is_empty()
        && !counter.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && timestamp.bytes().all(|byte| byte.is_ascii_digit())
        && counter.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn task_file_env(task_file: &Path) -> (OsString, OsString) {
    (
        OsString::from(TASK_FILE_ENV),
        task_file.as_os_str().to_os_string(),
    )
}

pub fn probe_profile(profile: &ResolvedAgentProfile) -> anyhow::Result<String> {
    let mut command = Command::new(&profile.executable);
    command.args(&profile.args).arg("--version");
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = command.output()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(if text.is_empty() { error } else { text })
    } else {
        Err(anyhow::anyhow!(if error.is_empty() {
            format!("{} exited with {}", profile.name, output.status)
        } else {
            error
        }))
    }
}

fn default_task_root() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("augur-git")
        .join(TASK_DIRECTORY)
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_profiles_use_expected_prompt_positions() {
        let settings = AgentSettings::default();
        let codex = settings.profile("codex").expect("codex profile");
        assert_eq!(codex.launch_spec(PathBuf::from("task.md")).args.len(), 1);

        let opencode = settings.profile("opencode").expect("opencode profile");
        assert_eq!(
            opencode.launch_spec(PathBuf::from("task.md")).args,
            vec!["--prompt", BOOTSTRAP_PROMPT]
        );
    }

    #[test]
    fn custom_profile_arguments_are_structured_before_bootstrap_prompt() {
        let settings = AgentSettings {
            custom_profiles: vec![CustomAgentProfile {
                id: "reviewer".into(),
                name: "Reviewer".into(),
                executable: PathBuf::from("review-agent"),
                args: vec!["--mode".into(), "safe mode".into()],
                prompt_mode: PromptMode::Flag("--task".into()),
            }],
            ..Default::default()
        };
        let profile = settings.profile("reviewer").expect("profile");
        assert_eq!(
            profile.launch_spec(PathBuf::from("task.md")).args,
            vec!["--mode", "safe mode", "--task", BOOTSTRAP_PROMPT]
        );
    }

    #[test]
    fn invalid_custom_profile_is_not_resolved() {
        let settings = AgentSettings {
            custom_profiles: vec![CustomAgentProfile {
                id: "broken".into(),
                name: String::new(),
                executable: PathBuf::from("agent"),
                args: Vec::new(),
                prompt_mode: PromptMode::TrailingArgument,
            }],
            ..Default::default()
        };
        assert!(settings.profile("broken").is_none());
    }

    #[test]
    fn task_document_contains_references_without_diff() {
        let context = ReviewContext {
            branch: "feature/demo".into(),
            selection: ReviewSelection::CommitFile {
                oid: "0123456789abcdef".into(),
                path: "src/main.rs".into(),
            },
        };
        let text = task_document("Fix the bug", &context);
        assert!(text.contains("Fix the bug"));
        assert!(text.contains("feature/demo"));
        assert!(text.contains("src/main.rs"));
        assert!(!text.contains("diff --git"));
    }

    #[test]
    fn task_file_is_removed_when_dropped() {
        let root = std::env::temp_dir()
            .join(format!("augur-agent-test-{}", std::process::id()));
        let store = TaskStore::new(root.clone());
        let task = store.write("task").expect("task file");
        let path = task.path().to_path_buf();
        assert!(path.exists());
        drop(task);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn task_storage_uses_private_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir()
            .join(format!("augur-agent-permissions-{}", std::process::id()));
        let store = TaskStore::new(root.clone());
        let task = store.write("private task").expect("task file");
        assert_eq!(
            fs::metadata(&root)
                .expect("task root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(task.path())
                .expect("task metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(task);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_cleanup_only_accepts_generated_names() {
        assert!(valid_task_filename("task-42-123-0.md"));
        assert!(!valid_task_filename("task-old.md"));
        assert!(!valid_task_filename("task-42-123.md"));
        assert!(!valid_task_filename("task-42-123-0.txt"));
    }

    #[test]
    fn prompt_mode_accepts_stable_kebab_case_and_legacy_variant_names() {
        let mode: PromptMode = serde_json::from_str(r#"{"flag":"--prompt"}"#)
            .expect("kebab-case prompt mode");
        assert_eq!(mode, PromptMode::Flag("--prompt".into()));
        let legacy: PromptMode = serde_json::from_str(r#"{"Flag":"--prompt"}"#)
            .expect("legacy prompt mode");
        assert_eq!(legacy, PromptMode::Flag("--prompt".into()));
        assert_eq!(
            serde_json::to_string(&PromptMode::TrailingArgument).unwrap(),
            "\"trailing-argument\""
        );
    }

    #[test]
    fn invalid_default_profile_is_reported() {
        let settings = AgentSettings {
            default_profile_id: Some("missing".into()),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
    }
}
