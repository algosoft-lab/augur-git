//! Long-lived extension workers and the global FIFO run queue.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::Local;

use crate::core::extension::{
    ExtensionPackage, ExtensionRunRecord, ExtensionRunTrigger,
    RepositoryRunRecord, RepositoryRunResult, SettingValue,
};

use super::api::{
    ExtensionEventPayload, ExtensionHost, ExtensionInvocation,
    ExtensionRunAdmission, ExtensionRuntime, ExtensionRuntimeError,
    ExtensionTrigger, RepositorySnapshot,
};

/// A package source plus the validated package metadata used to start a VM.
#[derive(Clone)]
pub struct ExtensionDefinition {
    pub package: ExtensionPackage,
    pub source: String,
}

/// Input for one manual or scheduled run.
#[derive(Clone)]
pub struct ExtensionRunRequest {
    pub extension_id: String,
    pub trigger: ExtensionTrigger,
    pub scheduled_at: Option<chrono::DateTime<Local>>,
    pub settings: std::collections::BTreeMap<String, SettingValue>,
    pub repositories: Vec<RepositorySnapshot>,
    pub events: Vec<ExtensionEventPayload>,
    pub handler: String,
}

#[derive(Clone, Debug)]
pub enum ExtensionEvent {
    RunQueued {
        extension_id: String,
        run_id: u64,
    },
    RunStarted {
        extension_id: String,
        run_id: u64,
    },
    RunFinished {
        extension_id: String,
        run_id: u64,
        record: ExtensionRunRecord,
        error: Option<String>,
    },
    #[allow(dead_code)]
    WorkerError {
        extension_id: String,
        summary: String,
    },
}

struct Worker {
    tx: Sender<WorkerCommand>,
}

enum WorkerCommand {
    Run {
        invocation: ExtensionInvocation,
        handler: String,
        completed: Sender<Result<serde_json::Value, ExtensionRuntimeError>>,
    },
    Shutdown,
}

struct QueueJob {
    extension_id: String,
    run_id: u64,
    request: ExtensionRunRequest,
    cancelled: Arc<AtomicBool>,
}

enum QueueCommand {
    Enqueue(QueueJob),
    Shutdown,
}

/// Manager shared by the Workspace and the extension page.
pub struct ExtensionManager {
    definitions: Arc<Mutex<HashMap<String, ExtensionDefinition>>>,
    workers: Arc<Mutex<HashMap<String, Worker>>>,
    host: Arc<dyn ExtensionHost>,
    queue_tx: Sender<QueueCommand>,
    pending: Arc<Mutex<HashSet<String>>>,
    coalesced: Arc<Mutex<HashMap<String, ExtensionRunRequest>>>,
    cancellations: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    run_extensions: Arc<Mutex<HashMap<u64, String>>>,
    next_run_id: Arc<AtomicU64>,
}

