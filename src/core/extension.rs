//! Data-only types shared by the Lua extension host and the workspace UI.
//!
//! This module deliberately does not depend on GPUI. Manifest validation,
//! schedule calculations, package identity, and run summaries can therefore
//! be tested without starting the application.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeZone};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const EXTENSION_API_VERSION: u32 = 1;
pub const MAX_EXTENSION_ID_LENGTH: usize = 64;
pub const MAX_EXTENSION_PACKAGE_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_EXTENSION_RUN_HISTORY: usize = 50;

/// The source of an installed extension package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionSource {
    Bundled,
    LocalDirectory,
}

/// Supported declarative settings rendered by the extension page.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingDefinition {
    String {
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        default: String,
    },
    Integer {
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        default: i64,
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
    },
    Boolean {
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        default: bool,
    },
    Time {
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default = "default_schedule_time")]
        default: String,
    },
    Select {
        label: String,
        #[serde(default)]
        description: Option<String>,
        options: Vec<SelectOption>,
        default: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SettingDefinition {
    pub fn label(&self) -> &str {
        match self {
            Self::String { label, .. }
            | Self::Integer { label, .. }
            | Self::Boolean { label, .. }
            | Self::Time { label, .. }
            | Self::Select { label, .. } => label,
        }
    }

    pub fn default_value(&self) -> SettingValue {
        match self {
            Self::String { default, .. } => {
                SettingValue::String(default.clone())
            }
            Self::Integer { default, .. } => SettingValue::Integer(*default),
            Self::Boolean { default, .. } => SettingValue::Boolean(*default),
            Self::Time { default, .. } => SettingValue::Time(default.clone()),
            Self::Select { default, .. } => {
                SettingValue::Select(default.clone())
            }
        }
    }

    pub fn validate_value(&self, value: &SettingValue) -> Result<(), String> {
        match (self, value) {
            (Self::String { .. }, SettingValue::String(_))
            | (Self::Boolean { .. }, SettingValue::Boolean(_))
            | (Self::Time { .. }, SettingValue::Time(_))
            | (Self::Select { .. }, SettingValue::Select(_))
            | (Self::Integer { .. }, SettingValue::Integer(_)) => {}
            _ => return Err("setting value has the wrong type".to_string()),
        }
        match self {
            Self::Integer { min, max, .. } => {
                let SettingValue::Integer(value) = value else {
                    unreachable!("integer type was checked above")
                };
                if min.is_some_and(|minimum| *value < minimum)
                    || max.is_some_and(|maximum| *value > maximum)
                {
                    return Err("integer setting is outside its allowed range"
                        .to_string());
                }
            }
            Self::Time { .. } => {
                let SettingValue::Time(value) = value else {
                    unreachable!("time type was checked above")
                };
                parse_daily_time(value)?;
            }
            Self::Select { options, .. } => {
                let SettingValue::Select(value) = value else {
                    unreachable!("select type was checked above")
                };
                if !options.iter().any(|option| option.value == *value) {
                    return Err(
                        "select setting value is not one of its options"
                            .to_string(),
                    );
                }
            }
            Self::String { .. } | Self::Boolean { .. } => {}
        }
        Ok(())
    }
}

fn default_schedule_time() -> String {
    "02:00".to_string()
}

/// Values persisted for an extension setting and exposed to Lua.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SettingValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Time(String),
    Select(String),
}

/// A daily trigger declared by an extension manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DailyTrigger {
    pub id: String,
    pub time_setting: String,
    pub handler: String,
}

/// On-disk manifest. Unknown fields are rejected so typos do not silently
/// change automation behavior.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    pub id: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default = "default_entrypoint")]
    pub entrypoint: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, SettingDefinition>,
    #[serde(default)]
    pub manual_handler: Option<String>,
    #[serde(default)]
    pub daily: Vec<DailyTrigger>,
}

fn default_entrypoint() -> String {
    "main.lua".to_string()
}

