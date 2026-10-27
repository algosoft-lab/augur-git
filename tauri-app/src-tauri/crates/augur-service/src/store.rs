//! Durable-document storage seam.
//!
//! The service layer reads and writes two JSON documents keyed by name. The
//! desktop frontend backs the trait with the Tauri store plugin; the server
//! frontend uses plain JSON files. Both keep the same document schemas.

use std::path::PathBuf;
use std::sync::Mutex;

/// One named JSON document that survives restarts.
pub trait DocumentStore: Send + Sync {
    /// Read one entry of the document.
    fn get(&self, key: &str) -> Option<serde_json::Value>;
    /// Write one entry of the document. Implementations decide when this
    /// reaches the disk; [`DocumentStore::save`] forces it.
    fn set(&self, key: &str, value: serde_json::Value);
    /// Flush pending writes to the disk.
    fn save(&self);
    /// Absolute path of the backing file, shown in diagnostics.
    fn path(&self) -> Option<PathBuf>;
}

/// A [`DocumentStore`] backed by one JSON file on the disk.
///
/// Every write rewrites the file through a temporary file and a rename, so a
/// crash mid-write leaves the previous content intact. Writes are frequent but
/// small, which the debounced callers of the service layer amortize.
pub struct JsonFileStore {
    path: PathBuf,
    document: Mutex<serde_json::Map<String, serde_json::Value>>,
}

impl JsonFileStore {
    /// Open or create the store file. A malformed file yields an empty
    /// document; the caller is expected to have kept a warning path for that
    /// case at a higher level.
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let document = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default(),
            Err(_) => serde_json::Map::new(),
        };
        Ok(Self {
            path,
            document: Mutex::new(document),
        })
    }

    /// Open or create the store file inside `directory`.
    pub fn open_in(directory: &std::path::Path, file_name: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        Self::open(directory.join(file_name))
    }
}

impl DocumentStore for JsonFileStore {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.document
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
    }

    fn set(&self, key: &str, value: serde_json::Value) {
        {
            let mut document = self
                .document
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            document.insert(key.to_string(), value);
        }
        self.save();
    }

    fn save(&self) {
        let document = self
            .document
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let text = match serde_json::to_string_pretty(&document) {
            Ok(text) => text,
            Err(error) => {
                log::error!("[store] failed to encode {}: {error}", self.path.display());
                return;
            }
        };
        let temporary = self.path.with_extension("tmp");
        if let Err(error) = std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, &self.path))
        {
            log::error!("[store] failed to save {}: {error}", self.path.display());
        }
    }

    fn path(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_store_round_trips_entries_through_the_file() {
        let directory = std::env::temp_dir().join(format!(
            "augur-store-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let path = directory.join("store.json");
        {
            let store = JsonFileStore::open(path.clone()).unwrap();
            assert_eq!(store.get("settings"), None);
            store.set("settings", serde_json::json!({ "theme": "github-dark" }));
        }
        let reopened = JsonFileStore::open(path).unwrap();
        assert_eq!(
            reopened.get("settings").unwrap()["theme"],
            "github-dark"
        );
        std::fs::remove_dir_all(directory).ok();
    }
}
