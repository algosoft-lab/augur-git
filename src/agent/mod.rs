//! External coding-agent profiles and safe launch arguments.
//!
//! This module deliberately contains no provider SDKs or agent logic. It only
//! prepares a structured PTY launch request for a user-installed CLI agent.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::{Deserialize, Serialize};

pub mod operations;
pub use operations::{AgentCommitChallenge, AgentOperation};

const TEST_DIRECTORY_PREFIX: &str = "augur-git-agent-test";

/// Built-in providers supported by the first-party UI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuiltInAgent {
    #[serde(alias = "Codex")]
    Codex,
    #[serde(alias = "ClaudeCode")]
    ClaudeCode,
    #[serde(alias = "OpenCode", alias = "opencode")]
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

    pub const fn supported_reasoning_efforts(self) -> &'static [&'static str] {
        match self {
            Self::Codex => &["minimal", "low", "medium", "high", "xhigh"],
            Self::ClaudeCode => &["low", "medium", "high", "xhigh", "max"],
            Self::OpenCode => &[],
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

/// Optional launch-time overrides for a built-in Agent CLI.
///
/// A missing value deliberately means that Augur Git omits the corresponding
/// command line option so the CLI can resolve its normal environment and
/// config-file defaults.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct AgentLaunchOverrides {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub variant: Option<String>,
}

impl AgentLaunchOverrides {
    pub fn validate_for(&self, agent: BuiltInAgent) -> Result<(), String> {
        if let Some(model) = self.model.as_deref() {
            validate_override_value(model, "model")?;
        }
        if let Some(variant) = self.variant.as_deref() {
            validate_override_value(variant, "variant")?;
            if agent != BuiltInAgent::OpenCode {
                return Err(format!(
                    "{} does not support a startup variant override",
                    agent.display_name()
                ));
            }
        }
        if let Some(reasoning_effort) = self.reasoning_effort.as_deref() {
            validate_override_value(reasoning_effort, "reasoning effort")?;
            if agent == BuiltInAgent::OpenCode {
                return Err(
                    "OpenCode uses a model variant instead of a reasoning-effort override"
                        .to_string(),
                );
            }
            if !agent
                .supported_reasoning_efforts()
                .contains(&reasoning_effort)
            {
                return Err(format!(
                    "{} does not support reasoning effort '{reasoning_effort}'",
                    agent.display_name()
                ));
            }
        }
        Ok(())
    }
}

fn validate_override_value(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
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
    pub launch_overrides: HashMap<BuiltInAgent, AgentLaunchOverrides>,
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
        for (agent, overrides) in &self.launch_overrides {
            if let Err(error) = overrides.validate_for(*agent) {
                errors.push(format!("{}: {error}", agent.id()));
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
                built_in: Some(agent),
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
                built_in: None,
            })
    }

    pub fn launch_overrides_for(
        &self,
        profile: &ResolvedAgentProfile,
    ) -> AgentLaunchOverrides {
        profile
            .built_in
            .and_then(|agent| self.launch_overrides.get(&agent).cloned())
            .unwrap_or_default()
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
    pub built_in: Option<BuiltInAgent>,
}

/// A structured command specification consumed by the terminal backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchSpecError {
    InvalidOverride {
        agent: BuiltInAgent,
        summary: String,
    },
    CustomProfileOverrides,
}

impl std::fmt::Display for LaunchSpecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOverride { agent, summary } => {
                write!(
                    formatter,
                    "{} launch override is invalid: {summary}",
                    agent.display_name()
                )
            }
            Self::CustomProfileOverrides => formatter.write_str(
                "custom profiles do not support typed launch overrides",
            ),
        }
    }
}

impl std::error::Error for LaunchSpecError {}

/// A fixed, non-destructive challenge used to verify that an Agent receives a
/// prompt and can return a response through the visible terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentConnectivityChallenge {
    pub prompt: String,
    pub expected_response: String,
}

impl AgentConnectivityChallenge {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let token =
            format!("augur-git-check-{}-{counter:016x}", std::process::id());
        let expected_response = token.chars().rev().collect::<String>();
        let prompt = format!(
            "Augur Git connectivity diagnostic. Do not read, create, edit, delete, or execute anything in this directory. Reverse this token and reply with the reversed token only: {token}. Then remain in the interactive session."
        );
        Self {
            prompt,
            expected_response,
        }
    }
}

impl Default for AgentConnectivityChallenge {
    fn default() -> Self {
        Self::new()
    }
}

impl ResolvedAgentProfile {
    /// Build a direct-prompt launch with structured executable arguments.
    pub fn launch_spec_for_prompt(&self, prompt: &str) -> AgentLaunchSpec {
        self.launch_spec_for_prompt_with_overrides(
            prompt,
            &AgentLaunchOverrides::default(),
        )
        .unwrap_or_else(|_| AgentLaunchSpec {
            executable: self.executable.clone(),
            args: self.args.clone(),
        })
    }

