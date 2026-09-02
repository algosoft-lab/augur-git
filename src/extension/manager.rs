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
    ExtensionHost, ExtensionInvocation, ExtensionRuntime,
    ExtensionRuntimeError, ExtensionTrigger, RepositorySnapshot,
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
    cancellations: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    next_run_id: AtomicU64,
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
        let cancellations = Arc::new(Mutex::new(HashMap::new()));
        let dispatch_workers = workers.clone();
        let dispatch_pending = pending.clone();
        let dispatch_cancellations = cancellations.clone();
        let dispatch_host = host.clone();
        thread::Builder::new()
            .name("augur-extension-queue".into())
            .spawn(move || {
                dispatcher_loop(
                    queue_rx,
                    dispatch_workers,
                    dispatch_pending,
                    dispatch_cancellations,
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
                cancellations,
                next_run_id: AtomicU64::new(1),
            },
            event_rx,
        ))
    }

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

    /// Queue a run. A second trigger while the same extension is queued or
    /// running is merged into the existing invocation.
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
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "extension queue state is unavailable".to_string())?;
        if !pending.insert(request.extension_id.clone()) {
            log::info!(
                "[extensions] coalesced overlapping trigger: id={}",
                request.extension_id
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
        let job = QueueJob {
            extension_id: extension_id.clone(),
            run_id,
            request,
            cancelled,
        };
        if self.queue_tx.send(QueueCommand::Enqueue(job)).is_err() {
            pending.remove(&extension_id);
            if let Ok(mut cancellations) = self.cancellations.lock() {
                cancellations.remove(&run_id);
            }
            return Err("extension queue is unavailable".into());
        }
        Ok(Some(run_id))
    }

    pub fn shutdown(&self) {
        let _ = self.queue_tx.send(QueueCommand::Shutdown);
        if let Ok(workers) = self.workers.lock() {
            for worker in workers.values() {
                let _ = worker.tx.send(WorkerCommand::Shutdown);
            }
        }
    }

    /// Request cancellation at the next Lua instruction or host-operation
    /// boundary. Running Git subprocesses are terminated by the host bridge.
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
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name(format!("augur-extension-{extension_id}"))
        .spawn(move || {
            let runtime = ExtensionRuntime::load(
                extension_id.clone(),
                &source,
                package_root,
                host,
            );
            let runtime = match runtime {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            while let Ok(command) = rx.recv() {
                match command {
                    WorkerCommand::Run {
                        invocation,
                        handler,
                        completed,
                    } => {
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
    cancellations: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
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
            Ok(()) => {
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
                                .unwrap_or("extension_error")
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
        if let Ok(mut pending) = pending.lock() {
            pending.remove(&job.extension_id);
        }
        if let Ok(mut cancellations) = cancellations.lock() {
            cancellations.remove(&job.run_id);
        }
    }
}
