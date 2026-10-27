//! Durable application state, re-exported from the shared service layer,
//! plus the Tauri store adapter that backs its document trait.

use std::sync::Arc;

pub use augur_service::persistence::{
    LoadReport, Persistence, SettingsDocument,
};
pub use augur_service::store::DocumentStore;

use tauri::AppHandle;
use tauri_plugin_store::{Store, StoreExt};

/// Store file holding user preferences.
pub const SETTINGS_FILE: &str = "settings.json";
/// Store file holding session state.
pub const WORKSPACE_FILE: &str = "workspace.json";

/// The stores plus any warning produced while opening them.
pub struct TauriStores {
    pub settings: Arc<dyn DocumentStore>,
    pub workspace: Arc<dyn DocumentStore>,
    pub warnings: Vec<String>,
}

/// Open both documents through the Tauri store plugin, which resolves them
/// under this application's own data directory.
pub fn open_stores(app: &AppHandle) -> TauriStores {
    let mut warnings = Vec::new();
    let settings = open_store(app, SETTINGS_FILE, &mut warnings);
    let workspace = open_store(app, WORKSPACE_FILE, &mut warnings);
    log::info!("[store] opened {SETTINGS_FILE} and {WORKSPACE_FILE}");
    TauriStores {
        settings,
        workspace,
        warnings,
    }
}

fn open_store(
    app: &AppHandle,
    name: &str,
    warnings: &mut Vec<String>,
) -> Arc<dyn DocumentStore> {
    match app.store_builder(name).build() {
        Ok(store) => Arc::new(TauriDocumentStore {
            store,
            path: tauri_plugin_store::resolve_store_path(app, name).ok(),
        }),
        Err(error) => {
            let message = format!("failed to open {name}: {error}");
            log::error!("[store] {message}");
            warnings.push(message);
            Arc::new(NullDocumentStore)
        }
    }
}

struct TauriDocumentStore {
    store: Arc<Store<tauri::Wry>>,
    path: Option<std::path::PathBuf>,
}

impl DocumentStore for TauriDocumentStore {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.store.get(key)
    }

    fn set(&self, key: &str, value: serde_json::Value) {
        self.store.set(key, value);
    }

    fn save(&self) {
        if let Err(error) = self.store.save() {
            log::error!("[store] failed to save the document: {error}");
        }
    }

    fn path(&self) -> Option<std::path::PathBuf> {
        self.path.clone()
    }
}

/// A store that keeps nothing, used when the plugin could not open the file so
/// the session still starts, with defaults and a warning.
struct NullDocumentStore;

impl DocumentStore for NullDocumentStore {
    fn get(&self, _key: &str) -> Option<serde_json::Value> {
        None
    }

    fn set(&self, _key: &str, _value: serde_json::Value) {}

    fn save(&self) {}

    fn path(&self) -> Option<std::path::PathBuf> {
        None
    }
}