    /// Build a direct-prompt launch with provider-specific typed overrides.
    pub fn launch_spec_for_prompt_with_overrides(
        &self,
        prompt: &str,
        overrides: &AgentLaunchOverrides,
    ) -> Result<AgentLaunchSpec, LaunchSpecError> {
        let Some(agent) = self.built_in else {
            if overrides.model.is_some()
                || overrides.reasoning_effort.is_some()
                || overrides.variant.is_some()
            {
                return Err(LaunchSpecError::CustomProfileOverrides);
            }
            return Ok(self.launch_spec_with_args(prompt, self.args.clone()));
        };
        overrides.validate_for(agent).map_err(|summary| {
            LaunchSpecError::InvalidOverride { agent, summary }
        })?;

        let mut args = self.args.clone();
        if let Some(model) = overrides.model.as_deref() {
            args.extend(["--model".to_string(), model.to_string()]);
        }
        if let Some(reasoning_effort) = overrides.reasoning_effort.as_deref() {
            match agent {
                BuiltInAgent::Codex => {
                    args.extend([
                        "--config".to_string(),
                        format!(
                            "model_reasoning_effort=\"{reasoning_effort}\""
                        ),
                    ]);
                }
                BuiltInAgent::ClaudeCode => {
                    args.extend([
                        "--effort".to_string(),
                        reasoning_effort.to_string(),
                    ]);
                }
                BuiltInAgent::OpenCode => {
                    // validate_for rejects this before reaching the adapter;
                    // keeping the arm explicit prevents accidental silent
                    // dropping if the supported-value policy changes later.
                    return Err(LaunchSpecError::InvalidOverride {
                        agent,
                        summary:
                            "interactive OpenCode does not support a startup reasoning override"
                                .to_string(),
                    });
                }
            }
        }
        if let Some(variant) = overrides.variant.as_deref() {
            if agent != BuiltInAgent::OpenCode {
                return Err(LaunchSpecError::InvalidOverride {
                    agent,
                    summary:
                        "only OpenCode supports a startup variant override"
                            .to_string(),
                });
            }
            args.extend(["--variant".to_string(), variant.to_string()]);
        }
        Ok(self.launch_spec_with_args(prompt, args))
    }

    fn launch_spec_with_args(
        &self,
        prompt: &str,
        mut args: Vec<String>,
    ) -> AgentLaunchSpec {
        match &self.prompt_mode {
            PromptMode::TrailingArgument => args.push(prompt.to_string()),
            PromptMode::Flag(flag) => {
                args.push(flag.clone());
                args.push(prompt.to_string());
            }
        }
        AgentLaunchSpec {
            executable: self.executable.clone(),
            args,
        }
    }
}

/// A unique empty directory owned by one connectivity test.
///
/// On macOS and Linux the directory lives below the user's local application
/// data directory. This keeps the working directory inside the user's home
/// tree, which is accessible to external Agent CLIs whose sandboxes reject
/// system temporary locations. Other platforms continue to use the system
/// temporary directory. The directory is removed after the test process exits
/// or the test window is dropped. A shared cleanup state lets the PTY and UI
/// retry cleanup independently.
#[derive(Clone, Debug)]
pub struct AgentTestDirectory {
    path: PathBuf,
    cleanup: Arc<AgentTestDirectoryCleanup>,
}

#[derive(Debug)]
struct AgentTestDirectoryCleanup {
    path: PathBuf,
    cleaned: AtomicBool,
}

