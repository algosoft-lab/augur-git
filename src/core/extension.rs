//! Data-only types shared by the Lua extension host and the workspace UI.
//!
//! This module deliberately does not depend on GPUI. Manifest validation,
//! schedule calculations, package identity, and run summaries can therefore
//! be tested without starting the application.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

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

/// A declarative event subscription exposed by an extension package.
///
/// The fields are intentionally represented as a flat manifest structure so
/// TOML packages stay easy to author. Validation below enforces which
/// setting references are valid for each event type.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EventTrigger {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub handler: String,
    #[serde(default)]
    pub time_setting: Option<String>,
    #[serde(default)]
    pub interval_setting: Option<String>,
    #[serde(default)]
    pub debounce_ms: Option<u64>,
}

impl EventTrigger {
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.id)
    }

    pub fn is_schedule(&self) -> bool {
        self.event_type.starts_with("schedule.")
    }

    pub fn is_repository_event(&self) -> bool {
        self.event_type.starts_with("repository.")
            || self.event_type.starts_with("workspace.repository_")
    }

    pub fn debounce_duration_ms(&self) -> u64 {
        self.debounce_ms
            .unwrap_or_else(|| if self.is_repository_event() { 500 } else { 0 })
    }
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
    /// New event declarations. The legacy `daily` field is normalized into
    /// this list so existing packages continue to load.
    #[serde(default)]
    pub events: Vec<EventTrigger>,
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
        if self
            .manual_handler
            .as_deref()
            .is_some_and(|handler| handler.trim().is_empty())
        {
            return Err(ExtensionError::InvalidManifest(
                "manual handler must not be empty".to_string(),
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
        let mut trigger_ids = HashSet::new();
        for trigger in self.event_triggers() {
            validate_event_trigger(self, &trigger, &mut trigger_ids)?;
        }
        Ok(())
    }

    pub fn default_settings(&self) -> BTreeMap<String, SettingValue> {
        self.settings
            .iter()
            .map(|(key, definition)| (key.clone(), definition.default_value()))
            .collect()
    }

    /// Return all event declarations, including legacy daily entries mapped
    /// to the new schedule event type.
    pub fn event_triggers(&self) -> Vec<EventTrigger> {
        let mut triggers = self.events.clone();
        triggers.extend(self.daily.iter().map(|trigger| EventTrigger {
            id: trigger.id.clone(),
            event_type: "schedule.daily".to_string(),
            label: None,
            description: None,
            handler: trigger.handler.clone(),
            time_setting: Some(trigger.time_setting.clone()),
            interval_setting: None,
            debounce_ms: None,
        }));
        triggers
    }
}

const SUPPORTED_EVENT_TYPES: &[&str] = &[
    "schedule.daily",
    "schedule.interval",
    "workspace.repository_opened",
    "workspace.repository_closed",
    "repository.branch_changed",
    "repository.status_changed",
];

fn validate_event_trigger(
    manifest: &ExtensionManifest,
    trigger: &EventTrigger,
    trigger_ids: &mut HashSet<String>,
) -> Result<(), ExtensionError> {
    if trigger.id.trim().is_empty() || trigger.handler.trim().is_empty() {
        return Err(ExtensionError::InvalidManifest(
            "event trigger id and handler must not be empty".to_string(),
        ));
    }
    if !trigger_ids.insert(trigger.id.clone()) {
        return Err(ExtensionError::InvalidManifest(format!(
            "duplicate event trigger id: {}",
            trigger.id
        )));
    }
    if !SUPPORTED_EVENT_TYPES.contains(&trigger.event_type.as_str()) {
        return Err(ExtensionError::InvalidManifest(format!(
            "unsupported event type: {}",
            trigger.event_type
        )));
    }
    if let Some(label) = &trigger.label {
        if label.trim().is_empty() {
            return Err(ExtensionError::InvalidManifest(format!(
                "event trigger {} label must not be empty",
                trigger.id
            )));
        }
    }
    if let Some(description) = &trigger.description {
        if description.trim().is_empty() {
            return Err(ExtensionError::InvalidManifest(format!(
                "event trigger {} description must not be empty",
                trigger.id
            )));
        }
    }
    if trigger.debounce_ms.unwrap_or(0) > 60_000 {
        return Err(ExtensionError::InvalidManifest(format!(
            "event trigger {} debounce_ms must not exceed 60000",
            trigger.id
        )));
    }
    match trigger.event_type.as_str() {
        "schedule.daily" => {
            let Some(setting_key) = trigger.time_setting.as_deref() else {
                return Err(ExtensionError::InvalidManifest(format!(
                    "daily trigger {} requires time_setting",
                    trigger.id
                )));
            };
            if !matches!(
                manifest.settings.get(setting_key),
                Some(SettingDefinition::Time { .. })
            ) {
                return Err(ExtensionError::InvalidManifest(format!(
                    "daily trigger {} references a non-time setting: {}",
                    trigger.id, setting_key
                )));
            }
            if trigger.interval_setting.is_some() {
                return Err(ExtensionError::InvalidManifest(format!(
                    "daily trigger {} must not define interval_setting",
                    trigger.id
                )));
            }
        }
        "schedule.interval" => {
            let Some(setting_key) = trigger.interval_setting.as_deref() else {
                return Err(ExtensionError::InvalidManifest(format!(
                    "interval trigger {} requires interval_setting",
                    trigger.id
                )));
            };
            let Some(SettingDefinition::Integer {
                default, min, max, ..
            }) = manifest.settings.get(setting_key)
            else {
                return Err(ExtensionError::InvalidManifest(format!(
                    "interval trigger {} references a non-integer setting: {}",
                    trigger.id, setting_key
                )));
            };
            if *default <= 0
                || min.is_some_and(|minimum| minimum <= 0)
                || max.is_some_and(|maximum| maximum <= 0)
            {
                return Err(ExtensionError::InvalidManifest(format!(
                    "interval trigger {} requires a positive integer setting: {}",
                    trigger.id, setting_key
                )));
            }
            if trigger.time_setting.is_some() {
                return Err(ExtensionError::InvalidManifest(format!(
                    "interval trigger {} must not define time_setting",
                    trigger.id
                )));
            }
        }
        _ => {
            if trigger.time_setting.is_some()
                || trigger.interval_setting.is_some()
            {
                return Err(ExtensionError::InvalidManifest(format!(
                    "repository trigger {} must not reference schedule settings",
                    trigger.id
                )));
            }
        }
    }
    Ok(())
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

/// Return the validated destination path for an installed local extension.
/// Keeping ID validation in this helper prevents callers from constructing a
/// path that escapes the extension root.
pub fn installed_extension_path(id: &str) -> Result<PathBuf, ExtensionError> {
    validate_extension_id(id)?;
    Ok(extensions_root()?.join(id))
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

/// Remove an installed local package. Bundled packages are not represented by
/// a directory and therefore cannot be removed through this API.
pub fn uninstall_local_package(id: &str) -> Result<(), ExtensionError> {
    let destination = installed_extension_path(id)?;
    let metadata = match fs::symlink_metadata(&destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => {
            return Err(ExtensionError::Io(format!(
                "failed to inspect extension package: {error}"
            )));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(ExtensionError::SymlinkNotAllowed(destination));
    }
    if !metadata.is_dir() {
        return Err(ExtensionError::InvalidManifest(
            "installed extension path is not a directory".into(),
        ));
    }
    fs::remove_dir_all(&destination).map_err(|error| {
        ExtensionError::Io(format!(
            "failed to uninstall extension package: {error}"
        ))
    })
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
    let digest = hasher.finalize();
    let mut fingerprint = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        fingerprint.push(HEX[(byte >> 4) as usize] as char);
        fingerprint.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(fingerprint)
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
    /// Event subscriptions enabled by the user. A missing legacy `enabled`
    /// field is migrated once by `normalized_for`.
    #[serde(default)]
    pub subscribed_triggers: BTreeSet<String>,
    #[serde(rename = "enabled", default, skip_serializing)]
    legacy_enabled: Option<bool>,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub values: BTreeMap<String, SettingValue>,
    #[serde(default)]
    pub last_seen_fingerprint: Option<String>,
    #[serde(default)]
    pub last_scheduled_date: Option<String>,
    #[serde(default)]
    pub last_event_occurrences: BTreeMap<String, String>,
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
        if let Some(legacy_enabled) = normalized.legacy_enabled.take() {
            normalized.subscribed_triggers = if legacy_enabled {
                manifest
                    .event_triggers()
                    .into_iter()
                    .map(|trigger| trigger.id)
                    .collect()
            } else {
                BTreeSet::new()
            };
        }
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
        let trigger_ids = manifest
            .event_triggers()
            .into_iter()
            .map(|trigger| trigger.id)
            .collect::<BTreeSet<_>>();
        normalized
            .subscribed_triggers
            .retain(|id| trigger_ids.contains(id));
        normalized
            .last_event_occurrences
            .retain(|id, _| trigger_ids.contains(id));
        if let Some(legacy_date) = normalized.last_scheduled_date.clone() {
            if let Some(trigger) = manifest
                .event_triggers()
                .into_iter()
                .find(|trigger| trigger.event_type == "schedule.daily")
            {
                normalized
                    .last_event_occurrences
                    .entry(trigger.id)
                    .or_insert(legacy_date);
            }
        }
        normalized
    }

    pub fn is_subscribed(&self, trigger_id: &str) -> bool {
        self.subscribed_triggers.contains(trigger_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExtensionRunTrigger {
    Manual,
    Schedule {
        trigger_id: String,
        #[serde(default = "default_schedule_event_type")]
        event_type: String,
    },
    Repository {
        trigger_id: String,
        event_type: String,
    },
}

fn default_schedule_event_type() -> String {
    "schedule.daily".to_string()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RepositoryRunResult {
    Success { summary: String },
    Failed { code: String, summary: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtensionRunRecord {
    pub run_id: u64,
    pub trigger: ExtensionRunTrigger,
    pub started_at: String,
    pub finished_at: String,
    pub repositories: Vec<RepositoryRunRecord>,
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

/// Return the first daily occurrence in the open/closed interval
/// `(last_check, now]`.
///
/// The occurrence date is needed by the scheduler when an application wakes
/// after midnight: recording `now`'s date could incorrectly suppress the
/// later occurrence for the current local day.
pub fn daily_occurrence_between(
    last_check: DateTime<Local>,
    now: DateTime<Local>,
    time: NaiveTime,
) -> Option<DateTime<Local>> {
    if now <= last_check {
        return None;
    }
    let mut date = last_check.date_naive();
    let final_date = now.date_naive();
    loop {
        if date > final_date {
            break;
        }
        if let Some(occurrence) = daily_occurrence(date, time)
            && occurrence > last_check
            && occurrence <= now
        {
            return Some(occurrence);
        }
        let Some(next) = date.succ_opt() else {
            break;
        };
        date = next;
    }
    None
}

/// Whether a daily trigger should fire between two local instants.
#[allow(dead_code)]
pub fn should_fire_daily(
    last_check: DateTime<Local>,
    now: DateTime<Local>,
    time: NaiveTime,
) -> bool {
    daily_occurrence_between(last_check, now, time).is_some()
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
        assert_eq!(manifest.event_triggers().len(), 1);
        assert_eq!(manifest.event_triggers()[0].event_type, "schedule.daily");
    }

    #[test]
    fn validates_new_event_manifest_types() {
        let parsed = ExtensionManifest::parse(
            r#"
id = "events"
version = "1.0.0"
api_version = 1
name = "Events"
description = "Event test"

[settings.interval]
type = "integer"
label = "Interval"
default = 5
min = 1

[[events]]
id = "timer"
type = "schedule.interval"
handler = "on_timer"
interval_setting = "interval"

[[events]]
id = "status"
type = "repository.status_changed"
label = "Status"
handler = "on_status"
debounce_ms = 250
"#,
        )
        .expect("event manifest should validate");
        let triggers = parsed.event_triggers();
        assert_eq!(triggers.len(), 2);
        assert_eq!(triggers[0].event_type, "schedule.interval");
        assert_eq!(triggers[1].debounce_duration_ms(), 250);
    }

    #[test]
    fn rejects_invalid_event_declarations() {
        let error = ExtensionManifest::parse(
            r#"
id = "events"
version = "1.0.0"
api_version = 1
name = "Events"
description = "Event test"

[[events]]
id = "bad"
type = "repository.status_changed"
handler = "on_status"
time_setting = "missing"
"#,
        )
        .unwrap_err();
        assert!(matches!(error, ExtensionError::InvalidManifest(_)));
    }

    #[test]
    fn interval_triggers_require_positive_settings() {
        let error = ExtensionManifest::parse(
            r#"
id = "events"
version = "1.0.0"
api_version = 1
name = "Events"
description = "Event test"

[settings.interval]
type = "integer"
label = "Interval"
default = 0

[[events]]
id = "timer"
type = "schedule.interval"
handler = "on_timer"
interval_setting = "interval"
"#,
        )
        .unwrap_err();
        assert!(matches!(error, ExtensionError::InvalidManifest(_)));
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
    fn rejects_empty_or_duplicate_handlers() {
        let empty = ExtensionManifest::parse(
            r#"id="example" version="1.0.0" api_version=1 name="x" description="x" manual_handler="""#,
        )
        .unwrap_err();
        assert!(matches!(empty, ExtensionError::InvalidManifest(_)));
        let duplicate = ExtensionManifest::parse(
            r#"
id = "example"
version = "1.0.0"
api_version = 1
name = "x"
description = "x"
[settings.when]
type = "time"
label = "When"
default = "02:00"
[[daily]]
id = "same"
time_setting = "when"
handler = "run"
[[daily]]
id = "same"
time_setting = "when"
handler = "run"
"#,
        )
        .unwrap_err();
        assert!(matches!(duplicate, ExtensionError::InvalidManifest(_)));
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
    fn migrates_legacy_enabled_to_existing_event_subscriptions() {
        let settings: ExtensionSettings = serde_json::from_str(
            r#"{"enabled":true,"trusted":true,"values":{}}"#,
        )
        .expect("legacy settings");
        let normalized = settings.normalized_for(&manifest());
        assert!(normalized.is_subscribed("daily"));
        let encoded = serde_json::to_string(&normalized).unwrap();
        assert!(!encoded.contains("\"enabled\""));
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
        assert_eq!(
            daily_occurrence_between(
                occurrence - chrono::Duration::minutes(1),
                occurrence,
                time,
            ),
            Some(occurrence)
        );
        assert!(should_fire_daily(
            occurrence - chrono::Duration::minutes(1),
            occurrence,
            time
        ));
        assert!(!should_fire_daily(occurrence, occurrence, time));
    }
}