impl ExtensionManifest {
    pub fn parse(text: &str) -> Result<Self, ExtensionError> {
        let manifest = toml::from_str::<Self>(text).map_err(|error| {
            ExtensionError::InvalidManifest(error.to_string())
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ExtensionError> {
        validate_extension_id(&self.id)?;
        Version::parse(&self.version).map_err(|error| {
            ExtensionError::InvalidManifest(format!("invalid version: {error}"))
        })?;
        if self.api_version != EXTENSION_API_VERSION {
            return Err(ExtensionError::UnsupportedApiVersion(
                self.api_version,
            ));
        }
        validate_relative_lua_path(&self.entrypoint)?;
        if !self.entrypoint.ends_with(".lua") {
            return Err(ExtensionError::InvalidManifest(
                "entrypoint must point to a .lua file".to_string(),
            ));
        }
        if self.name.trim().is_empty() || self.description.trim().is_empty() {
            return Err(ExtensionError::InvalidManifest(
                "name and description must not be empty".to_string(),
            ));
        }
        for (key, definition) in &self.settings {
            if key.trim().is_empty() || key.contains('.') {
                return Err(ExtensionError::InvalidManifest(format!(
                    "invalid setting key: {key}"
                )));
            }
            definition
                .validate_value(&definition.default_value())
                .map_err(|error| {
                    ExtensionError::InvalidManifest(format!(
                        "setting {key}: {error}"
                    ))
                })?;
        }
        for trigger in &self.daily {
            if trigger.id.trim().is_empty() || trigger.handler.trim().is_empty()
            {
                return Err(ExtensionError::InvalidManifest(
                    "daily trigger id and handler must not be empty"
                        .to_string(),
                ));
            }
            if !matches!(
                self.settings.get(&trigger.time_setting),
                Some(SettingDefinition::Time { .. })
            ) {
                return Err(ExtensionError::InvalidManifest(format!(
                    "daily trigger {} references a non-time setting: {}",
                    trigger.id, trigger.time_setting
                )));
            }
        }
        Ok(())
    }

    pub fn default_settings(&self) -> BTreeMap<String, SettingValue> {
        self.settings
            .iter()
            .map(|(key, definition)| (key.clone(), definition.default_value()))
            .collect()
    }
}

fn validate_extension_id(id: &str) -> Result<(), ExtensionError> {
    if id.is_empty() || id.len() > MAX_EXTENSION_ID_LENGTH {
        return Err(ExtensionError::InvalidManifest(
            "extension id must contain 1 to 64 bytes".to_string(),
        ));
    }
    if !id.bytes().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'_' | b'-')
    }) || !id.as_bytes()[0].is_ascii_lowercase()
        && !id.as_bytes()[0].is_ascii_digit()
    {
        return Err(ExtensionError::InvalidManifest(
            "extension id must use lowercase ASCII letters, digits, '.', '_' or '-' and start with a letter or digit".to_string(),
        ));
    }
    Ok(())
}

