//! The credential vault: the document, the secrets, and the lookups both
//! frontends share.
//!
//! Structural changes (saving, deleting, changing settings) are written through
//! immediately so the optional credential helper in a terminal sees the same
//! entries the application does. Usage bookkeeping is memory-only and flushed on
//! shutdown, because a push should not rewrite the settings file several times.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::credentials::model::{
    CREDENTIALS_DOCUMENT_KEY, CredentialDocument, CredentialEntry, CredentialEntryView,
    CredentialInput, CredentialScope, CredentialSettings, CredentialSettingsPatch,
    CredentialSnapshot, ImportReport, SecretStorage,
};
use crate::credentials::prompt::{RemoteTarget, normalize_path_prefix, normalize_typed_host};
use crate::credentials::secret::SecretStores;
use crate::store::DocumentStore;

/// How long a rejected entry is left out of automatic answers. Git retries a
/// rejected credential immediately, so this only has to cover one operation.
pub const FAILURE_COOLDOWN_SECONDS: u64 = 120;

struct VaultState {
    document: CredentialDocument,
    /// Entries the user chose to keep for this session only. They are never
    /// written to the document, so a restart cannot leave a dangling row.
    session: Vec<CredentialEntry>,
    dirty: bool,
}

/// One entry that can answer a prompt.
#[derive(Clone, Debug)]
pub struct ResolvedCredential {
    pub entry: CredentialEntry,
    /// `None` when only the username is remembered.
    pub secret: Option<String>,
}

/// Current wall-clock seconds, saturating at the epoch.
pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub struct CredentialVault {
    state: Mutex<VaultState>,
    store: Arc<dyn DocumentStore>,
    secrets: SecretStores,
}

