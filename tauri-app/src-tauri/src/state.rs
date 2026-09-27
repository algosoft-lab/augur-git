//! Application-wide state: open repositories, settings, and session data.
//!
//! One `AppState` lives for the whole process and is shared by every window.
//! The webview never holds a Git handle; it asks for an operation by name and
//! receives events carrying the repository id they belong to.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};

use augur_core::config::{
    AppConfig, GraphHistoryPreference, LocationConfig, OpenTabConfig, WorkspaceState,
};
use augur_core::git::{GitError, GitRepo, LogScope};

use crate::events::{OpenPathsPayload, OPEN_PATHS_EVENT};
use crate::persistence::{LoadReport, Persistence, SettingsDocument};
use crate::repo::RepoSession;

/// Mutable state guarded by a single lock. Operations are short: they either
/// update a counter, start a worker, or read a document.
struct Inner {
    persistence: Arc<Persistence>,
    repos: HashMap<u64, RepoSession>,
    next_repo_id: u64,
    /// Repository paths a launch asked for, waiting for the webview to
    /// subscribe. A window that loads late still receives them.
    pending_paths: Vec<String>,
}

/// Shared application state.
pub struct AppState {
    inner: Mutex<Inner>,
}

impl AppState {
    pub fn new(app: AppHandle, report: LoadReport) -> Self {
        let (persistence, load_warnings) = Persistence::load(app);
        for warning in load_warnings.warnings.into_iter().chain(report.warnings) {
            log::warn!("[store] {warning}");
        }
        Self {
            inner: Mutex::new(Inner {
                persistence: Arc::new(persistence),
                repos: HashMap::new(),
                next_repo_id: 1,
                pending_paths: Vec::new(),
            }),
        }
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
        app: &AppHandle,
        path: String,
        location: LocationConfig,
    ) -> Result<u64, GitError> {
        let repo: GitRepo = location.to_repo(path.clone())?;
        let persistence = self.persistence();

        let id = self.with(|inner| -> Result<u64, GitError> {
            if let Some((id, _)) = inner.repos.iter().find(|(_, session)| {
                session.path() == path && *session.location() == location
            }) {
                log::info!("[repo] reusing open repository {path}");
                return Ok(*id);
            }
            let id = inner.next_repo_id;
            inner.next_repo_id += 1;
            let session =
                RepoSession::open(app.clone(), id, path.clone(), location.clone(), repo)?;
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
            workspace.active_tab = Some(key);
        });
        persistence.update_settings(|settings| {
            settings.config.push_recent(&path, &location);
        });
        Ok(id)
    }

    /// Stop a repository's worker and forget it.
    pub fn close_repository(&self, repo_id: u64) {
        self.with(|inner| {
            if let Some(session) = inner.repos.remove(&repo_id) {
                session.close();
                log::info!("[repo] closed repository {repo_id}");
            }
        });
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

    /// Queue repository paths requested at launch. The main window collects
    /// them once it has subscribed to the open-paths event.
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

    /// Whether any path is waiting for the main window to ask for it.
    pub fn has_pending_paths(&self) -> bool {
        self.with(|inner| !inner.pending_paths.is_empty())
    }

    /// Send CLI paths to the main window.
    ///
    /// Queued as well as emitted, and that is the whole point: a path handed
    /// over by a second launch can arrive while the window is still booting,
    /// and an event emitted to a window that has not yet subscribed is dropped
    /// without a trace. A window that *is* listening receives both, and
    /// opening a path that is already open selects its tab rather than opening
    /// a second one, so the duplicate is harmless.
    pub fn deliver_open_paths(app: &AppHandle, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        if let Some(state) = app.try_state::<AppState>() {
            state.queue_paths(paths.clone());
        }
        let payload = OpenPathsPayload { paths };
        let delivered = app
            .get_webview_window("main")
            .map(|window| window.emit(OPEN_PATHS_EVENT, payload).is_ok())
            .unwrap_or(false);
        if !delivered {
            log::warn!("[cli] no main window to emit forwarded paths to");
        }
    }

    /// Persist the final snapshot and stop every worker.
    pub fn shutdown(&self) {
        let sessions: Vec<RepoSession> = self
            .with(|inner| inner.repos.drain().map(|(_, session)| session).collect());
        for session in sessions {
            session.close();
        }
        self.persistence().flush();
    }
}

/// Build a Git handle for a location without opening a worker. Used by the
/// inline validation in the WSL open dialog.
pub fn probe_location(
    location: &LocationConfig,
    path: &str,
) -> Result<(), GitError> {
    let repo: GitRepo = location.to_repo(path)?;
    augur_core::git::probe_wsl_repository(&repo)
}

/// Log scope for a repository, derived from the persisted preference and the
/// tracked upstream reported by the latest status snapshot.
pub fn log_scope(
    preference: GraphHistoryPreference,
    upstream: Option<String>,
) -> LogScope {
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
            log_scope(GraphHistoryPreference::AllBranches, Some("origin/main".into())),
            LogScope::AllBranches
        );
        assert_eq!(
            log_scope(GraphHistoryPreference::CurrentBranch, Some("origin/main".into())),
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

    #[test]
    fn a_wsl_location_cannot_be_probed_off_windows() {
        let result = probe_location(&LocationConfig::wsl("Ubuntu"), "/home/dev/repo");
        assert_eq!(result.is_ok(), cfg!(windows));
    }
}
