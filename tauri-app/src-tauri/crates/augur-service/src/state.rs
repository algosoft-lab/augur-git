//! Application-wide state: open repositories, settings, and session data.
//!
//! One `AppState` lives for the whole process and is shared by every client of
//! its frontend, whether that is a webview window or a browser tab. The
//! frontend never holds a Git handle; it asks for an operation by name and
//! receives events carrying the repository id they belong to.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use augur_core::config::{
    AppConfig, GraphHistoryPreference, LocationConfig, OpenTabConfig, WorkspaceState,
};
use augur_core::git::{GitError, GitRepo, LogScope};

use crate::auto_refresh::{AutoRefreshController, RefreshTarget};
use crate::credentials::model::{CredentialSettings, CredentialSettingsPatch, CredentialSnapshot};
use crate::credentials::secret::SecretStores;
use crate::credentials::vault::CredentialVault;
use crate::error::CommandError;
use crate::persistence::{Persistence, SettingsDocument};
use crate::repo::RepoSession;
use crate::sink::EventSink;

/// Mutable state guarded by a single lock. Operations are short: they either
/// update a counter, start a worker, or read a document.
struct Inner {
    persistence: Arc<Persistence>,
    credentials: Arc<CredentialVault>,
    repos: HashMap<u64, RepoSession>,
    next_repo_id: u64,
    /// Repository paths a launch asked for, waiting for the frontend to
    /// subscribe. A client that loads late still receives them.
    pending_paths: Vec<String>,
    auto_refresh_target: Option<u64>,
    auto_refresh_generation: u64,
}

/// Shared application state.
pub struct AppState {
    inner: Mutex<Inner>,
    sink: Arc<dyn EventSink>,
    auto_refresh: AutoRefreshController,
    focused: AtomicBool,
}

/// Everything a frontend supplies when it builds the shared state.
pub struct ServiceStartup {
    pub settings_store: Arc<dyn crate::store::DocumentStore>,
    pub workspace_store: Arc<dyn crate::store::DocumentStore>,
    /// The secret backends this frontend can reach. A headless server passes
    /// the same keychain and, when the user enabled it, the same plaintext
    /// file the desktop build would.
    pub secrets: SecretStores,
    /// Extra load warnings the frontend produced while preparing the stores.
    pub warnings: Vec<String>,
}

impl AppState {
    pub fn new(sink: Arc<dyn EventSink>, startup: ServiceStartup) -> Self {
        let (persistence, load_warnings) = Persistence::load(
            sink.clone(),
            startup.settings_store.clone(),
            startup.workspace_store,
        );
        let (credentials, credential_warnings) =
            CredentialVault::load(startup.settings_store, startup.secrets.clone());
        let config = persistence.config();
        let auto_refresh_enabled = config.view.auto_refresh;
        configure_lfs_policy(&config);
        for warning in load_warnings
            .warnings
            .into_iter()
            .chain(credential_warnings)
            .chain(startup.warnings)
        {
            log::warn!("[store] {warning}");
        }
        let state = Self {
            inner: Mutex::new(Inner {
                persistence: Arc::new(persistence),
                credentials: Arc::new(credentials),
                repos: HashMap::new(),
                next_repo_id: 1,
                pending_paths: Vec::new(),
                auto_refresh_target: None,
                auto_refresh_generation: 0,
            }),
            sink,
            auto_refresh: AutoRefreshController::new(auto_refresh_enabled),
            focused: AtomicBool::new(false),
        };
        state.sync_credential_helper();
        state
    }

    /// The credential vault, shared with the askpass bridge.
    pub fn credentials(&self) -> Arc<CredentialVault> {
        self.with(|inner| inner.credentials.clone())
    }

    pub fn credential_snapshot(&self) -> CredentialSnapshot {
        self.credentials().snapshot()
    }

    /// Apply an interface change and re-wire the Git helper when it flipped.
    pub fn update_credential_settings(
        &self,
        patch: CredentialSettingsPatch,
    ) -> Result<CredentialSnapshot, CommandError> {
        let snapshot = self
            .credentials()
            .update_settings(patch)
            .map_err(|detail| CommandError::new("err-credential-settings", detail))?;
        self.sync_credential_helper();
        Ok(snapshot)
    }

