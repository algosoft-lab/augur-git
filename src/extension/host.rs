//! Concrete host bridge for the Lua extension runtime.
//!
//! The bridge owns no GPUI entities. Workspace refreshes its immutable tab
//! registry and drains `HostEvent`s on the UI thread; Git and Agent work is
//! performed on the extension request thread with structured process args.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use serde_json::Value as JsonValue;

use crate::agent::{AgentOperation, AgentOperationChallenge, AgentSettings};
use crate::core::build_info;
use crate::core::git::automation;

use super::agent_runner::{AgentResult, agent_response, run_agent_process};
use super::api::{
    AgentPromptOptions, AgentRequest, ExtensionHost, ExtensionHostRequest,
    ExtensionRunAdmission, HostRequest, HostResponse, RepositoryOperation,
    RepositorySnapshot,
};
use super::file_log::ExtensionFileLogger;
use super::storage::ExtensionStorage;

const DEFAULT_AGENT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// UI-facing events emitted by host calls.
#[derive(Clone, Debug)]
pub enum HostEvent {
    Log {
        extension_id: String,
        level: String,
        message: String,
        fields: JsonValue,
    },
    Notify {
        extension_id: String,
        level: String,
        title: String,
        body: String,
    },
    RepositoryChanged {
        tab_id: u64,
        origin_extension_id: String,
        origin_run_id: u64,
    },
}

#[derive(Clone)]
struct RepositoryEntry {
    snapshot: RepositorySnapshot,
}

struct HostState {
    repositories: BTreeMap<u64, RepositoryEntry>,
    /// A repository can be owned by at most one extension invocation. This
    /// prevents two queued extensions from interleaving Git mutations.
    owners: HashMap<u64, (String, u64)>,
    run_identities: HashMap<(String, u64, u64), (String, Option<String>)>,
}

/// Thread-safe host implementation shared by all extension workers.
#[derive(Clone)]
pub struct HostBridge {
    state: Arc<Mutex<HostState>>,
    event_tx: Sender<HostEvent>,
    file_logger: ExtensionFileLogger,
    storage: ExtensionStorage,
    agent_settings: Arc<Mutex<AgentSettings>>,
}

impl HostBridge {
    pub fn new(agent_settings: AgentSettings) -> (Self, Receiver<HostEvent>) {
        let (event_tx, event_rx) = mpsc::channel();
        (
            Self {
                state: Arc::new(Mutex::new(HostState {
                    repositories: BTreeMap::new(),
                    owners: HashMap::new(),
                    run_identities: HashMap::new(),
                })),
                event_tx,
                file_logger: ExtensionFileLogger::default(),
                storage: ExtensionStorage::new(),
                agent_settings: Arc::new(Mutex::new(agent_settings)),
            },
            event_rx,
        )
    }

    /// Replace the current tab snapshot. This is called from Workspace after
    /// tab lifecycle/status events; no GPUI entity crosses the bridge.
    pub fn set_repositories(&self, snapshots: Vec<RepositorySnapshot>) {
        if let Ok(mut state) = self.state.lock() {
            state.repositories = snapshots
                .into_iter()
                .map(|snapshot| (snapshot.tab_id, RepositoryEntry { snapshot }))
                .collect();
        }
    }

    pub fn set_agent_settings(&self, settings: AgentSettings) {
        if let Ok(mut current) = self.agent_settings.lock() {
            *current = settings;
        }
    }