fn validate_relative_lua_path(path: &str) -> Result<(), ExtensionError> {
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(ExtensionError::InvalidManifest(
            "entrypoint must be a relative path inside the extension package"
                .to_string(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionError {
    InvalidManifest(String),
    UnsupportedApiVersion(u32),
    MissingFile(PathBuf),
    PackageTooLarge,
    SymlinkNotAllowed(PathBuf),
    Io(String),
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidManifest(error) => {
                write!(formatter, "invalid extension manifest: {error}")
            }
            Self::UnsupportedApiVersion(version) => write!(
                formatter,
                "unsupported extension API version: {version}"
            ),
            Self::MissingFile(path) => write!(
                formatter,
                "extension package is missing {}",
                path.display()
            ),
            Self::PackageTooLarge => formatter
                .write_str("extension package exceeds the 100 MiB limit"),
            Self::SymlinkNotAllowed(path) => write!(
                formatter,
                "extension package contains a symlink: {}",
                path.display()
            ),
            Self::Io(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for ExtensionError {}

/// A validated package discovered by the extension manager.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionPackage {
    pub manifest: ExtensionManifest,
    pub root: Option<PathBuf>,
    pub source: ExtensionSource,
    pub fingerprint: String,
    pub bundled: bool,
}

/// Return the user extension root without falling back to the current
/// working directory. Executable extension code must never silently land in a
/// repository or an arbitrary process directory.
pub fn extensions_root() -> Result<PathBuf, ExtensionError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        ExtensionError::Io(
            "could not locate the platform local data directory for extensions"
                .to_string(),
        )
    })?;
    Ok(base.join("augur-git").join("extensions"))
}

/// Discover every valid local package. Invalid directories are returned as
/// individual errors so one broken package does not hide the others.
pub fn discover_local_packages()
-> Result<Vec<Result<ExtensionPackage, ExtensionError>>, ExtensionError> {
    let root = extensions_root()?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = fs::read_dir(&root)
        .map_err(|error| {
            ExtensionError::Io(format!(
                "failed to read extension directory: {error}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            ExtensionError::Io(format!(
                "failed to enumerate extension directory: {error}"
            ))
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return None;
            }
            Some(load_local_package(&path))
        })
        .collect())
}

/// Load and validate a package already present at `root`.
pub fn load_local_package(
    root: &Path,
) -> Result<ExtensionPackage, ExtensionError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        ExtensionError::Io(format!(
            "failed to inspect extension package: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ExtensionError::SymlinkNotAllowed(root.to_path_buf()));
    }
    let manifest_path = root.join("manifest.toml");
    let manifest_text =
        fs::read_to_string(&manifest_path).map_err(|error| {
            ExtensionError::Io(format!(
                "failed to read {}: {error}",
                manifest_path.display()
            ))
        })?;
    let manifest = ExtensionManifest::parse(&manifest_text)?;
    let entrypoint = root.join(&manifest.entrypoint);
    let entry_metadata = fs::symlink_metadata(&entrypoint).map_err(|_| {
        ExtensionError::MissingFile(PathBuf::from(&manifest.entrypoint))
    })?;
    if !entry_metadata.is_file() || entry_metadata.file_type().is_symlink() {
        return Err(ExtensionError::MissingFile(PathBuf::from(
            &manifest.entrypoint,
        )));
    }
    let fingerprint = fingerprint_directory(root)?;
    Ok(ExtensionPackage {
        manifest,
        root: Some(root.to_path_buf()),
        source: ExtensionSource::LocalDirectory,
        fingerprint,
        bundled: false,
    })
}

/// Install a local directory package. Existing packages are moved aside
/// before the staging directory is promoted, allowing a failed replacement to
/// restore the previous package on platforms that cannot replace directories
/// with one rename.
pub fn install_local_package(
    source: &Path,
) -> Result<ExtensionPackage, ExtensionError> {
    let package = load_local_package(source)?;
    let root = extensions_root()?;
    fs::create_dir_all(&root).map_err(|error| {
        ExtensionError::Io(format!(
            "failed to create extension directory: {error}"
        ))
    })?;
    let destination = root.join(&package.manifest.id);
    let staging = root.join(format!(
        ".{}.staging-{}",
        package.manifest.id,
        next_package_counter()
    ));
    copy_package_tree(source, &staging)?;

    let backup = root.join(format!(
        ".{}.backup-{}",
        package.manifest.id,
        next_package_counter()
    ));
    let had_existing = destination.exists();
    if had_existing {
        fs::rename(&destination, &backup).map_err(|error| {
            let _ = fs::remove_dir_all(&staging);
            ExtensionError::Io(format!(
                "failed to stage the previous extension package: {error}"
            ))
        })?;
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        let _ = fs::remove_dir_all(&staging);
        if had_existing {
            let _ = fs::rename(&backup, &destination);
        }
        return Err(ExtensionError::Io(format!(
            "failed to install extension package: {error}"
        )));
    }
    if had_existing {
        fs::remove_dir_all(&backup).map_err(|error| {
            ExtensionError::Io(format!(
                "extension installed but old package cleanup failed: {error}"
            ))
        })?;
    }
    load_local_package(&destination)
}