    /// Keep the child-process helper in step with the saved preference.
    fn sync_credential_helper(&self) {
        let settings: CredentialSettings = self.credentials().settings();
        if !settings.use_helper {
            augur_core::git::location::configure_git_credential_bridge(None);
            return;
        }
        let document = self
            .persistence()
            .credential_document_path()
            .map(|path| path.to_string_lossy().into_owned());
        let executable = std::env::current_exe()
            .ok()
            .map(|path| path.to_string_lossy().into_owned());
        match (document, executable) {
            (Some(document), Some(executable)) => {
                augur_core::git::location::configure_git_credential_bridge(Some(
                    augur_core::git::location::GitCredentialBridge {
                        executable,
                        document,
                        secrets: self
                            .credentials()
                            .secrets()
                            .plaintext_path()
                            .map(|path| path.to_string_lossy().into_owned()),
                    },
                ));
            }
            _ => {
                log::warn!(
                    "[git_credential] the helper cannot be configured without a settings file path"
                );
                augur_core::git::location::configure_git_credential_bridge(None);
            }
        }
    }

    /// The event sink every emitted notification of this process uses.
    pub fn sink(&self) -> &Arc<dyn EventSink> {
        &self.sink
    }

    /// Run `body` with the locked state.
    fn with<R>(&self, body: impl FnOnce(&mut Inner) -> R) -> R {
        let mut guard = self.inner.lock().expect("app state lock");
        body(&mut guard)
    }

    /// A handle to the durable documents, usable without holding the state lock.
    pub fn persistence(&self) -> Arc<Persistence> {
        self.with(|inner| inner.persistence.clone())
    }

    pub fn settings(&self) -> SettingsDocument {
        self.persistence().settings()
    }

    pub fn config(&self) -> AppConfig {
        self.persistence().config()
    }

    pub fn workspace(&self) -> WorkspaceState {
        self.persistence().workspace()
    }

    pub fn update_settings(&self, mutate: impl FnOnce(&mut SettingsDocument)) {
        self.persistence().update_settings(mutate);
        configure_lfs_policy(&self.persistence().config());
        self.auto_refresh
            .set_enabled(self.persistence().config().view.auto_refresh);
    }

    /// Select the only repository eligible for ongoing automatic refresh.
    pub fn set_auto_refresh_target(
        &self,
        repo_id: Option<u64>,
        generation: u64,
    ) -> Result<(), CommandError> {
        let target = self.with(|inner| -> Result<Option<Option<RefreshTarget>>, CommandError> {
            if generation <= inner.auto_refresh_generation {
                return Ok(None);
            }
            let target = repo_id
                .map(|id| {
                    inner
                        .repos
                        .get(&id)
                        .map(|session| RefreshTarget {
                            id,
                            repo: session.repo().clone(),
                            handle: session.handle().clone(),
                        })
                        .ok_or_else(|| CommandError::missing_repo(id))
                })
                .transpose()?;
            inner.auto_refresh_generation = generation;
            inner.auto_refresh_target = repo_id;
            Ok(Some(target))
        })?;
        if let Some(target) = target {
            self.auto_refresh.set_target(target);
        }
        Ok(())
    }

    /// Pause or resume the monitor with the primary client's focus state.
    pub fn set_main_window_focused(&self, focused: bool) {
        if self.focused.swap(focused, Ordering::AcqRel) != focused {
            self.auto_refresh.set_focused(focused);
        }
    }

    pub fn update_workspace(&self, mutate: impl FnOnce(&mut WorkspaceState)) {
        self.persistence().update_workspace(mutate);
    }

    pub fn update_workspace_visible(&self, mutate: impl FnOnce(&mut WorkspaceState)) {
        self.persistence().update_workspace_visible(mutate);
    }

    /// Open a repository and return its id. Opening the same repository twice
    /// reuses the running worker so a tab and its Git session cannot diverge.
    pub fn open_repository(
        &self,
        path: String,
        location: LocationConfig,
    ) -> Result<u64, GitError> {
        let repo: GitRepo = location.to_repo(path.clone())?;
        let persistence = self.persistence();
        configure_lfs_policy(&persistence.config());

        let id = self.with(|inner| -> Result<u64, GitError> {
            if let Some((id, _)) = inner
                .repos
                .iter()
                .find(|(_, session)| session.path() == path && *session.location() == location)
            {
                log::info!("[repo] reusing open repository {path}");
                return Ok(*id);
            }
            let id = inner.next_repo_id;
            inner.next_repo_id += 1;
            let session = RepoSession::open(
                self.sink.clone(),
                id,
                path.clone(),
                location.clone(),
                repo,
            )?;
            inner.repos.insert(id, session);
            Ok(id)
        })?;

        let key = OpenTabConfig {
            path: path.clone(),
            location: location.clone(),
        }
        .key();
        persistence.update_workspace_visible(|workspace| {
            if !workspace.open_tabs.iter().any(|tab| tab.key() == key) {
                workspace.open_tabs.push(OpenTabConfig {
                    path: path.clone(),
                    location: location.clone(),
                });
            }
            // The active tab is not moved here. The frontend owns the tab list
            // and says which tab it is showing; a client restoring its saved
            // workspace opens several repositories in a row, and letting each
            // one claim the selection would leave it on whichever tab
            // happened to be opened last.
        });
        persistence.update_settings(|settings| {
            settings.config.push_recent(&path, &location);
        });
        Ok(id)
    }