    fn snapshots(&self) -> Result<Vec<RepositorySnapshot>, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "extension host state is poisoned".to_string())?;
        Ok(state
            .repositories
            .values()
            .map(|entry| entry.snapshot.clone())
            .collect())
    }

    fn repository(&self, tab_id: u64) -> Result<RepositorySnapshot, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "extension host state is poisoned".to_string())?;
        state
            .repositories
            .get(&tab_id)
            .map(|entry| entry.snapshot.clone())
            .ok_or_else(|| "repository tab is no longer open".to_string())
    }

    fn check_owner(
        &self,
        tab_id: u64,
        extension_id: &str,
        run_id: u64,
    ) -> Result<(), String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "extension host state is poisoned".to_string())?;
        match state.owners.get(&tab_id) {
            Some(owner) if owner == &(extension_id.to_string(), run_id) => {
                Ok(())
            }
            Some(_) => {
                Err("repository is busy with another extension run".into())
            }
            None => Err("extension run does not own this repository".into()),
        }
    }

    fn check_identity(
        &self,
        snapshot: &RepositorySnapshot,
        expected_branch: &str,
        expected_head: Option<&str>,
    ) -> Result<automation::RepositoryState, HostResponse> {
        let current = automation::capture(Path::new(&snapshot.path)).map_err(
            |summary| HostResponse::Failure {
                code: "status_failed".into(),
                summary,
            },
        )?;
        if current.branch != expected_branch {
            return Err(HostResponse::Failure {
                code: "branch_changed".into(),
                summary: "repository branch changed since the trigger snapshot"
                    .into(),
            });
        }
        if current.head.as_deref() != expected_head {
            return Err(HostResponse::Failure {
                code: "head_changed".into(),
                summary: "repository HEAD changed since the trigger snapshot"
                    .into(),
            });
        }
        Ok(current)
    }

    fn run_identity(
        &self,
        extension_id: &str,
        run_id: u64,
        tab_id: u64,
        fallback_branch: &str,
        fallback_head: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "extension host state is poisoned".to_string())?;
        Ok(state
            .run_identities
            .get(&(extension_id.to_string(), run_id, tab_id))
            .cloned()
            .unwrap_or_else(|| {
                (
                    fallback_branch.to_string(),
                    fallback_head.map(ToString::to_string),
                )
            }))
    }

    fn update_run_identity(
        &self,
        extension_id: &str,
        run_id: u64,
        tab_id: u64,
        state_after: &automation::RepositoryState,
    ) {
        if let Ok(mut state) = self.state.lock() {
            state.run_identities.insert(
                (extension_id.to_string(), run_id, tab_id),
                (state_after.branch.clone(), state_after.head.clone()),
            );
        }
    }

    fn handle_repository(
        &self,
        request: &ExtensionHostRequest,
        tab_id: u64,
        operation: RepositoryOperation,
        expected_branch: String,
        expected_head: Option<String>,
    ) -> Result<HostResponse, String> {
        let snapshot = match self.repository(tab_id) {
            Ok(snapshot) => snapshot,
            Err(summary) => {
                return Ok(HostResponse::Failure {
                    code: "tab_closed".into(),
                    summary,
                });
            }
        };
        if matches!(operation, RepositoryOperation::Status) {
            return Ok(self.status_response(&snapshot));
        }
        if let Err(summary) =
            self.check_owner(tab_id, &request.extension_id, request.run_id)
        {
            // A lease race is an expected business failure. Keep it in the
            // structured Lua result so one repository does not abort the
            // extension's remaining sequential work.
            return Ok(HostResponse::Failure {
                code: "repository_busy".into(),
                summary,
            });
        }
        let (identity_branch, identity_head) = self.run_identity(
            &request.extension_id,
            request.run_id,
            tab_id,
            &expected_branch,
            expected_head.as_deref(),
        )?;
        let current = match self.check_identity(
            &snapshot,
            &identity_branch,
            identity_head.as_deref(),
        ) {
            Ok(current) => current,
            Err(response) => return Ok(response),
        };
        let path = Path::new(&snapshot.path);
        let cancelled = request.cancelled.as_ref();
        let response = match operation {
            RepositoryOperation::Status => unreachable!(),
            RepositoryOperation::WaitUntilReady { timeout_seconds } => self
                .wait_until_ready(
                    tab_id,
                    &snapshot,
                    timeout_seconds,
                    cancelled,
                ),
            RepositoryOperation::Git {
                args,
                label: _,
                timeout_seconds,
            } => {
                let result = automation::run(
                    path,
                    &args,
                    Some(Duration::from_secs(
                        timeout_seconds.clamp(1, 30 * 60),
                    )),
                    cancelled,
                );
                self.command_response(tab_id, result)
            }
            RepositoryOperation::PullRebase => {
                match automation::pull_rebase(path, cancelled) {
                    Ok(result) => self.command_response(tab_id, result),
                    Err(summary) => HostResponse::Failure {
                        code: "pull_status_failed".into(),
                        summary,
                    },
                }
            }
            RepositoryOperation::Push { remote, branch } => {
                match automation::push(
                    path,
                    remote.as_deref(),
                    branch.as_deref(),
                    cancelled,
                ) {
                    Ok(result) => self.command_response(tab_id, result),
                    Err(summary) => HostResponse::Failure {
                        code: "push_status_failed".into(),
                        summary,
                    },
                }
            }
            RepositoryOperation::AgentCommit { hint } => self
                .agent_commit(
                    &request.extension_id,
                    path,
                    &current,
                    hint.as_deref(),
                    cancelled,
                )
                .unwrap_or_else(|summary| HostResponse::Failure {
                    code: "agent_commit_failed".into(),
                    summary,
                }),
            RepositoryOperation::AgentMerge { source } => {
                let target = match resolve_commit(path, &source) {
                    Ok(target) => target,
                    Err(summary) => {
                        return Ok(HostResponse::Failure {
                            code: "invalid_source".into(),
                            summary,
                        });
                    }
                };
                self.agent_merge(
                    &request.extension_id,
                    path,
                    &current,
                    &target,
                    cancelled,
                )
                .unwrap_or_else(|summary| {
                    HostResponse::Failure {
                        code: "agent_merge_failed".into(),
                        summary,
                    }
                })
            }
            RepositoryOperation::AgentRebase { source } => {
                let upstream = match resolve_commit(path, &source) {
                    Ok(upstream) => upstream,
                    Err(summary) => {
                        return Ok(HostResponse::Failure {
                            code: "invalid_source".into(),
                            summary,
                        });
                    }
                };
                self.agent_rebase(
                    &request.extension_id,
                    path,
                    &current,
                    &upstream,
                    cancelled,
                )
                .unwrap_or_else(|summary| {
                    HostResponse::Failure {
                        code: "agent_rebase_failed".into(),
                        summary,
                    }
                })
            }
            RepositoryOperation::ResolveMerge => self
                .agent_resolve_merge(
                    &request.extension_id,
                    path,
                    &current,
                    cancelled,
                )
                .unwrap_or_else(|summary| HostResponse::Failure {
                    code: "merge_recovery_failed".into(),
                    summary,
                }),
            RepositoryOperation::ResolveRebase => self
                .agent_resolve_rebase(
                    &request.extension_id,
                    path,
                    &current,
                    cancelled,
                )
                .unwrap_or_else(|summary| HostResponse::Failure {
                    code: "rebase_recovery_failed".into(),
                    summary,
                }),
        };
        let _ = self.event_tx.send(HostEvent::RepositoryChanged {
            tab_id,
            origin_extension_id: request.extension_id.clone(),
            origin_run_id: request.run_id,
        });
        if let Ok(after) = automation::capture(path) {
            self.update_run_identity(
                &request.extension_id,
                request.run_id,
                tab_id,
                &after,
            );
        }
        Ok(response)
    }

    fn wait_until_ready(
        &self,
        tab_id: u64,
        snapshot: &RepositorySnapshot,
        timeout_seconds: u64,
        cancelled: &AtomicBool,
    ) -> HostResponse {
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(timeout_seconds.clamp(1, 5 * 60)))
            .unwrap_or_else(Instant::now);
        loop {
            if cancelled.load(Ordering::Acquire) {
                return HostResponse::Failure {
                    code: "cancelled".into(),
                    summary: "extension run cancelled".into(),
                };
            }
            let busy = match self.state.lock() {
                Ok(state) => match state.repositories.get(&tab_id) {
                    Some(entry) => entry.snapshot.busy,
                    None => {
                        return HostResponse::Failure {
                            code: "tab_closed".into(),
                            summary: "repository tab is no longer open".into(),
                        };
                    }
                },
                Err(_) => {
                    return HostResponse::Failure {
                        code: "host_state_unavailable".into(),
                        summary: "extension host state is poisoned".into(),
                    };
                }
            };
            if !busy {
                return self.status_response(snapshot);
            }
            if Instant::now() >= deadline {
                return HostResponse::Failure {
                    code: "repository_busy_timeout".into(),
                    summary:
                        "repository remained busy for the configured timeout"
                            .into(),
                };
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn status_response(&self, snapshot: &RepositorySnapshot) -> HostResponse {
        match automation::capture(Path::new(&snapshot.path)) {
            Ok(state) => json_response(
                serde_json::to_value(state).unwrap_or_else(|_| JsonValue::Null),
            ),
            Err(summary) => HostResponse::Failure {
                code: "status_failed".into(),
                summary,
            },
        }
    }

    fn command_response(
        &self,
        _tab_id: u64,
        result: automation::CommandResult,
    ) -> HostResponse {
        let mut value =
            serde_json::to_value(&result).unwrap_or_else(|_| JsonValue::Null);
        if let JsonValue::Object(object) = &mut value {
            if !result.ok && !result.cancelled && !result.timed_out {
                if result.stderr.contains("CONFLICT")
                    || result.summary.to_ascii_lowercase().contains("conflict")
                {
                    object.insert(
                        "code".into(),
                        JsonValue::String("conflict".into()),
                    );
                }
            }
            object.insert("ok".into(), JsonValue::Bool(result.ok));
        }
        json_response(value)
    }

    fn agent_commit(
        &self,
        extension_id: &str,
        path: &Path,
        before: &automation::RepositoryState,
        hint: Option<&str>,
        cancelled: &AtomicBool,
    ) -> Result<HostResponse, String> {
        let operation = AgentOperation::Commit;
        let challenge = AgentOperationChallenge::new();
        let prompt = operation
            .prompt_with_challenge(hint, &challenge)
            .map_err(|error| error.to_string())?;
        let result = self.run_agent(
            extension_id,
            path,
            &prompt,
            DEFAULT_AGENT_TIMEOUT,
            cancelled,
        )?;
        let after =
            automation::capture(path).map_err(|error| error.to_string())?;
        let verified = result.completed
            && before.head != after.head
            && !after.dirty
            && !after.conflicts
            && after.operation.is_none();
        Ok(agent_response(
            result,
            verified,
            "agent commit was not verified",
        ))
    }

    fn agent_merge(
        &self,
        extension_id: &str,
        path: &Path,
        before: &automation::RepositoryState,
        target: &str,
        cancelled: &AtomicBool,
    ) -> Result<HostResponse, String> {
        let operation = AgentOperation::Merge {
            target_oid: target.to_string(),
            baseline_head: before.head.clone(),
        };
        self.run_verified_operation(
            extension_id,
            path,
            before,
            operation,
            cancelled,
            |after| after.operation.is_none() && !after.conflicts,
        )
    }

    fn agent_rebase(
        &self,
        extension_id: &str,
        path: &Path,
        before: &automation::RepositoryState,
        upstream: &str,
        cancelled: &AtomicBool,
    ) -> Result<HostResponse, String> {
        let operation = AgentOperation::Rebase {
            upstream_oid: upstream.to_string(),
            baseline_head: before.head.clone(),
        };
        self.run_verified_operation(
            extension_id,
            path,
            before,
            operation,
            cancelled,
            |after| after.operation.is_none() && !after.conflicts,
        )
    }

    fn agent_resolve_merge(
        &self,
        extension_id: &str,
        path: &Path,
        before: &automation::RepositoryState,
        cancelled: &AtomicBool,
    ) -> Result<HostResponse, String> {
        if before.operation.as_deref() != Some("merge") {
            return Ok(HostResponse::Failure {
                code: "not_in_merge".into(),
                summary: "repository is not in a merge operation".into(),
            });
        }
        let merge_head = resolve_marker(path, "MERGE_HEAD");
        let operation = AgentOperation::ResolveMerge {
            merge_head_oid: merge_head.unwrap_or_else(|| "unknown".into()),
            baseline_head: before.head.clone(),
        };
        self.run_verified_operation(
            extension_id,
            path,
            before,
            operation,
            cancelled,
            |after| after.operation.is_none() && !after.conflicts,
        )
    }

    fn agent_resolve_rebase(
        &self,
        extension_id: &str,
        path: &Path,
        before: &automation::RepositoryState,
        cancelled: &AtomicBool,
    ) -> Result<HostResponse, String> {
        if before.operation.as_deref() != Some("rebase") {
            return Ok(HostResponse::Failure {
                code: "not_in_rebase".into(),
                summary: "repository is not in a rebase operation".into(),
            });
        }
        let operation = AgentOperation::ResolveRebase {
            rebase_head_oid: resolve_marker(path, "REBASE_HEAD"),
            upstream_oid: None,
            baseline_head: before.head.clone(),
        };
        self.run_verified_operation(
            extension_id,
            path,
            before,
            operation,
            cancelled,
            |after| after.operation.is_none() && !after.conflicts,
        )
    }

    fn run_verified_operation(
        &self,
        extension_id: &str,
        path: &Path,
        before: &automation::RepositoryState,
        operation: AgentOperation,
        cancelled: &AtomicBool,
        verify: impl FnOnce(&automation::RepositoryState) -> bool,
    ) -> Result<HostResponse, String> {
        let challenge = AgentOperationChallenge::new();
        let prompt = operation
            .prompt_with_challenge(None, &challenge)
            .map_err(|error| error.to_string())?;
        let result = self.run_agent(
            extension_id,
            path,
            &prompt,
            DEFAULT_AGENT_TIMEOUT,
            cancelled,
        )?;
        let after =
            automation::capture(path).map_err(|error| error.to_string())?;
        let verified = result.completed && verify(&after);
        let summary = if verified {
            "agent operation completed and repository state was verified"
        } else {
            "agent operation was not verified"
        };
        let _ = before;
        Ok(agent_response(result, verified, summary))
    }

    fn run_agent(
        &self,
        extension_id: &str,
        repository: &Path,
        prompt: &str,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<AgentResult, String> {
        let settings = self
            .agent_settings
            .lock()
            .map_err(|_| "agent settings are unavailable".to_string())?
            .clone();
        let profile_id = settings.default_profile_id();
        let profile = settings.profile(&profile_id).ok_or_else(|| {
            format!("configured Agent profile is unavailable: {profile_id}")
        })?;
        let overrides = settings.launch_overrides_for(&profile);
        let spec = profile
            .launch_spec_for_prompt_with_overrides(prompt, &overrides)
            .map_err(|error| error.to_string())?;
        run_agent_process(
            extension_id,
            spec,
            Some(repository),
            timeout,
            cancelled,
            &self.event_tx,
        )
    }
}

impl ExtensionHost for HostBridge {
    fn request(
        &self,
        request: ExtensionHostRequest,
    ) -> Result<HostResponse, String> {
        if request.cancelled.load(Ordering::Acquire) {
            return Err("extension run cancelled".into());
        }
        let response = match request.request.clone() {
            HostRequest::WorkspaceRepositoryTabs => json_response(
                serde_json::to_value(self.snapshots()?)
                    .map_err(|error| error.to_string())?,
            ),
            HostRequest::Repository {
                tab_id,
                operation,
                expected_branch,
                expected_head,
            } => self.handle_repository(
                &request,
                tab_id,
                operation,
                expected_branch,
                expected_head,
            )?,
            HostRequest::AgentPrompt(AgentRequest {
                repository,
                options:
                    AgentPromptOptions {
                        prompt,
                        timeout_seconds,
                    },
            }) => {
                let path = repository
                    .map(|tab_id| {
                        self.repository(tab_id).map(|snapshot| snapshot.path)
                    })
                    .transpose()?;
                let result = match self.run_agent(
                    &request.extension_id,
                    path.as_deref()
                        .map(Path::new)
                        .unwrap_or_else(|| Path::new(".")),
                    &prompt,
                    Duration::from_secs(timeout_seconds.clamp(1, 30 * 60)),
                    request.cancelled.as_ref(),
                ) {
                    Ok(result) => agent_response(
                        result,
                        false,
                        "generic Agent prompt completed without verified repository semantics",
                    ),
                    Err(summary) => HostResponse::Failure {
                        code: "agent_prompt_failed".into(),
                        summary,
                    },
                };
                result
            }
            HostRequest::TimeNow => json_response(serde_json::json!({
                "unix_ms": Utc::now().timestamp_millis(),
                "utc_rfc3339": Utc::now().to_rfc3339(),
                "local_rfc3339": Local::now().to_rfc3339(),
                "offset_seconds": Local::now().offset().local_minus_utc(),
            })),
            HostRequest::SystemInfo => json_response(serde_json::json!({
                "app_name": build_info::APP_NAME,
                "app_version": build_info::APP_VERSION,
                "platform": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
                "locale": sys_locale::get_locale().unwrap_or_else(|| "en-US".into()),
            })),
            HostRequest::StorageGet(key) => {
                self.storage.get(&request.extension_id, key)?
            }
            HostRequest::StorageSet { key, value } => {
                self.storage.set(&request.extension_id, &key, value)?
            }
            HostRequest::StorageDelete(key) => {
                self.storage.delete(&request.extension_id, key)?
            }
            HostRequest::Log {
                level,
                message,
                fields,
            } => {
                let _ = self.event_tx.send(HostEvent::Log {
                    extension_id: request.extension_id.clone(),
                    level,
                    message,
                    fields,
                });
                json_response(serde_json::json!({ "ok": true }))
            }
            HostRequest::LogFileAppend { path, content } => {
                match self.file_logger.append(&path, &content) {
                    Ok(bytes_written) => json_response(serde_json::json!({
                        "ok": true,
                        "bytes_written": bytes_written,
                    })),
                    Err(error) => {
                        log::warn!(
                            "[extension_log] append failed: id={}, code={}",
                            request.extension_id,
                            error.code()
                        );
                        HostResponse::Failure {
                            code: error.code().into(),
                            summary: error.to_string(),
                        }
                    }
                }
            }
            HostRequest::Notify { level, title, body } => {
                let _ = self.event_tx.send(HostEvent::Notify {
                    extension_id: request.extension_id.clone(),
                    level,
                    title,
                    body,
                });
                json_response(serde_json::json!({ "ok": true }))
            }
        };
        if request.cancelled.load(Ordering::Acquire) {
            return Err("extension run cancelled".into());
        }
        Ok(response)
    }

    fn begin_run(
        &self,
        extension_id: &str,
        run_id: u64,
        repositories: &[RepositorySnapshot],
    ) -> Result<ExtensionRunAdmission, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "extension host state is poisoned".to_string())?;
        let mut touched = HashSet::new();
        for repository in repositories {
            if !touched.insert(repository.tab_id) {
                continue;
            }
            if !state.repositories.contains_key(&repository.tab_id) {
                // Keep the closed handle in the Lua context. Its first
                // operation returns a per-repository `tab_closed` result so
                // the script can continue with the remaining repositories.
                continue;
            }
            if let Some(owner) = state.owners.get(&repository.tab_id) {
                // Ownership is checked per operation as well. Leave this
                // handle unreserved so one busy repository does not prevent
                // the extension from processing its other tabs.
                log::debug!(
                    "[extension_runtime] repository already owned by run {}",
                    owner.1
                );
                continue;
            }
            state
                .owners
                .insert(repository.tab_id, (extension_id.to_string(), run_id));
        }
        Ok(ExtensionRunAdmission::Accepted)
    }

    fn finish_run(&self, extension_id: &str, run_id: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.owners.retain(|_, owner| {
                owner != &(extension_id.to_string(), run_id)
            });
            state.run_identities.retain(|(id, current_run, _), _| {
                id != extension_id || *current_run != run_id
            });
        }
    }
}

