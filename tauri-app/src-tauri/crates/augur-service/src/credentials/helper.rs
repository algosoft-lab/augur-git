//! `git credential` helper entry point.
//!
//! Git consults this process only after the user's own helpers came up empty,
//! because the application appends one `credential.helper` entry through
//! `GIT_CONFIG_*` for the commands it spawns. The helper answers reads only:
//! writes stay with the application, so a `git push` in a terminal cannot
//! rewrite the settings document the running application holds in memory.
//!
//! Nothing but the credential protocol reaches stdout; diagnostics go to
//! stderr, and the secret is never written to either log.

use std::io::{BufRead, Write};

use augur_core::config::LocationConfig;

use super::model::{
    CREDENTIALS_DOCUMENT_KEY, CredentialDocument, CredentialEntry, RepositoryIdentity,
};
use super::prompt::{RemoteTarget, normalize_typed_host};
use super::secret::{KeychainSecretStore, SecretStores};
use super::vault::now_seconds;

const MAX_INPUT_LINES: usize = 64;

/// Everything the helper reads from its environment.
#[derive(Clone, Debug)]
pub struct HelperEnvironment {
    pub document: std::path::PathBuf,
    pub secrets: Option<std::path::PathBuf>,
    pub repository: Option<String>,
}

impl HelperEnvironment {
    /// Read the environment Git inherited from the application.
    pub fn from_process() -> Result<Self, String> {
        Ok(Self {
            document: std::env::var("AUGUR_GIT_CREDENTIAL_DOC")
                .map(std::path::PathBuf::from)
                .map_err(|_| "the credential document is not configured".to_string())?,
            secrets: std::env::var("AUGUR_GIT_CREDENTIAL_SECRETS")
                .ok()
                .map(std::path::PathBuf::from),
            repository: std::env::var("AUGUR_GIT_REPO_PATH")
                .ok()
                .filter(|path| !path.is_empty()),
        })
    }
}

/// Run the helper: `get`, `store`, or `erase`.
pub fn run_helper(operation: &str) -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_helper_with(operation, &mut stdin.lock(), &mut stdout.lock())
}

/// The same entry point over explicit streams, so the protocol is testable.
pub fn run_helper_with(
    operation: &str,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<(), String> {
    let request = read_request(input)?;
    match operation {
        "get" => {
            let environment = HelperEnvironment::from_process()?;
            answer(request, &environment, output)
        }
        "store" | "erase" => {
            // Deliberately a no-op: see the module comment.
            Ok(())
        }
        other => Err(format!("unknown credential helper operation: {other}")),
    }
}

#[derive(Clone, Debug, Default)]
pub struct HelperRequest {
    pub protocol: String,
    pub host: String,
    pub path: Option<String>,
    pub username: Option<String>,
}

fn read_request(input: &mut impl BufRead) -> Result<HelperRequest, String> {
    let mut request = HelperRequest::default();
    for line in input.lines().take(MAX_INPUT_LINES) {
        let line = line.map_err(|error| error.to_string())?;
        if line.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "protocol" => request.protocol = value.to_string(),
            "host" => request.host = value.to_string(),
            "path" => {
                request.path = Some(value.trim_matches('/').to_string()).filter(|v| !v.is_empty())
            }
            "username" => {
                request.username = Some(value.to_string()).filter(|value| !value.is_empty())
            }
            _ => {}
        }
    }
    Ok(request)
}

fn answer(
    request: HelperRequest,
    environment: &HelperEnvironment,
    output: &mut impl Write,
) -> Result<(), String> {
    if request.protocol.is_empty() || request.host.is_empty() {
        return Ok(());
    }
    let text = match std::fs::read_to_string(&environment.document) {
        Ok(text) => text,
        // No document means no entries; failing the command would make every
        // unrelated fetch in a terminal noisy.
        Err(_) => return Ok(()),
    };
    let Some(document) = read_document(&text) else {
        return Ok(());
    };
    let target = RemoteTarget {
        protocol: request.protocol.to_ascii_lowercase(),
        host: normalize_typed_host(&request.host).ok_or("invalid host")?,
        path: request.path,
        username: request.username,
        repository: environment
            .repository
            .clone()
            .map(|path| RepositoryIdentity {
                location: LocationConfig::Local,
                path,
            }),
    };
    let secrets = SecretStores::with_plaintext(
        std::sync::Arc::new(KeychainSecretStore::new()),
        environment.secrets.clone(),
    );
    let Some(entry) = usable_entry(&document.entries, &target) else {
        return Ok(());
    };
    let Some(secret) = secrets
        .get(entry.storage, &entry.secret_handle())
        .ok()
        .flatten()
        .filter(|secret| !secret.is_empty())
    else {
        return Ok(());
    };
    writeln!(output, "username={}", entry.username).map_err(|error| error.to_string())?;
    writeln!(output, "password={secret}").map_err(|error| error.to_string())?;
    writeln!(output).map_err(|error| error.to_string())
}