impl ExtensionManager {
    /// Start a dedicated VM thread for every valid definition and a single
    /// FIFO dispatcher that serializes all extension invocations.
    pub fn new(
        definitions: Vec<ExtensionDefinition>,
        host: Arc<dyn ExtensionHost>,
    ) -> Result<(Self, Receiver<ExtensionEvent>), String> {
        let mut definition_map: HashMap<String, ExtensionDefinition> =
            HashMap::new();
        let mut worker_map: HashMap<String, Worker> = HashMap::new();
        for definition in definitions {
            let id = definition.package.manifest.id.clone();
            if definition_map.contains_key(&id) {
                return Err(format!("duplicate extension id: {id}"));
            }
            let worker = spawn_worker(&definition, host.clone())?;
            definition_map.insert(id.clone(), definition);
            worker_map.insert(id, worker);
        }

        let (queue_tx, queue_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let definitions = Arc::new(Mutex::new(definition_map));
        let workers = Arc::new(Mutex::new(worker_map));
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let coalesced = Arc::new(Mutex::new(HashMap::new()));
        let cancellations = Arc::new(Mutex::new(HashMap::new()));
        let run_extensions = Arc::new(Mutex::new(HashMap::new()));
        let next_run_id = Arc::new(AtomicU64::new(1));
        let dispatch_workers = workers.clone();
        let dispatch_pending = pending.clone();
        let dispatch_coalesced = coalesced.clone();
        let dispatch_cancellations = cancellations.clone();
        let dispatch_run_extensions = run_extensions.clone();
        let dispatch_host = host.clone();
        let dispatch_next_run_id = next_run_id.clone();
        thread::Builder::new()
            .name("augur-extension-queue".into())
            .spawn(move || {
                dispatcher_loop(
                    queue_rx,
                    dispatch_workers,
                    dispatch_pending,
                    dispatch_coalesced,
                    dispatch_cancellations,
                    dispatch_run_extensions,
                    dispatch_next_run_id,
                    dispatch_host,
                    event_tx,
                )
            })
            .map_err(|error| {
                format!("failed to start extension queue: {error}")
            })?;

        Ok((
            Self {
                definitions,
                workers,
                host,
                queue_tx,
                pending,
                coalesced,
                cancellations,
                run_extensions,
                next_run_id,
            },
            event_rx,
        ))
    }

    #[allow(dead_code)]
    pub fn definitions(&self) -> Vec<ExtensionDefinition> {
        self.definitions
            .lock()
            .map(|definitions| definitions.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Replace the package set and recreate long-lived Lua workers.
    ///
    /// Reloading while a run is active could invalidate its VM or package
    /// source, so callers must wait until the queue is idle. New workers are
    /// started before the shared maps are swapped; a failed package leaves the
    /// currently running manager untouched.
    pub fn reload(
        &self,
        definitions: Vec<ExtensionDefinition>,
    ) -> Result<(), String> {
        if self
            .pending
            .lock()
            .map_err(|_| "extension queue state is unavailable".to_string())?
            .len()
            != 0
        {
            return Err("cannot reload extensions while a run is active".into());
        }

        let mut definition_map: HashMap<String, ExtensionDefinition> =
            HashMap::new();
        let mut worker_map: HashMap<String, Worker> = HashMap::new();
        for definition in definitions {
            let id = definition.package.manifest.id.clone();
            if definition_map.contains_key(&id) {
                for worker in worker_map.values() {
                    let _ = worker.tx.send(WorkerCommand::Shutdown);
                }
                return Err(format!("duplicate extension id: {id}"));
            }
            let worker = match spawn_worker(&definition, self.host.clone()) {
                Ok(worker) => worker,
                Err(error) => {
                    for worker in worker_map.values() {
                        let _ = worker.tx.send(WorkerCommand::Shutdown);
                    }
                    return Err(error);
                }
            };
            definition_map.insert(id.clone(), definition);
            worker_map.insert(id, worker);
        }

        let mut old_workers = self
            .workers
            .lock()
            .map_err(|_| "extension workers are unavailable".to_string())?;
        let mut old_definitions = self
            .definitions
            .lock()
            .map_err(|_| "extension definitions are unavailable".to_string())?;
        for worker in old_workers.values() {
            let _ = worker.tx.send(WorkerCommand::Shutdown);
        }
        *old_workers = worker_map;
        *old_definitions = definition_map;
        Ok(())
    }

    /// Queue a run. Repeated manual invocations are coalesced while one is in
    /// flight. Event invocations retain one trailing, merged batch per
    /// extension and trigger so status bursts are not lost.
    pub fn run(
        &self,
        mut request: ExtensionRunRequest,
    ) -> Result<Option<u64>, String> {
        let exists = self
            .definitions
            .lock()
            .map_err(|_| "extension definitions are unavailable".to_string())?
            .contains_key(&request.extension_id);
        if !exists {
            return Err(format!("unknown extension: {}", request.extension_id));
        }
        if request.handler.trim().is_empty() {
            return Err("extension handler must not be empty".into());
        }
        let key = run_key(&request);
        let extension_id_for_log = request.extension_id.clone();
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "extension queue state is unavailable".to_string())?;
        if !pending.insert(key.clone()) {
            if !request.events.is_empty() {
                let mut coalesced = self.coalesced.lock().map_err(|_| {
                    "extension coalescing state is unavailable".to_string()
                })?;
                if let Some(existing) = coalesced.get_mut(&key) {
                    merge_event_requests(existing, request);
                } else {
                    coalesced.insert(key.clone(), request);
                }
            }
            log::info!(
                "[extensions] coalesced overlapping trigger: id={}, key={key}",
                extension_id_for_log,
            );
            return Ok(None);
        }
        request
            .repositories
            .sort_by_key(|repository| repository.tab_id);
        let run_id = self.next_run_id.fetch_add(1, Ordering::Relaxed);
        let extension_id = request.extension_id.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        if let Ok(mut cancellations) = self.cancellations.lock() {
            cancellations.insert(run_id, cancelled.clone());
        }
        if let Ok(mut run_extensions) = self.run_extensions.lock() {
            run_extensions.insert(run_id, extension_id.clone());
        }
        let job = QueueJob {
            extension_id: extension_id.clone(),
            run_id,
            request,
            cancelled,
        };
        if self.queue_tx.send(QueueCommand::Enqueue(job)).is_err() {
            pending.remove(&key);
            if let Ok(mut coalesced) = self.coalesced.lock() {
                coalesced.remove(&key);
            }
            if let Ok(mut cancellations) = self.cancellations.lock() {
                cancellations.remove(&run_id);
            }
            if let Ok(mut run_extensions) = self.run_extensions.lock() {
                run_extensions.remove(&run_id);
            }
            return Err("extension queue is unavailable".into());
        }
        Ok(Some(run_id))
    }

    pub fn shutdown(&self) {
        if let Ok(cancellations) = self.cancellations.lock() {
            for cancelled in cancellations.values() {
                cancelled.store(true, Ordering::Release);
            }
        }
        let _ = self.queue_tx.send(QueueCommand::Shutdown);
        if let Ok(workers) = self.workers.lock() {
            for worker in workers.values() {
                let _ = worker.tx.send(WorkerCommand::Shutdown);
            }
        }
    }

    pub fn active_count(&self) -> usize {
        self.cancellations
            .lock()
            .map(|cancellations| cancellations.len())
            .unwrap_or(0)
    }

    pub fn active_labels(&self) -> Vec<String> {
        self.active_runs()
            .into_iter()
            .map(|(extension_id, run_id)| {
                format!("{extension_id} (run {run_id})")
            })
            .collect()
    }

    /// Return queued and running extension runs for rebuilding a management
    /// window after it has been closed and reopened.
    pub fn active_runs(&self) -> Vec<(String, u64)> {
        let Ok(run_extensions) = self.run_extensions.lock() else {
            return Vec::new();
        };
        let mut runs = run_extensions
            .iter()
            .map(|(run_id, extension_id)| (extension_id.clone(), *run_id))
            .collect::<Vec<_>>();
        runs.sort_by(|left, right| {
            left.0.cmp(&right.0).then(left.1.cmp(&right.1))
        });
        runs
    }

    pub fn cancel_all(&self) -> usize {
        let Ok(cancellations) = self.cancellations.lock() else {
            return 0;
        };
        for flag in cancellations.values() {
            flag.store(true, Ordering::Release);
        }
        cancellations.len()
    }

    pub fn cancel_extension(&self, extension_id: &str) -> usize {
        let Ok(run_extensions) = self.run_extensions.lock() else {
            return 0;
        };
        let run_ids = run_extensions
            .iter()
            .filter_map(|(run_id, id)| (id == extension_id).then_some(*run_id))
            .collect::<Vec<_>>();
        let Ok(cancellations) = self.cancellations.lock() else {
            return 0;
        };
        let mut cancelled = 0;
        for run_id in run_ids {
            if let Some(flag) = cancellations.get(&run_id) {
                flag.store(true, Ordering::Release);
                cancelled += 1;
            }
        }
        cancelled
    }

    /// Request cancellation at the next Lua instruction or host-operation
    /// boundary. Running Git subprocesses are terminated by the host bridge.
    #[allow(dead_code)]
    pub fn cancel(&self, run_id: u64) -> bool {
        self.cancellations
            .lock()
            .ok()
            .and_then(|cancellations| cancellations.get(&run_id).cloned())
            .map(|cancelled| {
                cancelled.store(true, Ordering::Release);
                true
            })
            .unwrap_or(false)
    }
}

fn spawn_worker(
    definition: &ExtensionDefinition,
    host: Arc<dyn ExtensionHost>,
) -> Result<Worker, String> {
    let package_root = definition.package.root.clone();
    let extension_id = definition.package.manifest.id.clone();
    let source = definition.source.clone();
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
    thread::Builder::new()
        .name(format!("augur-extension-{extension_id}"))
        .spawn(move || {
            let _ = ready_tx.send(Ok(()));
            // Do not evaluate untrusted package source during discovery or
            // reload. The first invocation is only queued after the
            // Workspace has confirmed the user's trust and enabled state.
            let mut runtime = None;
            while let Ok(command) = rx.recv() {
                match command {
                    WorkerCommand::Run {
                        invocation,
                        handler,
                        completed,
                    } => {
                        if runtime.is_none() {
                            runtime = match ExtensionRuntime::load(
                                extension_id.clone(),
                                &source,
                                package_root.clone(),
                                host.clone(),
                            ) {
                                Ok(runtime) => Some(runtime),
                                Err(error) => {
                                    let _ = completed.send(Err(error));
                                    continue;
                                }
                            };
                        }
                        let Some(runtime) = runtime.as_ref() else {
                            let _ = completed.send(Err(
                                ExtensionRuntimeError::Lua(
                                    "extension runtime is unavailable".into(),
                                ),
                            ));
                            continue;
                        };
                        let result = runtime.run(invocation, &handler);
                        let _ = completed.send(result);
                    }
                    WorkerCommand::Shutdown => break,
                }
            }
        })
        .map_err(|error| {
            format!("failed to start extension worker: {error}")
        })?;
    ready_rx
        .recv()
        .map_err(|_| "extension worker exited during startup".to_string())??;
    Ok(Worker { tx })
}

fn dispatcher_loop(
    queue_rx: Receiver<QueueCommand>,
    workers: Arc<Mutex<HashMap<String, Worker>>>,
    pending: Arc<Mutex<HashSet<String>>>,
    coalesced: Arc<Mutex<HashMap<String, ExtensionRunRequest>>>,
    cancellations: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    run_extensions: Arc<Mutex<HashMap<u64, String>>>,
    next_run_id: Arc<AtomicU64>,
    host: Arc<dyn ExtensionHost>,
    event_tx: Sender<ExtensionEvent>,
) {
    let mut queue = VecDeque::new();
    loop {
        if queue.is_empty() {
            match queue_rx.recv() {
                Ok(QueueCommand::Enqueue(job)) => queue.push_back(job),
                Ok(QueueCommand::Shutdown) | Err(_) => break,
            }
        }
        while let Ok(command) = queue_rx.try_recv() {
            match command {
                QueueCommand::Enqueue(job) => queue.push_back(job),
                QueueCommand::Shutdown => return,
            }
        }
        let Some(job) = queue.pop_front() else {
            continue;
        };
        let request = job.request;
        let started_at = Local::now();
        let _ = event_tx.send(ExtensionEvent::RunQueued {
            extension_id: job.extension_id.clone(),
            run_id: job.run_id,
        });
        let result = match host.begin_run(
            &job.extension_id,
            job.run_id,
            &request.repositories,
        ) {
            Err(error) => Err(ExtensionRuntimeError::Lua(error)),
            Ok(ExtensionRunAdmission::Rejected { code, summary }) => {
                Ok(serde_json::json!({
                    "ok": false,
                    "code": code,
                    "summary": summary,
                }))
            }
            Ok(ExtensionRunAdmission::Accepted) => {
                let _ = event_tx.send(ExtensionEvent::RunStarted {
                    extension_id: job.extension_id.clone(),
                    run_id: job.run_id,
                });
                let invocation = ExtensionInvocation {
                    extension_id: job.extension_id.clone(),
                    run_id: job.run_id,
                    trigger: request.trigger.clone(),
                    scheduled_at: request.scheduled_at,
                    started_at,
                    settings: request.settings.clone(),
                    repositories: request.repositories.clone(),
                    events: request.events.clone(),
                    cancelled: job.cancelled.clone(),
                };
                let (completed_tx, completed_rx) = mpsc::channel();
                let sent = workers
                    .lock()
                    .ok()
                    .and_then(|workers| {
                        workers
                            .get(&job.extension_id)
                            .map(|worker| worker.tx.clone())
                    })
                    .map(|tx| {
                        tx.send(WorkerCommand::Run {
                            invocation,
                            handler: request.handler.clone(),
                            completed: completed_tx,
                        })
                    });
                match sent {
                    Some(Ok(())) => completed_rx.recv().unwrap_or_else(|_| {
                        Err(ExtensionRuntimeError::Lua(
                            "extension worker exited".into(),
                        ))
                    }),
                    Some(Err(_)) | None => Err(ExtensionRuntimeError::Lua(
                        "extension worker is unavailable".into(),
                    )),
                }
            }
        };
        host.finish_run(&job.extension_id, job.run_id);
        let (error, summary, handler_result) = match result {
            Ok(value) => {
                let ok = value
                    .get("ok")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true);
                let summary = value
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("extension completed")
                    .to_string();
                let error = (!ok).then(|| summary.clone());
                (error, summary, Some(value))
            }
            Err(error) => (Some(error.to_string()), error.to_string(), None),
        };
        let repositories = request
            .repositories
            .iter()
            .map(|repository| {
                let detail = handler_result
                    .as_ref()
                    .and_then(|value| value.get("repositories"))
                    .and_then(serde_json::Value::as_array)
                    .and_then(|items| {
                        items.iter().find(|item| {
                            item.get("repository")
                                .and_then(serde_json::Value::as_str)
                                == Some(repository.display_name.as_str())
                        })
                    });
                let steps = detail
                    .and_then(|item| item.get("steps"))
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let failed = detail
                    .and_then(|item| item.get("ok"))
                    .and_then(serde_json::Value::as_bool)
                    .map(|ok| !ok)
                    .unwrap_or(error.is_some());
                let top_level_code = handler_result
                    .as_ref()
                    .and_then(|value| value.get("code"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("extension_error");
                let result_summary = detail
                    .and_then(|item| item.get("summary"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(if failed {
                        &summary
                    } else {
                        "extension completed"
                    })
                    .to_string();
                RepositoryRunRecord {
                    display_name: repository.display_name.clone(),
                    result: if failed {
                        RepositoryRunResult::Failed {
                            code: detail
                                .and_then(|item| item.get("code"))
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or(top_level_code)
                                .to_string(),
                            summary: result_summary,
                        }
                    } else {
                        RepositoryRunResult::Success {
                            summary: result_summary,
                        }
                    },
                    steps,
                }
            })
            .collect();
        let record = ExtensionRunRecord {
            run_id: job.run_id,
            trigger: ExtensionRunTrigger::from(&request.trigger),
            started_at: started_at.to_rfc3339(),
            finished_at: Local::now().to_rfc3339(),
            repositories,
            summary,
        };
        let _ = event_tx.send(ExtensionEvent::RunFinished {
            extension_id: job.extension_id.clone(),
            run_id: job.run_id,
            record,
            error,
        });
        let key = run_key(&request);
        let follow_up = coalesced
            .lock()
            .ok()
            .and_then(|mut coalesced| coalesced.remove(&key));
        if let Some(mut request) = follow_up {
            request
                .repositories
                .sort_by_key(|repository| repository.tab_id);
            let run_id = next_run_id.fetch_add(1, Ordering::Relaxed);
            let cancelled = Arc::new(AtomicBool::new(false));
            if let Ok(mut all_cancellations) = cancellations.lock() {
                all_cancellations.insert(run_id, cancelled.clone());
            }
            if let Ok(mut all_run_extensions) = run_extensions.lock() {
                all_run_extensions.insert(run_id, request.extension_id.clone());
            }
            queue.push_back(QueueJob {
                extension_id: request.extension_id.clone(),
                run_id,
                request,
                cancelled,
            });
            log::info!(
                "[extensions] queued trailing coalesced trigger: key={key}, run_id={run_id}"
            );
        } else if let Ok(mut pending) = pending.lock() {
            pending.remove(&key);
        }
        if let Ok(mut cancellations) = cancellations.lock() {
            cancellations.remove(&job.run_id);
        }
        if let Ok(mut run_extensions) = run_extensions.lock() {
            run_extensions.remove(&job.run_id);
        }
    }
}

fn run_key(request: &ExtensionRunRequest) -> String {
    match &request.trigger {
        ExtensionTrigger::Manual => {
            format!("{}:manual", request.extension_id)
        }
        ExtensionTrigger::Schedule { trigger_id, .. }
        | ExtensionTrigger::Repository { trigger_id, .. } => {
            format!("{}:event:{trigger_id}", request.extension_id)
        }
    }
}

fn merge_event_requests(
    existing: &mut ExtensionRunRequest,
    incoming: ExtensionRunRequest,
) {
    existing.repositories = incoming.repositories;
    existing.settings = incoming.settings;
    existing.scheduled_at = incoming.scheduled_at;
    existing.handler = incoming.handler;
    let mut merged = existing.events.clone();
    for event in incoming.events {
        let tab_id = event
            .repository
            .as_ref()
            .map(|repository| repository.tab_id);
        if let Some(tab_id) = tab_id {
            merged.retain(|candidate| {
                candidate
                    .repository
                    .as_ref()
                    .map(|repository| repository.tab_id)
                    != Some(tab_id)
            });
        }
        merged.push(event);
    }
    existing.events = merged;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use super::*;
    use crate::core::extension::{
        ExtensionManifest, ExtensionPackage, ExtensionSource,
    };
    use crate::extension::api::{
        ExtensionHostRequest, HostRequest, HostResponse,
    };

    struct BlockingHost {
        started: Sender<()>,
        release: Arc<std::sync::Barrier>,
    }

    impl ExtensionHost for BlockingHost {
        fn request(
            &self,
            request: ExtensionHostRequest,
        ) -> Result<HostResponse, String> {
            if matches!(request.request, HostRequest::Log { .. }) {
                let _ = self.started.send(());
                self.release.wait();
            }
            Ok(HostResponse::Json(serde_json::json!({ "ok": true })))
        }
    }

    fn definition(id: &str, source: &str) -> ExtensionDefinition {
        let manifest = ExtensionManifest::parse(&format!(
            "id=\"{id}\"\nversion=\"1.0.0\"\napi_version=1\nname=\"{id}\"\ndescription=\"test\"\nmanual_handler=\"on_run\""
        ))
        .expect("test manifest");
        ExtensionDefinition {
            package: ExtensionPackage {
                manifest,
                root: None,
                source: ExtensionSource::Bundled,
                fingerprint: id.into(),
                bundled: true,
            },
            source: source.into(),
        }
    }

    fn request(id: &str) -> ExtensionRunRequest {
        ExtensionRunRequest {
            extension_id: id.into(),
            trigger: ExtensionTrigger::Manual,
            scheduled_at: None,
            settings: BTreeMap::new(),
            repositories: Vec::new(),
            events: Vec::new(),
            handler: "on_run".into(),
        }
    }

    fn snapshot(tab_id: u64, head: &str) -> RepositorySnapshot {
        RepositorySnapshot {
            tab_id,
            path: format!("repo-{tab_id}"),
            display_name: format!("repo-{tab_id}"),
            branch: "main".into(),
            head: Some(head.into()),
            upstream: None,
            ahead: 0,
            behind: 0,
            dirty: false,
            conflicts: false,
            busy: false,
            remotes: vec!["origin".into()],
        }
    }

    #[test]
    fn merges_trailing_event_batches_by_repository() {
        let now = Local::now();
        let mut first = request("test-extension");
        first.trigger = ExtensionTrigger::Repository {
            trigger_id: "status".into(),
            event_type: "repository.status_changed".into(),
        };
        first.events = vec![ExtensionEventPayload {
            trigger_id: "status".into(),
            event_type: "repository.status_changed".into(),
            occurred_at: now,
            repository: Some(snapshot(1, "a")),
            previous: None,
            current: None,
            origin_extension_id: None,
            origin_run_id: None,
        }];
        let mut incoming = first.clone();
        incoming.events = vec![
            ExtensionEventPayload {
                trigger_id: "status".into(),
                event_type: "repository.status_changed".into(),
                occurred_at: now,
                repository: Some(snapshot(1, "b")),
                previous: None,
                current: None,
                origin_extension_id: None,
                origin_run_id: None,
            },
            ExtensionEventPayload {
                trigger_id: "status".into(),
                event_type: "repository.status_changed".into(),
                occurred_at: now,
                repository: Some(snapshot(2, "c")),
                previous: None,
                current: None,
                origin_extension_id: None,
                origin_run_id: None,
            },
        ];
        merge_event_requests(&mut first, incoming);
        assert_eq!(first.events.len(), 2);
        assert_eq!(
            first.events[0]
                .repository
                .as_ref()
                .and_then(|repository| repository.head.as_deref()),
            Some("b")
        );
        assert_eq!(
            first.events[1]
                .repository
                .as_ref()
                .map(|repository| repository.tab_id),
            Some(2)
        );
    }

    #[test]
    fn coalesces_overlapping_runs_for_one_extension() {
        let (started_tx, started_rx) = mpsc::channel();
        let release = Arc::new(std::sync::Barrier::new(2));
        let host: Arc<dyn ExtensionHost> = Arc::new(BlockingHost {
            started: started_tx,
            release: release.clone(),
        });
        let (manager, events) = ExtensionManager::new(
            vec![definition(
                "test-extension",
                r#"local augur = require("augur"); return {on_run = function() augur.log("info", "started") return {ok=true} end}"#,
            )],
            host,
        )
        .expect("manager");
        assert!(manager.run(request("test-extension")).unwrap().is_some());
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("first run should reach host");
        assert!(manager.run(request("test-extension")).unwrap().is_none());
        release.wait();
        loop {
            match events.recv_timeout(Duration::from_secs(2)) {
                Ok(ExtensionEvent::RunFinished { .. }) => break,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("run did not finish"),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("manager event stream disconnected")
                }
            }
        }
        manager.shutdown();
    }

    #[test]
    fn invalid_source_is_deferred_until_first_run() {
        let host: Arc<dyn ExtensionHost> = Arc::new(BlockingHost {
            started: mpsc::channel().0,
            release: Arc::new(std::sync::Barrier::new(1)),
        });
        let (manager, events) = ExtensionManager::new(
            vec![definition("invalid-extension", "return {")],
            host,
        )
        .expect("worker startup must not evaluate source");
        assert!(manager.run(request("invalid-extension")).unwrap().is_some());
        let mut saw_error = false;
        for _ in 0..4 {
            match events.recv_timeout(Duration::from_secs(2)) {
                Ok(ExtensionEvent::RunFinished { error: Some(_), .. }) => {
                    saw_error = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_error, "invalid source should fail on invocation");
        manager.shutdown();
    }
}