fn resolve_commit(path: &Path, source: &str) -> Result<String, String> {
    if source.trim().is_empty()
        || source.starts_with('-')
        || source.contains('\0')
    {
        return Err("invalid commit or branch reference".into());
    }
    let result = automation::run(
        path,
        &[
            "rev-parse".into(),
            "--verify".into(),
            format!("{source}^{{commit}}"),
        ],
        Some(Duration::from_secs(30)),
        &AtomicBool::new(false),
    );
    if !result.ok {
        return Err(result.summary);
    }
    result
        .stdout
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "Git did not return a commit id".into())
}

fn resolve_marker(path: &Path, marker: &str) -> Option<String> {
    let git_dir = automation::run(
        path,
        &["rev-parse".into(), "--git-path".into(), marker.into()],
        Some(Duration::from_secs(30)),
        &AtomicBool::new(false),
    );
    if !git_dir.ok {
        return None;
    }
    let marker_path = PathBuf::from(git_dir.stdout.trim());
    let marker_path = if marker_path.is_absolute() {
        marker_path
    } else {
        Path::new(path).join(marker_path)
    };
    fs::read_to_string(marker_path)
        .ok()
        .map(|value| value.trim().to_string())
}

fn json_response(value: JsonValue) -> HostResponse {
    HostResponse::Json(value)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn temporary_root(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "augur-git-extension-host-log-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn file_log_request(path: &Path, content: &str) -> ExtensionHostRequest {
        ExtensionHostRequest {
            extension_id: "test-extension".into(),
            run_id: 1,
            cancelled: Arc::new(AtomicBool::new(false)),
            request: HostRequest::LogFileAppend {
                path: path.to_string_lossy().to_string(),
                content: content.into(),
            },
        }
    }

    #[test]
    fn file_log_host_request_appends_and_reports_bytes() {
        let root = temporary_root("append");
        let path = root.join("nested").join("run.log");
        let (host, _events) = HostBridge::new(AgentSettings::default());

        let response = host
            .request(file_log_request(&path, "hello\n"))
            .expect("host request");
        assert!(matches!(
            response,
            HostResponse::Json(JsonValue::Object(ref object))
                if object.get("ok") == Some(&JsonValue::Bool(true))
                    && object.get("bytes_written") == Some(&JsonValue::from(6))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_log_host_request_maps_filesystem_failures() {
        let root = temporary_root("directory");
        fs::create_dir_all(&root).unwrap();
        let (host, _events) = HostBridge::new(AgentSettings::default());

        let response = host
            .request(file_log_request(&root, "content"))
            .expect("host request");
        assert!(matches!(
            response,
            HostResponse::Failure { ref code, .. }
                if code == "log_write_failed"
        ));

        let _ = fs::remove_dir_all(root);
    }
}
