//! Secret backends: the operating system keychain, a consent-only plaintext
//! file, and process memory.
//!
//! Only the keychain is used without asking. A machine without a reachable
//! Secret Service, a sandbox that blocks it, or a server started by systemd
//! without a session bus all report that fact instead of silently keeping the
//! password somewhere weaker: the plaintext file has to be enabled in the
//! settings page, and every surface that shows such an entry says so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::model::{SecretBackendStatus, SecretStorage};

/// One place a secret can live.
pub trait SecretStore: Send + Sync {
    fn storage(&self) -> SecretStorage;
    /// Whether writing to this backend can work right now.
    fn status(&self) -> SecretBackendStatus;
    fn get(&self, handle: &str) -> Result<Option<String>, String>;
    fn set(&self, handle: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, handle: &str) -> Result<(), String>;
}

fn status_of(storage: SecretStorage, availability: Result<(), String>) -> SecretBackendStatus {
    SecretBackendStatus {
        storage,
        available: availability.is_ok(),
        detail: availability.err(),
    }
}

/// The operating system's credential store.
///
/// The availability probe reads a key that is never written, so it distinguishes
/// "no entry" (the backend works) from "the backend cannot be reached" without
/// touching a real secret.
pub struct KeychainSecretStore {
    service: String,
    probe: Mutex<Option<Result<(), String>>>,
}

impl KeychainSecretStore {
    pub const SERVICE: &'static str = "augur-git";

    pub fn new() -> Self {
        Self {
            service: Self::SERVICE.to_string(),
            probe: Mutex::new(None),
        }
    }

    fn entry(&self, handle: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, handle).map_err(describe_keychain_error)
    }

    fn probe(&self) -> Result<(), String> {
        if let Some(cached) = self
            .probe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            return cached.clone();
        }
        let result = match keyring::Entry::new(&self.service, "augur-availability-probe") {
            Ok(entry) => match entry.get_password() {
                Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(describe_keychain_error(error)),
            },
            Err(error) => Err(describe_keychain_error(error)),
        };
        *self
            .probe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result.clone());
        result
    }
}

impl Default for KeychainSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeychainSecretStore {
    fn storage(&self) -> SecretStorage {
        SecretStorage::Keychain
    }

    fn status(&self) -> SecretBackendStatus {
        status_of(self.storage(), self.probe())
    }

    fn get(&self, handle: &str) -> Result<Option<String>, String> {
        match self.entry(handle)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(describe_keychain_error(error)),
        }
    }

    fn set(&self, handle: &str, secret: &str) -> Result<(), String> {
        let entry = self.entry(handle)?;
        entry
            .set_password(secret)
            .map_err(describe_keychain_error)?;
        // A successful write proves the backend is reachable; a cached failure
        // from a session started before the keyring was unlocked must not keep
        // reporting the backend as unavailable.
        *self
            .probe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Ok(()));
        Ok(())
    }

    fn delete(&self, handle: &str) -> Result<(), String> {
        match self.entry(handle)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(describe_keychain_error(error)),
        }
    }
}

/// Turn a keychain error into a sentence the settings page can show.
///
/// The platform messages differ by distribution and locale, so the stable
/// classification is the sentence, not the raw error.
fn describe_keychain_error(error: keyring::Error) -> String {
    match error {
        keyring::Error::NoEntry => "No stored entry".into(),
        keyring::Error::Ambiguous(_) => "The credential store matched more than one entry".into(),
        keyring::Error::BadEncoding(_) => "The stored secret is not valid UTF-8".into(),
        other => {
            if cfg!(target_os = "linux") {
                format!(
                    "{other} (a D-Bus session and a Secret Service provider such as GNOME Keyring or KWallet are required)"
                )
            } else {
                other.to_string()
            }
        }
    }
}

/// A `0600` JSON file beside the settings document.
///
/// This backend exists for machines with no keychain. Every read and write goes
/// through a lock and a temporary file plus rename, so a crash mid-write cannot
/// truncate the file.
pub struct PlaintextFileStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl PlaintextFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<BTreeMap<String, String>>(&text).ok())
            .unwrap_or_default()
    }

    fn write(&self, values: &BTreeMap<String, String>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = serde_json::to_string_pretty(values).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        write_private(&temporary, &text)?;
        std::fs::rename(&temporary, &self.path).map_err(|error| error.to_string())
    }
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(text.as_bytes())
        .map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    // Windows inherits the directory's ACL; the file is still outside the
    // repository and never rendered by the web interface.
    std::fs::write(path, text).map_err(|error| error.to_string())
}

impl SecretStore for PlaintextFileStore {
    fn storage(&self) -> SecretStorage {
        SecretStorage::Plaintext
    }

    fn status(&self) -> SecretBackendStatus {
        let availability = match self.path.parent() {
            Some(parent) => std::fs::create_dir_all(parent).map_err(|error| error.to_string()),
            None => Ok(()),
        };
        status_of(self.storage(), availability)
    }

    fn get(&self, handle: &str) -> Result<Option<String>, String> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(self.read().get(handle).cloned())
    }

    fn set(&self, handle: &str, secret: &str) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut values = self.read();
        values.insert(handle.to_string(), secret.to_string());
        self.write(&values)
    }

    fn delete(&self, handle: &str) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut values = self.read();
        if values.remove(handle).is_none() {
            return Ok(());
        }
        self.write(&values)
    }
}