fn next_package_counter() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn copy_package_tree(
    source: &Path,
    destination: &Path,
) -> Result<(), ExtensionError> {
    let mut total_bytes = 0u64;
    let mut stack = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        let metadata = fs::symlink_metadata(&from).map_err(|error| {
            ExtensionError::Io(format!(
                "failed to inspect package entry: {error}"
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ExtensionError::SymlinkNotAllowed(from));
        }
        if metadata.is_dir() {
            fs::create_dir_all(&to).map_err(|error| {
                ExtensionError::Io(format!(
                    "failed to create package directory: {error}"
                ))
            })?;
            let entries = fs::read_dir(&from).map_err(|error| {
                ExtensionError::Io(format!(
                    "failed to read package directory: {error}"
                ))
            })?;
            for entry in entries {
                let entry = entry.map_err(|error| {
                    ExtensionError::Io(format!(
                        "failed to enumerate package entry: {error}"
                    ))
                })?;
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                stack.push((entry.path(), to.join(name)));
            }
        } else if metadata.is_file() {
            total_bytes = total_bytes.saturating_add(metadata.len());
            if total_bytes > MAX_EXTENSION_PACKAGE_BYTES {
                return Err(ExtensionError::PackageTooLarge);
            }
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    ExtensionError::Io(format!(
                        "failed to create package parent: {error}"
                    ))
                })?;
            }
            fs::copy(&from, &to).map_err(|error| {
                ExtensionError::Io(format!(
                    "failed to copy extension package entry: {error}"
                ))
            })?;
        }
    }
    Ok(())
}

fn fingerprint_directory(root: &Path) -> Result<String, ExtensionError> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (relative, path, size) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(size.to_le_bytes());
        hasher.update(fs::read(&path).map_err(|error| {
            ExtensionError::Io(format!(
                "failed to hash extension package: {error}"
            ))
        })?);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, PathBuf, u64)>,
) -> Result<(), ExtensionError> {
    let entries = fs::read_dir(current).map_err(|error| {
        ExtensionError::Io(format!("failed to read extension package: {error}"))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ExtensionError::Io(format!(
                "failed to enumerate extension package: {error}"
            ))
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            ExtensionError::Io(format!(
                "failed to inspect extension package: {error}"
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ExtensionError::SymlinkNotAllowed(path));
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| ExtensionError::Io(error.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path, metadata.len()));
        }
    }
    let total: u64 = files.iter().map(|(_, _, size)| *size).sum();
    if total > MAX_EXTENSION_PACKAGE_BYTES {
        return Err(ExtensionError::PackageTooLarge);
    }
    Ok(())
}

/// Persistent user choices for one extension.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtensionSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub values: BTreeMap<String, SettingValue>,
    #[serde(default)]
    pub last_seen_fingerprint: Option<String>,
    #[serde(default)]
    pub last_scheduled_date: Option<String>,
}

impl ExtensionSettings {
    pub fn with_defaults(manifest: &ExtensionManifest) -> Self {
        Self {
            values: manifest.default_settings(),
            ..Self::default()
        }
    }