impl CredentialVault {
    /// Load the document and report anything that had to be discarded.
    pub fn load(store: Arc<dyn DocumentStore>, secrets: SecretStores) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let document = match store.get(CREDENTIALS_DOCUMENT_KEY) {
            Some(value) => match serde_json::from_value::<CredentialDocument>(value) {
                Ok(mut document) => {
                    document.normalize();
                    document
                }
                Err(error) => {
                    warnings.push(format!(
                        "credentials could not be read and were reset: {error}"
                    ));
                    CredentialDocument::default()
                }
            },
            None => CredentialDocument::default(),
        };
        (
            Self {
                state: Mutex::new(VaultState {
                    document,
                    session: Vec::new(),
                    dirty: false,
                }),
                store,
                secrets,
            },
            warnings,
        )
    }

    pub fn secrets(&self) -> SecretStores {
        self.secrets.clone()
    }

    fn with<R>(&self, body: impl FnOnce(&mut VaultState) -> R) -> R {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        body(&mut guard)
    }

    pub fn settings(&self) -> CredentialSettings {
        self.with(|state| state.document.settings.clone())
    }

    /// Whether stored credentials may answer a prompt without asking.
    pub fn auto_fill(&self) -> bool {
        self.with(|state| state.document.settings.auto_fill)
    }

    pub fn snapshot(&self) -> CredentialSnapshot {
        let (settings, entries) = self.with(|state| {
            let mut entries: Vec<CredentialEntry> = state.document.entries.clone();
            entries.extend(state.session.iter().cloned());
            entries.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.host.cmp(&right.host))
            });
            (state.document.settings.clone(), entries)
        });
        let views = entries
            .into_iter()
            .map(|entry| {
                let secret_present = self
                    .secrets
                    .get(entry.storage, &entry.secret_handle())
                    .ok()
                    .flatten()
                    .is_some();
                CredentialEntryView {
                    scope_label: entry.scope_label(),
                    failing: entry
                        .last_failure_at
                        .is_some_and(|failed| failed > entry.updated_at),
                    secret_present,
                    entry,
                }
            })
            .collect();
        let statuses = self.secrets.statuses();
        let available = |storage: SecretStorage| {
            statuses
                .iter()
                .find(|status| status.storage == storage)
                .is_some_and(|status| status.available)
        };
        CredentialSnapshot {
            keychain_available: available(SecretStorage::Keychain),
            plaintext_available: available(SecretStorage::Plaintext) && settings.allow_plaintext,
            plaintext_path: self
                .secrets
                .plaintext_path()
                .map(|path| path.to_string_lossy().into_owned()),
            helper_active: settings.use_helper,
            settings,
            entries: views,
            backends: statuses,
        }
    }

    pub fn update_settings(
        &self,
        patch: CredentialSettingsPatch,
    ) -> Result<CredentialSnapshot, String> {
        self.with(|state| {
            if let Some(value) = patch.auto_fill {
                state.document.settings.auto_fill = value;
            }
            if let Some(value) = patch.default_storage {
                state.document.settings.default_storage = value;
            }
            if let Some(value) = patch.allow_plaintext {
                state.document.settings.allow_plaintext = value;
                if !value {
                    self.remove_plaintext_entries(state);
                }
            }
            if let Some(value) = patch.use_helper {
                state.document.settings.use_helper = value;
            }
            state.document.normalize();
            state.dirty = true;
            self.persist(state);
        });
        Ok(self.snapshot())
    }

    /// Drop every entry whose secret lives in the plaintext file.
    fn remove_plaintext_entries(&self, state: &mut VaultState) {
        let removed: Vec<CredentialEntry> = state
            .document
            .entries
            .iter()
            .filter(|entry| entry.storage == SecretStorage::Plaintext)
            .cloned()
            .collect();
        for entry in &removed {
            let _ = self
                .secrets
                .delete(SecretStorage::Plaintext, &entry.secret_handle());
        }
        state
            .document
            .entries
            .retain(|entry| entry.storage != SecretStorage::Plaintext);
        if state.document.settings.default_storage == SecretStorage::Plaintext {
            state.document.settings.default_storage = SecretStorage::Keychain;
        }
    }

    /// Create or update one entry.
    pub fn save(&self, input: CredentialInput, now: u64) -> Result<CredentialSnapshot, String> {
        let protocol = input.protocol.trim().to_ascii_lowercase();
        if protocol != "http" && protocol != "https" {
            return Err("Only http and https remotes can be stored".into());
        }
        let host =
            normalize_typed_host(&input.host).ok_or_else(|| "A host is required".to_string())?;
        let username = input.username.trim().to_string();
        if username.is_empty() || username.contains(['\n', '\r', '\0']) {
            return Err("A username is required".into());
        }
        let path_prefix = input.path_prefix.as_deref().and_then(normalize_path_prefix);
        if let Some(password) = input.password.as_deref()
            && password.contains(['\n', '\r', '\0'])
        {
            return Err("The password contains a character Git cannot accept".into());
        }
        let storage = input.storage;
        self.ensure_storage_allowed(storage)?;

        let scope = match input.scope {
            CredentialScope::Global => CredentialScope::Global,
            CredentialScope::Repository { location, path } => {
                CredentialScope::Repository { location, path }
            }
        };

        let existing = self.with(|state| {
            input
                .id
                .as_deref()
                .and_then(|id| {
                    state
                        .document
                        .entries
                        .iter()
                        .chain(state.session.iter())
                        .find(|entry| entry.id == id)
                        .cloned()
                })
                .or_else(|| {
                    state
                        .document
                        .entries
                        .iter()
                        .chain(state.session.iter())
                        .find(|entry| {
                            entry.protocol == protocol
                                && entry.host == host
                                && entry.username == username
                                && entry.path_prefix == path_prefix
                                && entry.scope == scope
                        })
                        .cloned()
                })
        });

        let id = existing
            .as_ref()
            .map(|entry| entry.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let entry = CredentialEntry {
            id: id.clone(),
            protocol,
            host,
            path_prefix,
            username,
            scope,
            storage,
            created_at: existing.as_ref().map_or(now, |entry| entry.created_at),
            updated_at: now,
            last_used_at: existing.as_ref().and_then(|entry| entry.last_used_at),
            last_failure_at: None,
        };
        let handle = entry.secret_handle();

        if let Some(password) = input.password.as_deref().filter(|value| !value.is_empty()) {
            self.secrets.set(storage, &handle, password)?;
        } else if existing
            .as_ref()
            .is_some_and(|previous| previous.storage != storage)
        {
            return Err("Changing the storage requires the password again".into());
        }
        if let Some(previous) = existing.as_ref()
            && previous.storage != storage
        {
            let _ = self
                .secrets
                .delete(previous.storage, &previous.secret_handle());
        }

        self.with(|state| {
            Self::remove_entry_by_id(state, &id);
            if entry.storage.is_persistent() {
                state.document.entries.push(entry.clone());
            } else {
                state.session.push(entry.clone());
            }
            state.document.normalize();
            state.dirty = true;
            self.persist(state);
        });
        Ok(self.snapshot())
    }

    fn ensure_storage_allowed(&self, storage: SecretStorage) -> Result<(), String> {
        match storage {
            SecretStorage::Session => Ok(()),
            SecretStorage::Keychain => {
                let status = self.secrets.status(SecretStorage::Keychain);
                if status.available {
                    Ok(())
                } else {
                    Err(status
                        .detail
                        .unwrap_or_else(|| "The system keychain is unavailable".into()))
                }
            }
            SecretStorage::Plaintext => {
                let settings = self.settings();
                if !settings.allow_plaintext {
                    return Err(
                        "Saving to an unencrypted file has to be enabled in the settings first"
                            .into(),
                    );
                }
                let status = self.secrets.status(SecretStorage::Plaintext);
                if status.available {
                    Ok(())
                } else {
                    Err(status
                        .detail
                        .unwrap_or_else(|| "The credential file is not writable".into()))
                }
            }
        }
    }

    fn remove_entry_by_id(state: &mut VaultState, id: &str) {
        state.document.entries.retain(|entry| entry.id != id);
        state.session.retain(|entry| entry.id != id);
    }

    /// Forget one entry and its secret.
    pub fn delete(&self, id: &str) -> Result<CredentialSnapshot, String> {
        let removed = self.with(|state| {
            let storage = state
                .document
                .entries
                .iter()
                .chain(state.session.iter())
                .find(|entry| entry.id == id)
                .map(|entry| entry.storage);
            Self::remove_entry_by_id(state, id);
            state.dirty = true;
            self.persist(state);
            storage
        });
        if let Some(storage) = removed {
            self.secrets.delete(storage, &format!("credential-{id}"))?;
        }
        Ok(self.snapshot())
    }

    /// The best entry for a remote, if one may answer right now.
    ///
    /// Entries rejected within [`FAILURE_COOLDOWN_SECONDS`] are skipped so a
    /// stale password cannot make Git retry the same value forever.
    pub fn resolve(&self, target: &RemoteTarget, now: u64) -> Option<ResolvedCredential> {
        let candidates = self.with(|state| {
            let mut candidates: Vec<(i64, CredentialEntry)> = state
                .document
                .entries
                .iter()
                .chain(state.session.iter())
                .filter(|entry| {
                    entry
                        .last_failure_at
                        .is_none_or(|failed| now.saturating_sub(failed) > FAILURE_COOLDOWN_SECONDS)
                })
                .filter_map(|entry| {
                    super::prompt::match_score(entry, target).map(|score| (score, entry.clone()))
                })
                .collect();
            candidates.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| right.1.updated_at.cmp(&left.1.updated_at))
            });
            candidates
        });
        if candidates.is_empty() {
            log::debug!(
                "[credentials] no entry matches {}://{}{}",
                target.protocol,
                target.host,
                target
                    .path
                    .as_deref()
                    .map(|path| format!("/{path}"))
                    .unwrap_or_default()
            );
            return None;
        }
        for (_, entry) in candidates {
            match self.secrets.get(entry.storage, &entry.secret_handle()) {
                Ok(Some(secret)) if !secret.is_empty() => {
                    self.mark_used(&entry.id, now);
                    return Some(ResolvedCredential {
                        entry,
                        secret: Some(secret),
                    });
                }
                Ok(_) if target.username.is_some() => {
                    // The username matched, so pre-filling it is still useful
                    // even when no password was kept.
                    log::debug!(
                        "[credentials] entry {} matches but holds no password",
                        entry.id
                    );
                    return Some(ResolvedCredential {
                        entry,
                        secret: None,
                    });
                }
                Ok(_) => {
                    log::debug!(
                        "[credentials] entry {} matches but holds no password",
                        entry.id
                    );
                    continue;
                }
                Err(reason) => {
                    log::warn!(
                        "[credentials] entry {} could not be read from {}: {reason}",
                        entry.id,
                        entry.storage.as_str()
                    );
                    continue;
                }
            }
        }
        None
    }

    /// Record that an entry answered a prompt.
    pub fn mark_used(&self, id: &str, now: u64) {
        self.with(|state| {
            for entry in state
                .document
                .entries
                .iter_mut()
                .chain(state.session.iter_mut())
            {
                if entry.id == id {
                    entry.last_used_at = Some(now);
                    state.dirty = true;
                }
            }
        });
    }

    /// Record that the remote refused the stored value.
    pub fn mark_failed(&self, id: &str, now: u64) {
        self.with(|state| {
            for entry in state
                .document
                .entries
                .iter_mut()
                .chain(state.session.iter_mut())
            {
                if entry.id == id {
                    entry.last_failure_at = Some(now);
                    state.dirty = true;
                }
            }
        });
    }

    /// Write the document when anything changed since the last write.
    pub fn flush(&self) {
        self.with(|state| {
            if state.dirty {
                self.persist(state);
                state.dirty = false;
            }
        });
    }

    fn persist(&self, state: &mut VaultState) {
        match serde_json::to_value(&state.document) {
            Ok(value) => {
                self.store.set(CREDENTIALS_DOCUMENT_KEY, value);
                self.store.save();
                state.dirty = false;
            }
            Err(error) => log::error!("[credentials] the document could not be encoded: {error}"),
        }
    }

    /// Import the file Git's own `store` helper writes.
    ///
    /// The source is plaintext, so the entries are moved into the keychain when
    /// it works. The file itself is left alone: deleting another tool's
    /// configuration is not this feature's decision.
    pub fn import_git_credentials(&self, text: &str, now: u64) -> (ImportReport, Vec<String>) {
        let mut warnings = Vec::new();
        let storage = if self.secrets.available(SecretStorage::Keychain) {
            SecretStorage::Keychain
        } else if self.settings().allow_plaintext {
            SecretStorage::Plaintext
        } else {
            warnings.push(
                "Nothing could be imported: the system keychain is unavailable and the unencrypted file is disabled"
                    .to_string(),
            );
            return (
                ImportReport {
                    source: String::new(),
                    imported: 0,
                    skipped: 0,
                },
                warnings,
            );
        };
        let mut report = ImportReport {
            source: String::new(),
            imported: 0,
            skipped: 0,
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((protocol, rest)) = line.split_once("://") else {
                report.skipped += 1;
                continue;
            };
            let protocol = protocol.to_ascii_lowercase();
            if protocol != "http" && protocol != "https" {
                report.skipped += 1;
                continue;
            }
            let authority = rest.split('/').next().unwrap_or(rest);
            let Some((userinfo, host)) = authority.rsplit_once('@') else {
                report.skipped += 1;
                continue;
            };
            let (username, password) = match userinfo.split_once(':') {
                Some((username, password)) => (username, password),
                None => (userinfo, ""),
            };
            let Some(host) = normalize_typed_host(host) else {
                report.skipped += 1;
                continue;
            };
            if username.is_empty() || password.is_empty() {
                report.skipped += 1;
                continue;
            }
            let input = CredentialInput {
                id: None,
                protocol,
                host,
                path_prefix: None,
                username: username.to_string(),
                password: Some(password.to_string()),
                scope: CredentialScope::Global,
                storage,
            };
            match self.save(input, now) {
                Ok(_) => report.imported += 1,
                Err(error) => {
                    report.skipped += 1;
                    warnings.push(error);
                }
            }
        }
        (report, warnings)
    }

    /// One entry by id, for the commands that act on a single row.
    pub fn entry(&self, id: &str) -> Option<CredentialEntry> {
        self.with(|state| {
            state
                .document
                .entries
                .iter()
                .chain(state.session.iter())
                .find(|entry| entry.id == id)
                .cloned()
        })
    }

    /// The entry that would answer this remote, readable or not.
    ///
    /// The dialogs use this to say "a saved account exists" instead of
    /// pretending nothing was ever stored, without reading a secret.
    pub fn matching_entry(&self, target: &RemoteTarget) -> Option<CredentialEntry> {
        self.with(|state| {
            super::prompt::best_match(
                state.document.entries.iter().chain(state.session.iter()),
                target,
            )
            .cloned()
        })
    }

    /// Whether any entry claims this remote, readable or not.
    pub fn has_entry(&self, target: &RemoteTarget) -> bool {
        self.matching_entry(target).is_some()
    }

    /// Whether an entry still has a readable secret.
    pub fn secret_present(&self, entry: &CredentialEntry) -> bool {
        self.secrets
            .get(entry.storage, &entry.secret_handle())
            .ok()
            .flatten()
            .is_some()
    }

    /// `git credential fill`-shaped answer, for the optional helper process.
    pub fn fill(&self, target: &RemoteTarget) -> Option<(String, String)> {
        let resolved = self.resolve(target, now_seconds())?;
        let secret = resolved.secret?;
        Some((resolved.entry.username, secret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::JsonFileStore;
    use std::path::PathBuf;

    struct Fixture {
        vault: CredentialVault,
        directory: PathBuf,
        store: Arc<JsonFileStore>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn fixture() -> Fixture {
        let directory = std::env::temp_dir().join(format!(
            "augur-vault-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let store: Arc<JsonFileStore> =
            Arc::new(JsonFileStore::open_in(&directory, "settings.json").unwrap());
        let (vault, warnings) = CredentialVault::load(
            store.clone(),
            SecretStores::with_plaintext(
                Arc::new(crate::credentials::secret::MemorySecretStore::new()),
                Some(directory.join("credential-secrets.json")),
            ),
        );
        assert!(warnings.is_empty());
        Fixture {
            vault,
            directory,
            store,
        }
    }

    fn input(host: &str, username: &str, password: &str) -> CredentialInput {
        CredentialInput {
            id: None,
            protocol: "https".into(),
            host: host.into(),
            path_prefix: None,
            username: username.into(),
            password: Some(password.into()),
            scope: CredentialScope::Global,
            storage: SecretStorage::Keychain,
        }
    }

    fn target(host: &str) -> RemoteTarget {
        RemoteTarget {
            protocol: "https".into(),
            host: host.into(),
            path: None,
            username: None,
            repository: None,
        }
    }

    #[test]
    fn a_saved_entry_answers_its_own_host_only() {
        let fixture = fixture();
        fixture
            .vault
            .save(input("github.com", "dev", "s3cret"), 10)
            .unwrap();
        let resolved = fixture.vault.resolve(&target("github.com"), 20).unwrap();
        assert_eq!(resolved.secret.as_deref(), Some("s3cret"));
        assert_eq!(resolved.entry.username, "dev");
        assert!(fixture.vault.resolve(&target("gitlab.com"), 20).is_none());
    }

    #[test]
    fn saving_the_same_account_twice_updates_one_entry() {
        let fixture = fixture();
        fixture
            .vault
            .save(input("github.com", "dev", "one"), 10)
            .unwrap();
        let snapshot = fixture
            .vault
            .save(input("github.com", "dev", "two"), 20)
            .unwrap();
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(
            fixture
                .vault
                .resolve(&target("github.com"), 30)
                .unwrap()
                .secret
                .as_deref(),
            Some("two")
        );
    }

    #[test]
    fn a_session_entry_never_reaches_the_document() {
        let fixture = fixture();
        let mut request = input("github.com", "dev", "s3cret");
        request.storage = SecretStorage::Session;
        let snapshot = fixture.vault.save(request, 10).unwrap();
        assert_eq!(snapshot.entries.len(), 1);
        let stored = fixture
            .store
            .get(CREDENTIALS_DOCUMENT_KEY)
            .expect("document written");
        assert_eq!(stored["entries"].as_array().unwrap().len(), 0);
        assert!(fixture.vault.resolve(&target("github.com"), 20).is_some());
    }

    #[test]
    fn deleting_an_entry_removes_its_secret() {
        let fixture = fixture();
        let snapshot = fixture
            .vault
            .save(input("github.com", "dev", "s3cret"), 10)
            .unwrap();
        let id = snapshot.entries[0].entry.id.clone();
        let snapshot = fixture.vault.delete(&id).unwrap();
        assert!(snapshot.entries.is_empty());
        assert!(fixture.vault.resolve(&target("github.com"), 20).is_none());
    }

    #[test]
    fn a_rejected_entry_is_not_offered_again_immediately() {
        let fixture = fixture();
        let snapshot = fixture
            .vault
            .save(input("github.com", "dev", "s3cret"), 10)
            .unwrap();
        let id = snapshot.entries[0].entry.id.clone();
        fixture.vault.mark_failed(&id, 20);
        assert!(fixture.vault.resolve(&target("github.com"), 30).is_none());
        assert!(
            fixture
                .vault
                .resolve(&target("github.com"), 20 + FAILURE_COOLDOWN_SECONDS + 1)
                .is_some()
        );
    }

    #[test]
    fn editing_an_entry_without_a_password_keeps_the_secret() {
        let fixture = fixture();
        let snapshot = fixture
            .vault
            .save(input("github.com", "dev", "s3cret"), 10)
            .unwrap();
        let id = snapshot.entries[0].entry.id.clone();
        let mut request = input("github.com", "dev", "");
        request.id = Some(id);
        request.password = None;
        fixture.vault.save(request, 20).unwrap();
        assert_eq!(
            fixture
                .vault
                .resolve(&target("github.com"), 30)
                .unwrap()
                .secret
                .as_deref(),
            Some("s3cret")
        );
    }

    #[test]
    fn a_plaintext_entry_is_refused_until_the_backend_is_enabled() {
        let fixture = fixture();
        let mut request = input("github.com", "dev", "s3cret");
        request.storage = SecretStorage::Plaintext;
        assert!(fixture.vault.save(request, 10).is_err());
        fixture
            .vault
            .update_settings(CredentialSettingsPatch {
                allow_plaintext: Some(true),
                ..CredentialSettingsPatch::default()
            })
            .unwrap();
        let mut request = input("github.com", "dev", "s3cret");
        request.storage = SecretStorage::Plaintext;
        assert!(fixture.vault.save(request, 10).is_ok());
        assert!(fixture.directory.join("credential-secrets.json").exists());
    }

    #[test]
    fn disabling_the_plaintext_backend_drops_its_entries() {
        let fixture = fixture();
        fixture
            .vault
            .update_settings(CredentialSettingsPatch {
                allow_plaintext: Some(true),
                ..CredentialSettingsPatch::default()
            })
            .unwrap();
        let mut request = input("github.com", "dev", "s3cret");
        request.storage = SecretStorage::Plaintext;
        fixture.vault.save(request, 10).unwrap();
        let snapshot = fixture
            .vault
            .update_settings(CredentialSettingsPatch {
                allow_plaintext: Some(false),
                ..CredentialSettingsPatch::default()
            })
            .unwrap();
        assert!(snapshot.entries.is_empty());
    }

    #[test]
    fn an_invalid_host_is_refused() {
        let fixture = fixture();
        assert!(
            fixture
                .vault
                .save(input("bad host", "dev", "s3cret"), 10)
                .is_err()
        );
        assert!(fixture.vault.save(input("", "dev", "s3cret"), 10).is_err());
        let mut wrong_protocol = input("github.com", "dev", "s3cret");
        wrong_protocol.protocol = "ftp".into();
        assert!(fixture.vault.save(wrong_protocol, 10).is_err());
    }

    #[test]
    fn importing_git_store_lines_moves_them_into_the_keychain() {
        let fixture = fixture();
        let text =
            "https://dev:s3cret@github.com\nhttps://dev:token@git.example.com:8443\nnot a url\n";
        let (report, warnings) = fixture.vault.import_git_credentials(text, 10);
        assert_eq!(report.imported, 2);
        assert_eq!(report.skipped, 1);
        assert!(warnings.is_empty());
        let snapshot = fixture.vault.snapshot();
        assert!(
            snapshot
                .entries
                .iter()
                .all(|entry| entry.entry.storage == SecretStorage::Keychain)
        );
    }

    #[test]
    fn a_username_only_entry_prefills_the_prompt_without_a_secret() {
        let fixture = fixture();
        fixture
            .vault
            .save(input("github.com", "dev", ""), 10)
            .unwrap();
        let mut named = target("github.com");
        named.username = Some("dev".into());
        let resolved = fixture.vault.resolve(&named, 20).unwrap();
        assert!(resolved.secret.is_none());
        assert_eq!(resolved.entry.username, "dev");
    }

    #[test]
    fn help_text_never_reaches_the_document() {
        let fixture = fixture();
        fixture
            .vault
            .save(input("github.com", "dev", "s3cret"), 10)
            .unwrap();
        let text = fixture
            .store
            .path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap();
        assert!(text.contains("github.com"));
        assert!(!text.contains("s3cret"));
    }
}
