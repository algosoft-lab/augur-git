//! Per-extension run history persisted outside the installed package.

use std::fs;
use std::path::PathBuf;

use crate::core::extension::{
    ExtensionError, ExtensionRunRecord, MAX_EXTENSION_RUN_HISTORY,
};

fn history_path(extension_id: &str) -> Result<PathBuf, ExtensionError> {
    let root = crate::core::extension::extensions_root()?;
    let data_root = root
        .parent()
        .map(|parent| parent.join("extension-data"))
        .ok_or_else(|| {
        ExtensionError::Io("invalid extension data path".into())
    })?;
    Ok(data_root.join(format!("{extension_id}-history.json")))
}

fn load_run_history(
    extension_id: &str,
) -> Result<Vec<ExtensionRunRecord>, ExtensionError> {
    let path = history_path(extension_id)?;
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            ExtensionError::Io(format!(
                "invalid extension run history: {error}"
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(Vec::new())
        }
        Err(error) => Err(ExtensionError::Io(format!(
            "failed to read extension run history: {error}"
        ))),
    }
}

pub fn append_run_history(
    extension_id: &str,
    record: &ExtensionRunRecord,
) -> Result<(), ExtensionError> {
    let path = history_path(extension_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            ExtensionError::Io(format!(
                "failed to create extension history directory: {error}"
            ))
        })?;
    }
    let mut history = load_run_history(extension_id)?;
    history.push(record.clone());
    if history.len() > MAX_EXTENSION_RUN_HISTORY {
        let keep_from = history.len() - MAX_EXTENSION_RUN_HISTORY;
        history.drain(..keep_from);
    }
    let text = serde_json::to_string_pretty(&history).map_err(|error| {
        ExtensionError::Io(format!(
            "failed to encode extension history: {error}"
        ))
    })?;
    let temp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp, text).map_err(|error| {
        ExtensionError::Io(format!(
            "failed to write extension history: {error}"
        ))
    })?;
    fs::rename(&temp, &path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        ExtensionError::Io(format!(
            "failed to replace extension history: {error}"
        ))
    })
}