impl AgentTestDirectory {
    pub fn create() -> anyhow::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = agent_test_directory_root()?;
        fs::create_dir_all(&base)?;
        set_private_directory_permissions(&base);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let path = base.join(format!(
            "{TEST_DIRECTORY_PREFIX}-{}-{timestamp}-{counter}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        set_private_directory_permissions(&path);
        Ok(Self {
            cleanup: Arc::new(AgentTestDirectoryCleanup {
                path: path.clone(),
                cleaned: AtomicBool::new(false),
            }),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Remove the directory once, returning an error so callers can retry
    /// after a platform releases an open working-directory handle.
    pub fn cleanup(&self) -> anyhow::Result<()> {
        if self.cleanup.cleaned.load(Ordering::Acquire) {
            return Ok(());
        }
        match fs::remove_dir_all(&self.cleanup.path) {
            Ok(()) => {
                self.cleanup.cleaned.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cleanup.cleaned.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }
}

fn agent_test_directory_root() -> anyhow::Result<PathBuf> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let data_dir = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "could not locate a per-user data directory for the Agent test"
                )
            })?;
        return Ok(data_dir.join("augur-git").join("agent-tests"));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(std::env::temp_dir())
    }
}

impl Drop for AgentTestDirectory {
    fn drop(&mut self) {
        // Clones are passed to the window, PTY proxy, and event-loop joiner.
        // Only the final owner may perform implicit cleanup; otherwise a
        // short-lived clone can remove the working directory while the child
        // is still starting (Unix permits unlinking an active cwd).
        if Arc::strong_count(&self.cleanup) != 1 {
            return;
        }
        if self.cleanup().is_err() {
            log::debug!(
                "[agent_terminal] temporary test directory cleanup deferred"
            );
        }
    }
}

/// Resolve a profile executable using the same Windows executable suffixes
/// that an interactive command shell uses for npm-installed CLI shims.
pub fn resolve_executable(path: &Path) -> anyhow::Result<PathBuf> {
    #[cfg(windows)]
    {
        return resolve_windows_executable(path);
    }

    #[cfg(not(windows))]
    {
        if (path.is_absolute() || path.components().count() > 1)
            && !path.is_file()
        {
            anyhow::bail!("executable '{}' was not found", path.display());
        }
        Ok(path.to_path_buf())
    }
}

#[cfg(windows)]
fn resolve_windows_executable(path: &Path) -> anyhow::Result<PathBuf> {
    let has_directory = path.is_absolute()
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
    if has_directory {
        if let Some(candidate) = windows_existing_candidate(path) {
            return Ok(candidate);
        }
        anyhow::bail!("executable '{}' was not found", path.display());
    }

    let path_variable = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path_variable) {
        if let Some(candidate) =
            windows_existing_candidate(&directory.join(path))
        {
            return Ok(candidate);
        }
    }
    anyhow::bail!("executable '{}' was not found in PATH", path.display());
}

#[cfg(windows)]
fn windows_existing_candidate(path: &Path) -> Option<PathBuf> {
    let candidates = if path.extension().is_some() {
        vec![path.to_path_buf()]
    } else {
        vec![
            path.with_extension("exe"),
            path.with_extension("cmd"),
            path.with_extension("bat"),
        ]
    };
    candidates.into_iter().find(|candidate| candidate.is_file())
}

pub fn probe_profile(profile: &ResolvedAgentProfile) -> anyhow::Result<String> {
    let executable = resolve_executable(&profile.executable)?;
    let mut command = Command::new(executable);
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

/// Capabilities discovered from a provider CLI's own help output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentCliCapabilities {
    /// Whether the root interactive command accepts `--variant`.
    pub supports_interactive_variant: bool,
}

/// Probe optional arguments without starting an interactive session.
///
/// OpenCode versions that predate root-TUI Variant support still expose a
/// useful help page, so a non-zero help exit status is accepted when output is
/// present. This keeps the probe compatible with CLI wrappers that use a
/// non-standard help exit code while still reporting genuine launch failures.
pub fn probe_profile_capabilities(
    profile: &ResolvedAgentProfile,
) -> anyhow::Result<AgentCliCapabilities> {
    let executable = resolve_executable(&profile.executable)?;
    let mut command = Command::new(executable);
    command.args(&profile.args).arg("--help");
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let help = if stdout.trim().is_empty() {
        stderr.into_owned()
    } else if stderr.trim().is_empty() {
        stdout.into_owned()
    } else {
        // Keep the probe in-memory and avoid logging provider output.
        format!("{stdout}\n{stderr}")
    };
    if help.trim().is_empty() {
        return Err(anyhow::anyhow!(if output.status.success() {
            format!("{} returned empty help output", profile.name)
        } else {
            format!("{} exited with {}", profile.name, output.status)
        }));
    }
    Ok(AgentCliCapabilities {
        supports_interactive_variant: help_advertises_flag(&help, "--variant"),
    })
}

fn help_advertises_flag(help: &str, flag: &str) -> bool {
    help.split_whitespace().any(|token| {
        token == flag
            || token
                .strip_prefix(flag)
                .is_some_and(|suffix| suffix.starts_with('='))
    })
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) {}

#[cfg(test)]
mod launch_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_profiles_use_expected_prompt_positions() {
        let settings = AgentSettings::default();
        let codex = settings.profile("codex").expect("codex profile");
        assert_eq!(
            codex.launch_spec_for_prompt("diagnostic").args,
            vec!["diagnostic"]
        );