/// Secrets that live only as long as this process.
#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<BTreeMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        self.values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

impl SecretStore for MemorySecretStore {
    fn storage(&self) -> SecretStorage {
        SecretStorage::Session
    }

    fn status(&self) -> SecretBackendStatus {
        status_of(self.storage(), Ok(()))
    }

    fn get(&self, handle: &str) -> Result<Option<String>, String> {
        Ok(self
            .values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(handle)
            .cloned())
    }

    fn set(&self, handle: &str, secret: &str) -> Result<(), String> {
        self.values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, handle: &str) -> Result<(), String> {
        self.values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(handle);
        Ok(())
    }
}

/// The three backends the vault routes between.
#[derive(Clone)]
pub struct SecretStores {
    keychain: Arc<dyn SecretStore>,
    plaintext: Arc<dyn SecretStore>,
    session: Arc<dyn SecretStore>,
    plaintext_path: Option<PathBuf>,
}

impl SecretStores {
    pub fn new(keychain: Arc<dyn SecretStore>, plaintext: Arc<dyn SecretStore>) -> Self {
        Self {
            keychain,
            plaintext,
            session: Arc::new(MemorySecretStore::new()),
            plaintext_path: None,
        }
    }

    /// Every backend in memory: the desktop tests and the browser-free server
    /// tests use this so no real keychain is touched.
    pub fn memory_only() -> Self {
        Self::new(
            Arc::new(MemorySecretStore::new()),
            Arc::new(MemorySecretStore::new()),
        )
    }

    /// The keychain, plus the plaintext file only when one is configured.
    pub fn with_plaintext(keychain: Arc<dyn SecretStore>, plaintext_path: Option<PathBuf>) -> Self {
        let plaintext: Arc<dyn SecretStore> = match &plaintext_path {
            Some(path) => Arc::new(PlaintextFileStore::new(path)),
            None => Arc::new(MemorySecretStore::new()),
        };
        Self {
            keychain,
            plaintext,
            session: Arc::new(MemorySecretStore::new()),
            plaintext_path,
        }
    }

    fn backend(&self, storage: SecretStorage) -> &Arc<dyn SecretStore> {
        match storage {
            SecretStorage::Session => &self.session,
            SecretStorage::Keychain => &self.keychain,
            SecretStorage::Plaintext => &self.plaintext,
        }
    }

    pub fn get(&self, storage: SecretStorage, handle: &str) -> Result<Option<String>, String> {
        self.backend(storage).get(handle)
    }

    pub fn set(&self, storage: SecretStorage, handle: &str, secret: &str) -> Result<(), String> {
        self.backend(storage).set(handle, secret)
    }

    pub fn delete(&self, storage: SecretStorage, handle: &str) -> Result<(), String> {
        self.backend(storage).delete(handle)
    }

    pub fn status(&self, storage: SecretStorage) -> SecretBackendStatus {
        self.backend(storage).status()
    }

    pub fn available(&self, storage: SecretStorage) -> bool {
        self.backend(storage).status().available
    }

    pub fn statuses(&self) -> Vec<SecretBackendStatus> {
        [
            SecretStorage::Keychain,
            SecretStorage::Plaintext,
            SecretStorage::Session,
        ]
        .into_iter()
        .map(|storage| self.status(storage))
        .collect()
    }

    /// Path of the plaintext file, when this set has one.
    pub fn plaintext_path(&self) -> Option<PathBuf> {
        self.plaintext_path.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores_with_file(path: &Path) -> SecretStores {
        SecretStores::with_plaintext(Arc::new(MemorySecretStore::new()), Some(path.to_path_buf()))
    }

    #[test]
    fn every_backend_round_trips_a_secret() {
        let stores = SecretStores::memory_only();
        for storage in [
            SecretStorage::Session,
            SecretStorage::Keychain,
            SecretStorage::Plaintext,
        ] {
            assert_eq!(stores.get(storage, "handle").unwrap(), None);
            stores.set(storage, "handle", "s3cret").unwrap();
            assert_eq!(
                stores.get(storage, "handle").unwrap().as_deref(),
                Some("s3cret")
            );
            stores.delete(storage, "handle").unwrap();
            assert_eq!(stores.get(storage, "handle").unwrap(), None);
        }
    }

    #[test]
    fn the_plaintext_file_keeps_other_entries_when_one_is_deleted() {
        let directory = std::env::temp_dir().join(format!(
            "augur-credential-file-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let path = directory.join("credential-secrets.json");
        let stores = stores_with_file(&path);
        stores.set(SecretStorage::Plaintext, "a", "one").unwrap();
        stores.set(SecretStorage::Plaintext, "b", "two").unwrap();
        stores.delete(SecretStorage::Plaintext, "a").unwrap();
        assert_eq!(
            stores
                .get(SecretStorage::Plaintext, "b")
                .unwrap()
                .as_deref(),
            Some("two")
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("one"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(unix)]
    #[test]
    fn the_plaintext_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "augur-credential-mode-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let path = directory.join("credential-secrets.json");
        let stores = stores_with_file(&path);
        stores.set(SecretStorage::Plaintext, "a", "one").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
