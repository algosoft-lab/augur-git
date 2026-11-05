//! Durable credential metadata for HTTP/HTTPS Git remotes.
//!
//! The document holds everything except the secrets themselves: which host an
//! entry belongs to, which account it names, which storage backend keeps the
//! secret, and when it was last used or rejected. Secrets live behind
//! [`crate::credentials::secret::SecretStore`], so this document can be read,
//! exported, and inspected without ever exposing a password.

use serde::{Deserialize, Serialize};

use augur_core::config::LocationConfig;

pub const CREDENTIALS_SCHEMA_VERSION: u32 = 1;
/// Key of the credential document inside the settings store file.
pub const CREDENTIALS_DOCUMENT_KEY: &str = "credentials";

/// Where the secret of one entry is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretStorage {
    /// Kept in memory for this process only, so a restart forgets it.
    Session,
    /// The operating system's keychain or credential manager.
    #[default]
    Keychain,
    /// A `0600` JSON file beside the settings document. Never chosen
    /// automatically: the interface has to opt in.
    Plaintext,
}

impl SecretStorage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Keychain => "keychain",
            Self::Plaintext => "plaintext",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "session" => Some(Self::Session),
            "keychain" => Some(Self::Keychain),
            "plaintext" => Some(Self::Plaintext),
            _ => None,
        }
    }

    pub fn is_persistent(self) -> bool {
        !matches!(self, Self::Session)
    }
}

/// Which repositories an entry may answer for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CredentialScope {
    /// Any repository that reaches the host.
    #[default]
    Global,
    /// Only one repository, so a work and a personal account can share a host.
    Repository {
        location: LocationConfig,
        path: String,
    },
}

impl CredentialScope {
    pub fn is_global(&self) -> bool {
        matches!(self, Self::Global)
    }

    /// Repository label for the settings table; `None` means every repository.
    ///
    /// The interface renders the global case from its own catalog, so no
    /// translated text is produced here.
    pub fn label(&self) -> Option<String> {
        match self {
            Self::Global => None,
            Self::Repository { location, path } => Some(location.label(path)),
        }
    }

    pub(crate) fn matches(&self, repository: Option<&RepositoryIdentity>) -> bool {
        match self {
            Self::Global => true,
            Self::Repository { location, path } => repository
                .is_some_and(|identity| identity.path == *path && identity.location == *location),
        }
    }
}

/// One repository a Git command runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryIdentity {
    pub location: LocationConfig,
    pub path: String,
}

/// One remembered account for one HTTP(S) host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEntry {
    pub id: String,
    /// `https` or `http`.
    pub protocol: String,
    /// Host with a non-default port when the remote names one.
    pub host: String,
    /// Optional repository path prefix, without a leading slash.
    #[serde(default)]
    pub path_prefix: Option<String>,
    pub username: String,
    #[serde(default)]
    pub scope: CredentialScope,
    pub storage: SecretStorage,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub last_used_at: Option<u64>,
    #[serde(default)]
    pub last_failure_at: Option<u64>,
}

impl CredentialEntry {
    /// Stable name of this entry's secret inside a [`SecretStore`].
    ///
    /// [`SecretStore`]: crate::credentials::secret::SecretStore
    pub fn secret_handle(&self) -> String {
        format!("credential-{}", self.id)
    }

    pub fn scope_label(&self) -> Option<String> {
        self.scope.label()
    }
}

/// The stored credential document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CredentialDocument {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub settings: CredentialSettings,
    #[serde(default)]
    pub entries: Vec<CredentialEntry>,
}

fn schema_version() -> u32 {
    CREDENTIALS_SCHEMA_VERSION
}

impl Default for CredentialDocument {
    fn default() -> Self {
        Self {
            schema_version: CREDENTIALS_SCHEMA_VERSION,
            settings: CredentialSettings::default(),
            entries: Vec::new(),
        }
    }
}

impl CredentialDocument {
    /// Drop entries the settings no longer allow and de-duplicate ids, so a
    /// hand-edited document cannot make the table contradict itself.
    pub fn normalize(&mut self) {
        self.schema_version = CREDENTIALS_SCHEMA_VERSION;
        if !self.settings.allow_plaintext {
            // A kept entry would be unreadable; refusing to render it as valid
            // is better than silently answering prompts with nothing.
            self.entries
                .retain(|entry| entry.storage != SecretStorage::Plaintext);
            if self.settings.default_storage == SecretStorage::Plaintext {
                self.settings.default_storage = SecretStorage::Keychain;
            }
        }
        let mut seen: Vec<String> = Vec::new();
        self.entries.retain(|entry| {
            if seen.contains(&entry.id) {
                false
            } else {
                seen.push(entry.id.clone());
                true
            }
        });
    }
}