fn read_document(text: &str) -> Option<CredentialDocument> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let document = value.get(CREDENTIALS_DOCUMENT_KEY)?;
    serde_json::from_value(document.clone()).ok()
}

/// The entry that would answer this request right now.
fn usable_entry<'a>(
    entries: &'a [CredentialEntry],
    target: &RemoteTarget,
) -> Option<&'a CredentialEntry> {
    let now = now_seconds();
    super::prompt::best_match(
        entries.iter().filter(|entry| {
            entry.last_failure_at.is_none_or(|failed| {
                now.saturating_sub(failed) > super::vault::FAILURE_COOLDOWN_SECONDS
            })
        }),
        target,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::model::{CredentialScope, SecretStorage};

    fn entry(host: &str, username: &str) -> CredentialEntry {
        CredentialEntry {
            id: "id".into(),
            protocol: "https".into(),
            host: host.into(),
            path_prefix: None,
            username: username.into(),
            scope: CredentialScope::Global,
            storage: SecretStorage::Keychain,
            created_at: 1,
            updated_at: 1,
            last_used_at: None,
            last_failure_at: None,
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

    fn plaintext_fixture() -> (HelperEnvironment, std::path::PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "augur-helper-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let document = directory.join("settings.json");
        let secrets = directory.join("credential-secrets.json");
        std::fs::write(
            &document,
            serde_json::json!({
                "credentials": {
                    "settings": { "allowPlaintext": true },
                    "entries": [ {
                        "id": "abc", "protocol": "https", "host": "git.example.com",
                        "username": "dev", "storage": "plaintext",
                        "createdAt": 1, "updatedAt": 1
                    } ]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(&secrets, r#"{"credential-abc":"s3cret"}"#).unwrap();
        (
            HelperEnvironment {
                document,
                secrets: Some(secrets),
                repository: None,
            },
            directory,
        )
    }

    #[test]
    fn get_prints_the_git_credential_protocol() {
        let (environment, directory) = plaintext_fixture();
        let mut input = std::io::Cursor::new("protocol=https\nhost=git.example.com\n\n");
        let request = read_request(&mut input).unwrap();
        let mut output: Vec<u8> = Vec::new();
        answer(request, &environment, &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "username=dev\npassword=s3cret\n\n"
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn get_prints_nothing_for_a_host_it_does_not_know() {
        let (environment, directory) = plaintext_fixture();
        let mut input = std::io::Cursor::new("protocol=https\nhost=other.example.com\n\n");
        let request = read_request(&mut input).unwrap();
        let mut output: Vec<u8> = Vec::new();
        answer(request, &environment, &mut output).unwrap();
        assert!(output.is_empty());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn the_document_is_read_from_the_settings_key() {
        let text = serde_json::json!({
            "settings": {},
            "credentials": { "entries": [ {
                "id": "a", "protocol": "https", "host": "github.com",
                "username": "dev", "storage": "keychain",
                "createdAt": 1, "updatedAt": 1
            } ] }
        })
        .to_string();
        let document = read_document(&text).expect("document");
        assert_eq!(document.entries.len(), 1);
        assert_eq!(document.entries[0].host, "github.com");
    }

    #[test]
    fn a_settings_file_without_credentials_is_not_an_error() {
        assert!(read_document(r#"{"theme":"dark"}"#).is_none());
        assert!(read_document("not json").is_none());
    }

    #[test]
    fn only_a_usable_entry_answers() {
        let entries = [entry("github.com", "dev")];
        assert!(usable_entry(&entries, &target("github.com")).is_some());
        assert!(usable_entry(&entries, &target("gitlab.com")).is_none());
        let mut rejected = entries[0].clone();
        rejected.last_failure_at = Some(now_seconds());
        assert!(usable_entry(&[rejected], &target("github.com")).is_none());
    }
}