    pub fn normalized_for(&self, manifest: &ExtensionManifest) -> Self {
        let mut normalized = self.clone();
        for (key, definition) in &manifest.settings {
            let value = normalized
                .values
                .get(key)
                .filter(|value| definition.validate_value(value).is_ok())
                .cloned()
                .unwrap_or_else(|| definition.default_value());
            normalized.values.insert(key.clone(), value);
        }
        normalized
            .values
            .retain(|key, _| manifest.settings.contains_key(key));
        normalized
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum ExtensionRunTrigger {
    Manual,
    Schedule { trigger_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum RepositoryRunResult {
    Success { summary: String },
    Failed { code: String, summary: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExtensionRunRecord {
    pub run_id: u64,
    pub trigger: ExtensionRunTrigger,
    pub started_at: String,
    pub finished_at: String,
    pub repositories: Vec<RepositoryRunRecord>,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RepositoryRunRecord {
    pub display_name: String,
    pub result: RepositoryRunResult,
    pub steps: Vec<String>,
}

/// Parse a daily local-time setting in `HH:MM` form.
pub fn parse_daily_time(value: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(value.trim(), "%H:%M").map_err(|_| {
        "time setting must use HH:MM in the local 24-hour clock".to_string()
    })
}

/// Return the first valid local occurrence of a daily time after `after`.
/// Non-existent DST times are skipped; ambiguous times use the earlier one.
pub fn daily_occurrence(
    date: chrono::NaiveDate,
    time: NaiveTime,
) -> Option<DateTime<Local>> {
    match Local.from_local_datetime(&date.and_time(time)) {
        chrono::LocalResult::Single(value) => Some(value),
        chrono::LocalResult::Ambiguous(earlier, _) => Some(earlier),
        chrono::LocalResult::None => None,
    }
}

/// Whether a daily trigger should fire between two local instants.
pub fn should_fire_daily(
    last_check: DateTime<Local>,
    now: DateTime<Local>,
    time: NaiveTime,
) -> bool {
    if now <= last_check {
        return false;
    }
    let mut date = last_check.date_naive();
    while date <= now.date_naive() {
        if let Some(occurrence) = daily_occurrence(date, time)
            && occurrence > last_check
            && occurrence <= now
        {
            return true;
        }
        date = date.succ_opt().unwrap_or(now.date_naive());
        if date == now.date_naive()
            && now.date_naive() == last_check.date_naive()
        {
            break;
        }
    }
    false
}

pub fn local_date_string(value: DateTime<Local>) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        value.month(),
        value.day()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> ExtensionManifest {
        ExtensionManifest::parse(
            r#"
id = "example.sync"
version = "0.1.0"
api_version = 1
name = "Example"
description = "Example extension"
manual_handler = "on_run"

[settings.sync_time]
type = "time"
label = "Time"
default = "02:00"

[[daily]]
id = "daily"
time_setting = "sync_time"
handler = "on_schedule"
"#,
        )
        .unwrap()
    }

    #[test]
    fn validates_manifest_and_default_values() {
        let manifest = manifest();
        assert_eq!(
            manifest.default_settings()["sync_time"],
            SettingValue::Time("02:00".to_string())
        );
    }

    #[test]
    fn rejects_invalid_entrypoint_and_trigger_setting() {
        let error = ExtensionManifest::parse(
            r#"id="example" version="1.0.0" api_version=1 name="x" description="x" entrypoint="../main.lua""#,
        )
        .unwrap_err();
        assert!(matches!(error, ExtensionError::InvalidManifest(_)));
    }

    #[test]
    fn normalizes_removed_and_invalid_settings() {
        let manifest = manifest();
        let mut settings = ExtensionSettings::with_defaults(&manifest);
        settings
            .values
            .insert("sync_time".into(), SettingValue::Time("invalid".into()));
        settings
            .values
            .insert("removed".into(), SettingValue::String("x".into()));
        let normalized = settings.normalized_for(&manifest);
        assert_eq!(
            normalized.values["sync_time"],
            SettingValue::Time("02:00".into())
        );
        assert!(!normalized.values.contains_key("removed"));
    }

    #[test]
    fn parses_daily_time() {
        assert_eq!(
            parse_daily_time("23:45").unwrap(),
            NaiveTime::from_hms_opt(23, 45, 0).unwrap()
        );
        assert!(parse_daily_time("25:00").is_err());
    }

    #[test]
    fn fires_only_for_an_occurrence_between_checks() {
        let now = Local::now();
        let date = now.date_naive();
        let time = now.time();
        let occurrence = daily_occurrence(date, time).unwrap();
        assert!(should_fire_daily(
            occurrence - chrono::Duration::minutes(1),
            occurrence,
            time
        ));
        assert!(!should_fire_daily(occurrence, occurrence, time));
    }
}