        let opencode = settings.profile("opencode").expect("opencode profile");
        assert_eq!(
            opencode.launch_spec_for_prompt("diagnostic").args,
            vec!["--prompt", "diagnostic"]
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
            profile.launch_spec_for_prompt("diagnostic").args,
            vec!["--mode", "safe mode", "--task", "diagnostic"]
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

    #[cfg(windows)]
    #[test]
    fn windows_resolver_accepts_cmd_shims_without_an_extension() {
        let directory = std::env::temp_dir().join(format!(
            "augur-agent-resolver-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("resolver test directory");
        let shim = directory.join("agent.cmd");
        fs::write(&shim, "@echo off\r\n").expect("resolver shim");

        let resolved = resolve_executable(&directory.join("agent"))
            .expect("cmd shim should resolve");
        assert_eq!(resolved, shim);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn windows_probe_runs_a_cmd_shim() {
        let directory = std::env::temp_dir().join(format!(
            "augur-agent-probe-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&directory).expect("probe test directory");
        let shim = directory.join("agent.cmd");
        fs::write(&shim, "@echo off\r\necho shim-version\r\n")
            .expect("probe shim");
        let profile = ResolvedAgentProfile {
            id: "probe".into(),
            name: "Probe".into(),
            executable: directory.join("agent"),
            args: Vec::new(),
            prompt_mode: PromptMode::TrailingArgument,
            built_in: None,
        };

        assert_eq!(
            probe_profile(&profile).expect("probe result"),
            "shim-version"
        );
        let _ = fs::remove_dir_all(directory);
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
    fn help_probe_detects_exact_optional_flags() {
        let help = "Options: --model <id> --variant <name> --prompt <text>";
        assert!(help_advertises_flag(help, "--variant"));
        assert!(!help_advertises_flag(help, "--reasoning"));
        assert!(!help_advertises_flag("--variant-name", "--variant"));
        assert!(help_advertises_flag("--variant=high", "--variant"));
    }

    #[test]
    fn invalid_default_profile_is_reported() {
        let settings = AgentSettings {
            default_profile_id: Some("missing".into()),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn connectivity_challenge_does_not_contain_expected_response() {
        let challenge = AgentConnectivityChallenge::new();
        assert!(!challenge.prompt.contains(&challenge.expected_response));
        assert!(!challenge.expected_response.is_empty());
        let token = challenge
            .prompt
            .split_once(": ")
            .and_then(|(_, rest)| rest.split_once(". Then"))
            .map(|(token, _)| token)
            .expect("challenge token");
        assert_eq!(
            token.chars().rev().collect::<String>(),
            challenge.expected_response
        );
    }

    #[test]
    fn connectivity_prompt_uses_structured_arguments() {
        let settings = AgentSettings {
            custom_profiles: vec![CustomAgentProfile {
                id: "flag-agent".into(),
                name: "Flag Agent".into(),
                executable: PathBuf::from("flag-agent"),
                args: vec!["--mode".into(), "interactive".into()],
                prompt_mode: PromptMode::Flag("--prompt".into()),
            }],
            ..Default::default()
        };
        let profile = settings.profile("flag-agent").expect("profile");
        let spec = profile.launch_spec_for_prompt("diagnostic prompt");
        assert_eq!(
            spec.args,
            vec!["--mode", "interactive", "--prompt", "diagnostic prompt"]
        );

        let codex = settings.profile("codex").expect("codex profile");
        let spec = codex.launch_spec_for_prompt("diagnostic prompt");
        assert_eq!(spec.args, vec!["diagnostic prompt"]);
    }

    #[test]
    fn connectivity_test_directory_starts_empty_and_can_be_cleaned() {
        let directory = AgentTestDirectory::create().expect("test directory");
        let second = AgentTestDirectory::create().expect("second directory");
        let path = directory.path().to_path_buf();
        assert!(path.is_dir());
        assert_ne!(path, second.path());
        assert_eq!(fs::read_dir(&path).expect("directory entries").count(), 0);
        directory.cleanup().expect("cleanup");
        second.cleanup().expect("second cleanup");
        assert!(!path.exists());
    }

    #[test]
    fn dropping_a_directory_clone_does_not_cleanup_the_active_test() {
        let directory = AgentTestDirectory::create().expect("test directory");
        let path = directory.path().to_path_buf();
        let clone = directory.clone();

        drop(clone);
        assert!(path.is_dir());

        directory.cleanup().expect("cleanup");
        assert!(!path.exists());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn connectivity_test_directory_uses_per_user_agent_root() {
        let directory = AgentTestDirectory::create().expect("test directory");
        let expected_root = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .expect("per-user data directory")
            .join("augur-git")
            .join("agent-tests");

        assert_eq!(directory.path().parent(), Some(expected_root.as_path()));
        assert!(directory.path().starts_with(&expected_root));
        directory.cleanup().expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn connectivity_test_directory_uses_private_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = AgentTestDirectory::create().expect("test directory");
        assert_eq!(
            fs::metadata(directory.path())
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        directory.cleanup().expect("cleanup");
    }
}