    /// Stop a repository's worker and forget it.
    pub fn close_repository(&self, repo_id: u64) {
        let was_target = self.with(|inner| {
            let was_target = inner.auto_refresh_target == Some(repo_id);
            if was_target {
                inner.auto_refresh_target = None;
            }
            if let Some(session) = inner.repos.remove(&repo_id) {
                session.close();
                log::info!("[repo] closed repository {repo_id}");
            }
            was_target
        });
        if was_target {
            self.auto_refresh.set_target(None);
        }
    }

    /// Borrow a repository session for one operation.
    pub fn with_repo<R>(&self, repo_id: u64, body: impl FnOnce(&RepoSession) -> R) -> Option<R> {
        self.with(|inner| inner.repos.get(&repo_id).map(body))
    }

    /// Repository ids that are currently open, in ascending order.
    pub fn repository_ids(&self) -> Vec<u64> {
        self.with(|inner| {
            let mut ids: Vec<u64> = inner.repos.keys().copied().collect();
            ids.sort_unstable();
            ids
        })
    }

    /// Queue repository paths requested at launch. The frontend collects them
    /// once it has subscribed to the open-paths event.
    pub fn queue_paths(&self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        log::info!("[cli] {} path(s) requested at startup", paths.len());
        self.with(|inner| inner.pending_paths.extend(paths));
    }

    /// Take the paths queued at launch, clearing the queue.
    pub fn take_pending_paths(&self) -> Vec<String> {
        self.with(|inner| std::mem::take(&mut inner.pending_paths))
    }

    /// Whether any path is waiting for the primary client to ask for it.
    pub fn has_pending_paths(&self) -> bool {
        self.with(|inner| !inner.pending_paths.is_empty())
    }

    /// Persist the final snapshot and stop every worker.
    pub fn shutdown(&self) {
        augur_core::git::location::configure_git_credential_bridge(None);
        self.credentials().flush();
        self.auto_refresh.shutdown();
        let sessions: Vec<RepoSession> =
            self.with(|inner| inner.repos.drain().map(|(_, session)| session).collect());
        for session in sessions {
            session.close();
        }
        self.persistence().flush();
    }
}

fn configure_lfs_policy(config: &AppConfig) {
    augur_core::lfs::configure_download_policy(
        config.lfs_auto_download,
        config.lfs_download_on_open,
        config.lfs_repository_auto_download.clone(),
    );
}

/// Build a Git handle for a location without opening a worker. Used by the
/// inline validation in the WSL open dialog.
pub fn probe_location(location: &LocationConfig, path: &str) -> Result<(), GitError> {
    let repo: GitRepo = location.to_repo(path)?;
    augur_core::git::probe_wsl_repository(&repo)
}

/// Log scope for a repository, derived from the persisted preference and the
/// tracked upstream reported by the latest status snapshot.
pub fn log_scope(preference: GraphHistoryPreference, upstream: Option<String>) -> LogScope {
    match preference {
        GraphHistoryPreference::AllBranches => LogScope::AllBranches,
        GraphHistoryPreference::CurrentBranch => LogScope::CurrentBranch { upstream },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_scope_follows_the_persisted_preference() {
        assert_eq!(
            log_scope(
                GraphHistoryPreference::AllBranches,
                Some("origin/main".into())
            ),
            LogScope::AllBranches
        );
        assert_eq!(
            log_scope(
                GraphHistoryPreference::CurrentBranch,
                Some("origin/main".into())
            ),
            LogScope::CurrentBranch {
                upstream: Some("origin/main".into())
            }
        );
        assert_eq!(
            log_scope(GraphHistoryPreference::CurrentBranch, None),
            LogScope::CurrentBranch { upstream: None }
        );
    }

    #[test]
    fn tab_identity_separates_local_and_wsl_locations() {
        let local = OpenTabConfig {
            path: "/home/dev/repo".into(),
            location: LocationConfig::Local,
        };
        let wsl = OpenTabConfig {
            path: "/home/dev/repo".into(),
            location: LocationConfig::wsl("Ubuntu"),
        };
        assert_ne!(local.key(), wsl.key());
        assert_eq!(local.key(), local.path);
    }

    /// The name is the contract: off Windows the probe must fail. On Windows
    /// the outcome depends on machine state (which distros are installed,
    /// whether the path is a repository), so no assertion there is portable.
    #[cfg(not(windows))]
    #[test]
    fn a_wsl_location_cannot_be_probed_off_windows() {
        let result = probe_location(&LocationConfig::wsl("Ubuntu"), "/home/dev/repo");
        assert!(result.is_err());
    }
}
