//! The credential dialog: username and password forms for the interface.
//!
//! Re-exported so both frontends and the Tauri shell name one type.

pub mod helper;
pub mod model;
pub mod prompt;
pub mod secret;
pub mod vault;

pub use helper::run_helper;
pub use model::{
    CREDENTIALS_DOCUMENT_KEY, CredentialDocument, CredentialEntry, CredentialInput,
    CredentialScope, CredentialSettings, CredentialSettingsPatch, CredentialSnapshot,
    CredentialTest, ImportReport, RepositoryIdentity, SecretStorage,
};
pub use prompt::{PromptContext, PromptKind, RemoteTarget, parse_prompt};
pub use secret::{KeychainSecretStore, MemorySecretStore, PlaintextFileStore, SecretStores};
pub use vault::{CredentialVault, ResolvedCredential, now_seconds};
