//! Private JSON storage for one extension.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::Value as JsonValue;

use super::api::HostResponse;

/// Storage is scoped by extension id and persisted outside the package tree.
/// The file replacement is atomic so a process interruption cannot leave a
/// partially written JSON document.
#[derive(Clone)]
pub(super) struct ExtensionStorage {
    root: PathBuf,
}

impl ExtensionStorage {
    pub(super) fn new() -> Self {
        let root = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("augur-git")
            .join("extension-data");
        Self { root }
    }

    pub(super) fn get(
        &self,
        extension_id: &str,
        key: Option<String>,
    ) -> Result<HostResponse, String> {
        let storage = self.read(extension_id)?;
        let value = match key {
            Some(key) => storage.get(&key).cloned().unwrap_or(JsonValue::Null),
            None => JsonValue::Object(storage.into_iter().collect()),
        };
        Ok(HostResponse::Json(value))
    }

    pub(super) fn set(
        &self,
        extension_id: &str,
        key: &str,
        value: JsonValue,
    ) -> Result<HostResponse, String> {
        validate_key(key)?;
        let mut storage = self.read(extension_id)?;
        storage.insert(key.to_string(), value);
        self.write(extension_id, &storage)?;
        Ok(HostResponse::Json(serde_json::json!({ "ok": true })))
    }

    pub(super) fn delete(
        &self,
        extension_id: &str,
        key: Option<String>,
    ) -> Result<HostResponse, String> {
        let mut storage = self.read(extension_id)?;
        if let Some(key) = key {
            validate_key(&key)?;
            storage.remove(&key);
        } else {
            storage.clear();
        }
        self.write(extension_id, &storage)?;
        Ok(HostResponse::Json(serde_json::json!({ "ok": true })))
    }

    fn path(&self, extension_id: &str) -> Result<PathBuf, String> {
        validate_extension_id(extension_id)?;
        Ok(self.root.join(format!("{extension_id}.json")))
    }

    fn read(
        &self,
        extension_id: &str,
    ) -> Result<BTreeMap<String, JsonValue>, String> {
        let path = self.path(extension_id)?;
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| {
                format!("extension storage is invalid: {error}")
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(BTreeMap::new())
            }
            Err(error) => {
                Err(format!("failed to read extension storage: {error}"))
            }
        }
    }

    fn write(
        &self,
        extension_id: &str,
        storage: &BTreeMap<String, JsonValue>,
    ) -> Result<(), String> {
        let path = self.path(extension_id)?;
        fs::create_dir_all(&self.root).map_err(|error| {
            format!("failed to create extension storage: {error}")
        })?;
        let text = serde_json::to_string_pretty(storage).map_err(|error| {
            format!("failed to encode extension storage: {error}")
        })?;
        let temp =
            path.with_extension(format!("json.tmp-{}", std::process::id()));
        fs::write(&temp, text).map_err(|error| {
            format!("failed to write extension storage: {error}")
        })?;
        fs::rename(&temp, &path).map_err(|error| {
            let _ = fs::remove_file(&temp);
            format!("failed to replace extension storage: {error}")
        })
    }
}

fn validate_extension_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-')
        })
        || !id.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err("invalid extension id".into());
    }
    Ok(())
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.trim().is_empty() || key.chars().any(char::is_control) {
        return Err(
            "storage key must not be empty or contain control characters"
                .into(),
        );
    }
    Ok(())
}