/// Interface-level preferences for credential handling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSettings {
    /// Answer a Git prompt from a saved entry without showing the dialog.
    #[serde(default = "default_true")]
    pub auto_fill: bool,
    /// Preselected storage in the prompt dialog and the add form.
    #[serde(default)]
    pub default_storage: SecretStorage,
    /// Whether the unencrypted file backend may be selected at all.
    #[serde(default)]
    pub allow_plaintext: bool,
    /// Offer stored credentials to Git through `credential.helper`.
    #[serde(default)]
    pub use_helper: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CredentialSettings {
    fn default() -> Self {
        Self {
            auto_fill: true,
            default_storage: SecretStorage::Keychain,
            allow_plaintext: false,
            use_helper: false,
        }
    }
}

/// A partial settings update sent by the interface.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSettingsPatch {
    pub auto_fill: Option<bool>,
    pub default_storage: Option<SecretStorage>,
    pub allow_plaintext: Option<bool>,
    pub use_helper: Option<bool>,
}

/// A new or edited entry sent by the interface.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInput {
    /// Present when an existing entry is edited.
    pub id: Option<String>,
    pub protocol: String,
    pub host: String,
    #[serde(default)]
    pub path_prefix: Option<String>,
    pub username: String,
    /// `None` keeps the stored secret, which is how an entry's other fields are
    /// edited without asking the user for the password again.
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub scope: CredentialScope,
    pub storage: SecretStorage,
}

/// What the settings page renders.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSnapshot {
    pub settings: CredentialSettings,
    pub entries: Vec<CredentialEntryView>,
    pub backends: Vec<SecretBackendStatus>,
    /// Whether the dialog may offer the keychain.
    pub keychain_available: bool,
    /// Whether the dialog may offer the plaintext file.
    pub plaintext_available: bool,
    /// Absolute path of the plaintext secret file, when configured.
    pub plaintext_path: Option<String>,
    /// Whether the Git credential helper is wired into spawned commands.
    pub helper_active: bool,
}

/// One row of the settings table.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEntryView {
    #[serde(flatten)]
    pub entry: CredentialEntry,
    /// Repository label, or `None` when the entry applies to every repository.
    pub scope_label: Option<String>,
    /// Whether the secret is still readable in its backend.
    pub secret_present: bool,
    /// Whether the last use was rejected by the server.
    pub failing: bool,
}

/// Availability of one backend, in the order the interface lists them.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretBackendStatus {
    pub storage: SecretStorage,
    pub available: bool,
    pub detail: Option<String>,
}

/// Result of importing the plaintext file Git's own `store` helper writes.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub source: String,
    pub imported: usize,
    pub skipped: usize,
}

/// Result of asking the server whether a stored entry is accepted.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialTest {
    pub ok: bool,
    /// Translation key describing the outcome.
    pub key: String,
    /// Git's own words, kept short.
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> CredentialEntry {
        CredentialEntry {
            id: id.into(),
            protocol: "https".into(),
            host: "github.com".into(),
            path_prefix: None,
            username: "dev".into(),
            scope: CredentialScope::Global,
            storage: SecretStorage::Keychain,
            created_at: 1,
            updated_at: 1,
            last_used_at: None,
            last_failure_at: None,
        }
    }

    #[test]
    fn a_document_written_before_settings_existed_still_parses() {
        let document: CredentialDocument = serde_json::from_str("{}").unwrap();
        assert_eq!(document.schema_version, CREDENTIALS_SCHEMA_VERSION);
        assert!(document.settings.auto_fill);
        assert_eq!(document.settings.default_storage, SecretStorage::Keychain);
        assert!(!document.settings.allow_plaintext);
    }

    #[test]
    fn plaintext_entries_are_removed_when_the_backend_is_disabled() {
        let mut document = CredentialDocument::default();
        document.settings.allow_plaintext = false;
        document.settings.default_storage = SecretStorage::Plaintext;
        let mut plaintext = entry("a");
        plaintext.storage = SecretStorage::Plaintext;
        document.entries.push(plaintext);
        document.entries.push(entry("a"));
        document.entries.push(entry("b"));
        document.normalize();
        assert_eq!(document.entries.len(), 2);
        assert_eq!(document.settings.default_storage, SecretStorage::Keychain);
    }

    #[test]
    fn storage_names_round_trip_through_the_wire_format() {
        for storage in [
            SecretStorage::Session,
            SecretStorage::Keychain,
            SecretStorage::Plaintext,
        ] {
            assert_eq!(SecretStorage::parse(storage.as_str()), Some(storage));
        }
        assert!(SecretStorage::Session.is_persistent().eq(&false));
        assert!(SecretStorage::Keychain.is_persistent());
    }

    #[test]
    fn a_repository_scope_names_its_repository() {
        let scope = CredentialScope::Repository {
            location: LocationConfig::Local,
            path: "/home/dev/repo".into(),
        };
        assert_eq!(scope.label(), Some("/home/dev/repo".to_string()));
        assert_eq!(CredentialScope::Global.label(), None);
    }
}
