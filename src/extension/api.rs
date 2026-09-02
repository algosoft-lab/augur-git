//! Versioned Lua API exposed to trusted extensions.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, Utc};
use mlua::{
    Function, HookTriggers, Lua, LuaSerdeExt, Table, UserData, UserDataMethods,
    Value, VmState,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::core::extension::{ExtensionRunTrigger, SettingValue};

pub const LUA_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_AGENT_TRANSCRIPT_BYTES: usize = 1024 * 1024;

/// Repository identity captured when an extension run is queued.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositorySnapshot {
    pub tab_id: u64,
    pub path: String,
    pub display_name: String,
    pub branch: String,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub dirty: bool,
    pub conflicts: bool,
    pub busy: bool,
    pub remotes: Vec<String>,
}

/// A coalesced application event delivered to a Lua invocation. The origin
/// fields are used by the host to suppress self-triggered loops and are not
/// exposed to Lua or persisted in run history.
#[derive(Clone, Debug)]
pub struct ExtensionEventPayload {
    pub trigger_id: String,
    pub event_type: String,
    pub occurred_at: DateTime<Local>,
    pub repository: Option<RepositorySnapshot>,
    pub previous: Option<RepositorySnapshot>,
    pub current: Option<RepositorySnapshot>,
    pub origin_extension_id: Option<String>,
    pub origin_run_id: Option<u64>,
}

/// Operation requested by a Lua repository handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryOperation {
    Status,
    Git {
        args: Vec<String>,
        label: String,
        timeout_seconds: u64,
    },
    PullRebase,
    Push {
        remote: Option<String>,
        branch: Option<String>,
    },
    AgentCommit {
        hint: Option<String>,
    },
    AgentMerge {
        source: String,
    },
    AgentRebase {
        source: String,
    },
    ResolveMerge,
    ResolveRebase,
}

/// Generic Agent prompt requested by a trusted extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPromptOptions {
    pub prompt: String,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRequest {
    pub repository: Option<u64>,
    pub options: AgentPromptOptions,
}

/// Requests crossing from the Lua thread into the GPUI host.
#[derive(Clone, Debug)]
pub enum HostRequest {
    WorkspaceRepositoryTabs,
    Repository {
        tab_id: u64,
        operation: RepositoryOperation,
        /// Identity captured when the repository handle was created. The
        /// host rejects mutating operations when the branch or HEAD changed.
        expected_branch: String,
        expected_head: Option<String>,
    },
    AgentPrompt(AgentRequest),
    TimeNow,
    SystemInfo,
    StorageGet(Option<String>),
    StorageSet {
        key: String,
        value: JsonValue,
    },
    StorageDelete(Option<String>),
    Log {
        level: String,
        message: String,
        fields: JsonValue,
    },
    Notify {
        level: String,
        title: String,
        body: String,
    },
}

/// A request is annotated with the extension and run that produced it for
/// audit logs and cancellation checks.
#[derive(Clone, Debug)]
pub struct ExtensionHostRequest {
    pub extension_id: String,
    pub run_id: u64,
    pub cancelled: Arc<AtomicBool>,
    pub request: HostRequest,
}

/// Host responses carry JSON data so the bridge remains independent from Lua.
#[derive(Clone, Debug)]
pub enum HostResponse {
    Json(JsonValue),
    Failure { code: String, summary: String },
}

/// Admission result for a queued run. Repository races are expected business
/// failures and are surfaced in run history instead of being raised as Lua
/// errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionRunAdmission {
    Accepted,
    Rejected { code: String, summary: String },
}

/// Implemented by Workspace. Calls are synchronous from Lua's perspective,
/// but the implementation may wait for an asynchronous GPUI/Git operation.
pub trait ExtensionHost: Send + Sync {
    fn request(
        &self,
        request: ExtensionHostRequest,
    ) -> Result<HostResponse, String>;

    /// Reserve captured repositories for one run. Implementations can use
    /// this to serialize extension writes with one another; the default keeps
    /// the trait convenient for embedders and unit-test hosts.
    fn begin_run(
        &self,
        _extension_id: &str,
        _run_id: u64,
        _repositories: &[RepositorySnapshot],
    ) -> Result<ExtensionRunAdmission, String> {
        Ok(ExtensionRunAdmission::Accepted)
    }

    /// Release reservations made by `begin_run`.
    fn finish_run(&self, _extension_id: &str, _run_id: u64) {}
}

#[derive(Clone, Debug)]
pub enum ExtensionTrigger {
    Manual,
    Schedule {
        trigger_id: String,
        event_type: String,
    },
    Repository {
        trigger_id: String,
        event_type: String,
    },
}

impl From<&ExtensionTrigger> for ExtensionRunTrigger {
    fn from(trigger: &ExtensionTrigger) -> Self {
        match trigger {
            ExtensionTrigger::Manual => Self::Manual,
            ExtensionTrigger::Schedule {
                trigger_id,
                event_type,
            } => Self::Schedule {
                trigger_id: trigger_id.clone(),
                event_type: event_type.clone(),
            },
            ExtensionTrigger::Repository {
                trigger_id,
                event_type,
            } => Self::Repository {
                trigger_id: trigger_id.clone(),
                event_type: event_type.clone(),
            },
        }
    }
}

#[derive(Clone)]
pub struct ExtensionInvocation {
    #[allow(dead_code)]
    pub extension_id: String,
    pub run_id: u64,
    pub trigger: ExtensionTrigger,
    pub scheduled_at: Option<DateTime<Local>>,
    pub started_at: DateTime<Local>,
    pub settings: BTreeMap<String, SettingValue>,
    pub repositories: Vec<RepositorySnapshot>,
    pub events: Vec<ExtensionEventPayload>,
    pub cancelled: Arc<AtomicBool>,
}

struct RuntimeState {
    extension_id: String,
    host: Arc<dyn ExtensionHost>,
    invocation: Mutex<Option<ExtensionInvocation>>,
}

impl RuntimeState {
    fn set_invocation(&self, invocation: ExtensionInvocation) {
        if let Ok(mut current) = self.invocation.lock() {
            *current = Some(invocation);
        }
    }

    fn clear_invocation(&self) {
        if let Ok(mut current) = self.invocation.lock() {
            *current = None;
        }
    }

    fn invocation(&self) -> Result<ExtensionInvocation, mlua::Error> {
        self.invocation
            .lock()
            .map_err(|_| {
                mlua::Error::runtime("extension invocation state is poisoned")
            })?
            .clone()
            .ok_or_else(|| {
                mlua::Error::runtime(
                    "host API is unavailable outside an extension run",
                )
            })
    }

    fn request(&self, request: HostRequest) -> mlua::Result<HostResponse> {
        let invocation = self.invocation()?;
        if invocation.cancelled.load(Ordering::Acquire) {
            return Err(mlua::Error::runtime("extension run cancelled"));
        }
        let response = self
            .host
            .request(ExtensionHostRequest {
                extension_id: self.extension_id.clone(),
                run_id: invocation.run_id,
                cancelled: invocation.cancelled.clone(),
                request,
            })
            .map_err(mlua::Error::external)?;
        if invocation.cancelled.load(Ordering::Acquire) {
            return Err(mlua::Error::runtime("extension run cancelled"));
        }
        Ok(response)
    }

    fn cancellation_hook(&self) -> mlua::Result<VmState> {
        let invocation = self.invocation()?;
        if invocation.cancelled.load(Ordering::Acquire) {
            Err(mlua::Error::runtime("extension run cancelled"))
        } else {
            Ok(VmState::Continue)
        }
    }
}

/// A long-lived Lua VM for one extension. The VM is only accessed from the
/// worker thread that created it; host calls cross the explicit bridge above.
pub struct ExtensionRuntime {
    lua: Lua,
    handlers: Table,
    state: Arc<RuntimeState>,
}

impl ExtensionRuntime {
    /// Construct a runtime from a validated package source.
    pub fn load(
        extension_id: String,
        source: &str,
        package_root: Option<PathBuf>,
        host: Arc<dyn ExtensionHost>,
    ) -> Result<Self, ExtensionRuntimeError> {
        // Full standard libraries are intentional: enabling an extension is
        // an explicit trust decision and is equivalent to running local code.
        let lua = unsafe { Lua::unsafe_new() };
        lua.set_memory_limit(LUA_MEMORY_LIMIT_BYTES)
            .map_err(|error| ExtensionRuntimeError::Lua(error.to_string()))?;
        let state = Arc::new(RuntimeState {
            extension_id,
            host,
            invocation: Mutex::new(None),
        });
        let hook_state = state.clone();
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            move |_, _| hook_state.cancellation_hook(),
        )
        .map_err(|error| ExtensionRuntimeError::Lua(error.to_string()))?;
        install_package_paths(&lua, package_root)
            .map_err(|error| ExtensionRuntimeError::Lua(error.to_string()))?;
        install_api(&lua, state.clone())
            .map_err(|error| ExtensionRuntimeError::Lua(error.to_string()))?;
        let handlers = lua
            .load(source)
            .eval::<Table>()
            .map_err(|error| ExtensionRuntimeError::Lua(error.to_string()))?;
        Ok(Self {
            lua,
            handlers,
            state,
        })
    }

    /// Invoke one named handler with a fresh run context.
    pub fn run(
        &self,
        invocation: ExtensionInvocation,
        handler: &str,
    ) -> Result<JsonValue, ExtensionRuntimeError> {
        if !self.has_handler(handler) {
            return Err(ExtensionRuntimeError::MissingHandler(
                handler.to_string(),
            ));
        }
        self.state.set_invocation(invocation);
        let result = (|| {
            let function =
                self.handlers.get::<Function>(handler).map_err(|error| {
                    ExtensionRuntimeError::Lua(error.to_string())
                })?;
            let context = create_context(&self.lua, self.state.clone())
                .map_err(|error| {
                    ExtensionRuntimeError::Lua(error.to_string())
                })?;
            let value = function.call::<Value>(context).map_err(|error| {
                ExtensionRuntimeError::Lua(error.to_string())
            })?;
            value_to_json(&value)
                .map_err(|error| ExtensionRuntimeError::Lua(error.to_string()))
        })();
        self.state.clear_invocation();
        result
    }

    pub fn has_handler(&self, handler: &str) -> bool {
        self.handlers.get::<Function>(handler).is_ok()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeError {
    Lua(String),
    MissingHandler(String),
}

impl fmt::Display for ExtensionRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lua(error) => formatter.write_str(error),
            Self::MissingHandler(handler) => {
                write!(formatter, "extension handler is missing: {handler}")
            }
        }
    }
}

impl std::error::Error for ExtensionRuntimeError {}

fn install_package_paths(
    lua: &Lua,
    package_root: Option<PathBuf>,
) -> mlua::Result<()> {
    let Some(root) = package_root else {
        return Ok(());
    };
    let package: Table = lua.globals().get("package")?;
    let path = root.join("?.lua").to_string_lossy().replace('\\', "/")
        + ";"
        + &root.join("?/init.lua").to_string_lossy().replace('\\', "/");
    let existing: String = package.get("path")?;
    package.set("path", format!("{path};{existing}"))?;
    let existing_c: String = package.get("cpath")?;
    let cpath = root.join("?.dll").to_string_lossy().replace('\\', "/")
        + ";"
        + &root.join("?.so").to_string_lossy().replace('\\', "/")
        + ";"
        + &root.join("?.dylib").to_string_lossy().replace('\\', "/");
    package.set("cpath", format!("{cpath};{existing_c}"))?;
    Ok(())
}

fn install_api(lua: &Lua, state: Arc<RuntimeState>) -> mlua::Result<()> {
    let augur = lua.create_table()?;
    augur.set("api_version", lua.create_function(|_, ()| Ok(1u32))?)?;

    let time = lua.create_table()?;
    let time_state = state.clone();
    time.set(
        "now",
        lua.create_function(move |lua, ()| {
            let value = time_state.request(HostRequest::TimeNow)?;
            response_to_lua(lua, value)
        })?,
    )?;
    augur.set("time", time)?;

    let system = lua.create_table()?;
    let system_state = state.clone();
    system.set(
        "info",
        lua.create_function(move |lua, ()| {
            let value = system_state.request(HostRequest::SystemInfo)?;
            response_to_lua(lua, value)
        })?,
    )?;
    augur.set("system", system)?;

    let log_state = state.clone();
    augur.set("log", lua.create_function(move |_, (level, message, fields): (String, String, Option<Table>)| {
        let fields = fields
            .map(|table| table_to_json(&table))
            .transpose()?
            .unwrap_or(JsonValue::Object(serde_json::Map::new()));
        let _ = log_state.request(HostRequest::Log { level, message, fields })?;
        Ok(())
    })?)?;

    let notify_state = state.clone();
    augur.set(
        "notify",
        lua.create_function(
            move |_, (level, title, body): (String, String, String)| {
                let _ = notify_state.request(HostRequest::Notify {
                    level,
                    title,
                    body,
                })?;
                Ok(())
            },
        )?,
    )?;

    let storage = lua.create_table()?;
    let get_state = state.clone();
    storage.set(
        "get",
        lua.create_function(move |lua, key: Option<String>| {
            response_to_lua(
                lua,
                get_state.request(HostRequest::StorageGet(key))?,
            )
        })?,
    )?;
    let set_state = state.clone();
    storage.set(
        "set",
        lua.create_function(move |_, (key, value): (String, Value)| {
            if key.trim().is_empty() {
                return Err(mlua::Error::runtime(
                    "storage key must not be empty",
                ));
            }
            let value = value_to_json(&value)?;
            let _ =
                set_state.request(HostRequest::StorageSet { key, value })?;
            Ok(())
        })?,
    )?;
    let delete_state = state.clone();
    storage.set(
        "delete",
        lua.create_function(move |_, key: Option<String>| {
            let _ = delete_state.request(HostRequest::StorageDelete(key))?;
            Ok(())
        })?,
    )?;
    augur.set("storage", storage)?;

    let workspace = lua.create_table()?;
    let workspace_state = state.clone();
    workspace.set(
        "repository_tabs",
        lua.create_function(move |lua, ()| {
            let response = workspace_state
                .request(HostRequest::WorkspaceRepositoryTabs)?;
            repository_response_to_lua(lua, response, workspace_state.clone())
        })?,
    )?;
    augur.set("workspace", workspace)?;

    let agent = lua.create_table()?;
    let agent_state = state.clone();
    agent.set("prompt", lua.create_function(move |lua, (repository, options): (Option<mlua::AnyUserData>, Table)| {
        let repository = repository
            .map(|value| value.borrow::<LuaRepository>().map(|repo| repo.snapshot.tab_id))
            .transpose()?;
        let prompt: String = options.get("prompt")?;
        if prompt.trim().is_empty() {
            return Err(mlua::Error::runtime("agent prompt must not be empty"));
        }
        let timeout_seconds = options.get::<Option<u64>>("timeout_seconds")?.unwrap_or(1800);
        let response = agent_state.request(HostRequest::AgentPrompt(AgentRequest {
            repository,
            options: AgentPromptOptions { prompt, timeout_seconds },
        }))?;
        response_to_lua(lua, response)
    })?)?;
    augur.set("agent", agent)?;

    let package: Table = lua.globals().get("package")?;
    let preload: Table = package.get("preload")?;
    let module = augur.clone();
    preload.set(
        "augur",
        lua.create_function(move |_, _name: String| Ok(module.clone()))?,
    )?;
    Ok(())
}

fn create_context(lua: &Lua, state: Arc<RuntimeState>) -> mlua::Result<Table> {
    let invocation = state.invocation()?;
    let context = lua.create_table()?;
    context.set("run_id", invocation.run_id)?;
    context.set(
        "trigger",
        match &invocation.trigger {
            ExtensionTrigger::Manual => "manual".to_string(),
            ExtensionTrigger::Schedule { .. } => "schedule".to_string(),
            ExtensionTrigger::Repository { .. } => "repository".to_string(),
        },
    )?;
    if let ExtensionTrigger::Schedule {
        trigger_id,
        event_type,
    }
    | ExtensionTrigger::Repository {
        trigger_id,
        event_type,
    } = &invocation.trigger
    {
        context.set("trigger_id", trigger_id.clone())?;
        context.set("event_type", event_type.clone())?;
    }
    context.set(
        "scheduled_at",
        invocation.scheduled_at.map(|value| value.to_rfc3339()),
    )?;
    let occurred_at = invocation
        .events
        .first()
        .map(|event| event.occurred_at.to_rfc3339())
        .or_else(|| invocation.scheduled_at.map(|value| value.to_rfc3339()));
    context.set("occurred_at", occurred_at)?;
    context.set("started_at", invocation.started_at.to_rfc3339())?;
    let setting_values = lua.create_table()?;
    for (key, value) in &invocation.settings {
        setting_values.set(key.as_str(), setting_to_lua(lua, value)?)?;
    }
    let settings = lua.create_table()?;
    let read_only = lua.create_table()?;
    let pairs_values = setting_values.clone();
    read_only.set("__index", setting_values)?;
    read_only.set(
        "__pairs",
        lua.create_function(move |lua, _table: Table| {
            let next: Function = lua.globals().get("next")?;
            Ok((next, pairs_values.clone(), Value::Nil))
        })?,
    )?;
    read_only.set(
        "__newindex",
        lua.create_function(
            |_,
             (_table, _key, _value): (Table, Value, Value)|
             -> mlua::Result<()> {
                Err(mlua::Error::runtime("extension settings are read-only"))
            },
        )?,
    )?;
    settings.set_metatable(Some(read_only))?;
    context.set("settings", settings)?;
    let repositories = lua.create_table()?;
    for (index, snapshot) in invocation.repositories.iter().enumerate() {
        repositories.raw_set(
            index + 1,
            lua.create_userdata(LuaRepository {
                snapshot: snapshot.clone(),
                state: state.clone(),
            })?,
        )?;
    }
    context.set("repositories", repositories)?;
    let events = lua.create_table()?;
    for (index, event) in invocation.events.iter().enumerate() {
        events.raw_set(
            index + 1,
            event_payload_to_lua(lua, state.clone(), event)?,
        )?;
    }
    context.set("events", events)?;
    if let Some(event) = invocation.events.first() {
        context
            .set("event", event_payload_to_lua(lua, state.clone(), event)?)?;
    } else {
        context.set("event", Value::Nil)?;
    }
    let cancel_state = state.clone();
    context.set(
        "cancelled",
        lua.create_function(move |_, ()| {
            Ok(cancel_state.invocation()?.cancelled.load(Ordering::Acquire))
        })?,
    )?;
    Ok(context)
}

fn event_payload_to_lua(
    lua: &Lua,
    state: Arc<RuntimeState>,
    event: &ExtensionEventPayload,
) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("trigger_id", event.trigger_id.clone())?;
    table.set("event_type", event.event_type.clone())?;
    table.set("occurred_at", event.occurred_at.to_rfc3339())?;
    if let Some(repository) = &event.repository {
        table.set(
            "repository_snapshot",
            json_to_lua(
                lua,
                serde_json::to_value(repository)
                    .map_err(mlua::Error::external)?,
            )?,
        )?;
        if event.event_type != "workspace.repository_closed" {
            table.set(
                "repository",
                lua.create_userdata(LuaRepository {
                    snapshot: repository.clone(),
                    state: state.clone(),
                })?,
            )?;
        }
    }
    table.set(
        "previous",
        event
            .previous
            .as_ref()
            .map(|value| serde_json::to_value(value))
            .transpose()
            .map_err(mlua::Error::external)
            .and_then(|value| match value {
                Some(value) => json_to_lua(lua, value),
                None => Ok(Value::Nil),
            })?,
    )?;
    table.set(
        "current",
        event
            .current
            .as_ref()
            .map(|value| serde_json::to_value(value))
            .transpose()
            .map_err(mlua::Error::external)
            .and_then(|value| match value {
                Some(value) => json_to_lua(lua, value),
                None => Ok(Value::Nil),
            })?,
    )?;
    Ok(table)
}

/// Userdata handle for one repository. The captured identity is stable for a
/// run, while each operation is revalidated by the Workspace before mutation.
#[derive(Clone)]
pub struct LuaRepository {
    pub snapshot: RepositorySnapshot,
    state: Arc<RuntimeState>,
}

impl UserData for LuaRepository {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("snapshot", |lua, repository, ()| {
            json_to_lua(
                lua,
                serde_json::to_value(&repository.snapshot)
                    .map_err(mlua::Error::external)?,
            )
        });
        methods.add_method("path", |_, repository, ()| {
            Ok(repository.snapshot.path.clone())
        });
        methods.add_method("display_name", |_, repository, ()| {
            Ok(repository.snapshot.display_name.clone())
        });
        methods.add_method("status", |lua, repository, ()| {
            response_to_lua(
                lua,
                repository.state.request(HostRequest::Repository {
                    tab_id: repository.snapshot.tab_id,
                    operation: RepositoryOperation::Status,
                    expected_branch: repository.snapshot.branch.clone(),
                    expected_head: repository.snapshot.head.clone(),
                })?,
            )
        });
        methods.add_method(
            "git",
            |lua, repository, (args, options): (Table, Option<Table>)| {
                let args = table_to_strings(&args)?;
                if args.is_empty() {
                    return Err(mlua::Error::runtime(
                        "repo:git requires at least one argument",
                    ));
                }
                let label = options
                    .as_ref()
                    .and_then(|table| {
                        table.get::<Option<String>>("label").ok().flatten()
                    })
                    .unwrap_or_else(|| args.join(" "));
                let timeout_seconds = options
                    .as_ref()
                    .and_then(|table| {
                        table
                            .get::<Option<u64>>("timeout_seconds")
                            .ok()
                            .flatten()
                    })
                    .unwrap_or(1800);
                response_to_lua(
                    lua,
                    repository.state.request(HostRequest::Repository {
                        tab_id: repository.snapshot.tab_id,
                        operation: RepositoryOperation::Git {
                            args,
                            label,
                            timeout_seconds,
                        },
                        expected_branch: repository.snapshot.branch.clone(),
                        expected_head: repository.snapshot.head.clone(),
                    })?,
                )
            },
        );
        methods.add_method("pull_rebase", |lua, repository, ()| {
            response_to_lua(
                lua,
                repository.state.request(HostRequest::Repository {
                    tab_id: repository.snapshot.tab_id,
                    operation: RepositoryOperation::PullRebase,
                    expected_branch: repository.snapshot.branch.clone(),
                    expected_head: repository.snapshot.head.clone(),
                })?,
            )
        });
        methods.add_method(
            "push",
            |lua, repository, options: Option<Table>| {
                let remote = options.as_ref().and_then(|table| {
                    table.get::<Option<String>>("remote").ok().flatten()
                });
                let branch = options.as_ref().and_then(|table| {
                    table.get::<Option<String>>("branch").ok().flatten()
                });
                response_to_lua(
                    lua,
                    repository.state.request(HostRequest::Repository {
                        tab_id: repository.snapshot.tab_id,
                        operation: RepositoryOperation::Push { remote, branch },
                        expected_branch: repository.snapshot.branch.clone(),
                        expected_head: repository.snapshot.head.clone(),
                    })?,
                )
            },
        );
        methods.add_method(
            "agent_commit",
            |lua, repository, options: Option<Table>| {
                let hint = options.as_ref().and_then(|table| {
                    table.get::<Option<String>>("hint").ok().flatten()
                });
                response_to_lua(
                    lua,
                    repository.state.request(HostRequest::Repository {
                        tab_id: repository.snapshot.tab_id,
                        operation: RepositoryOperation::AgentCommit { hint },
                        expected_branch: repository.snapshot.branch.clone(),
                        expected_head: repository.snapshot.head.clone(),
                    })?,
                )
            },
        );
        methods.add_method("agent_merge", |lua, repository, source: String| {
            response_to_lua(
                lua,
                repository.state.request(HostRequest::Repository {
                    tab_id: repository.snapshot.tab_id,
                    operation: RepositoryOperation::AgentMerge { source },
                    expected_branch: repository.snapshot.branch.clone(),
                    expected_head: repository.snapshot.head.clone(),
                })?,
            )
        });
        methods.add_method("merge", |lua, repository, source: String| {
            response_to_lua(
                lua,
                repository.state.request(HostRequest::Repository {
                    tab_id: repository.snapshot.tab_id,
                    operation: RepositoryOperation::AgentMerge { source },
                    expected_branch: repository.snapshot.branch.clone(),
                    expected_head: repository.snapshot.head.clone(),
                })?,
            )
        });
        methods.add_method(
            "agent_rebase",
            |lua, repository, source: String| {
                response_to_lua(
                    lua,
                    repository.state.request(HostRequest::Repository {
                        tab_id: repository.snapshot.tab_id,
                        operation: RepositoryOperation::AgentRebase { source },
                        expected_branch: repository.snapshot.branch.clone(),
                        expected_head: repository.snapshot.head.clone(),
                    })?,
                )
            },
        );
        methods.add_method("rebase", |lua, repository, source: String| {
            response_to_lua(
                lua,
                repository.state.request(HostRequest::Repository {
                    tab_id: repository.snapshot.tab_id,
                    operation: RepositoryOperation::AgentRebase { source },
                    expected_branch: repository.snapshot.branch.clone(),
                    expected_head: repository.snapshot.head.clone(),
                })?,
            )
        });
        methods.add_method("resolve_merge", |lua, repository, ()| {
            response_to_lua(
                lua,
                repository.state.request(HostRequest::Repository {
                    tab_id: repository.snapshot.tab_id,
                    operation: RepositoryOperation::ResolveMerge,
                    expected_branch: repository.snapshot.branch.clone(),
                    expected_head: repository.snapshot.head.clone(),
                })?,
            )
        });
        methods.add_method("resolve_rebase", |lua, repository, ()| {
            response_to_lua(
                lua,
                repository.state.request(HostRequest::Repository {
                    tab_id: repository.snapshot.tab_id,
                    operation: RepositoryOperation::ResolveRebase,
                    expected_branch: repository.snapshot.branch.clone(),
                    expected_head: repository.snapshot.head.clone(),
                })?,
            )
        });
    }
}

fn repository_response_to_lua(
    lua: &Lua,
    response: HostResponse,
    state: Arc<RuntimeState>,
) -> mlua::Result<Value> {
    match response {
        HostResponse::Json(value) => {
            if let Ok(snapshots) =
                serde_json::from_value::<Vec<RepositorySnapshot>>(value.clone())
            {
                let table = lua.create_table()?;
                for (index, snapshot) in snapshots.into_iter().enumerate() {
                    table.raw_set(
                        index + 1,
                        lua.create_userdata(LuaRepository {
                            snapshot,
                            state: state.clone(),
                        })?,
                    )?;
                }
                Ok(Value::Table(table))
            } else {
                json_to_lua(lua, value)
            }
        }
        other => response_to_lua(lua, other),
    }
}

fn response_to_lua(lua: &Lua, response: HostResponse) -> mlua::Result<Value> {
    match response {
        HostResponse::Json(value) => json_to_lua(lua, value),
        HostResponse::Failure { code, summary } => json_to_lua(
            lua,
            serde_json::json!({ "ok": false, "code": code, "summary": summary }),
        ),
    }
}

fn json_to_lua(lua: &Lua, value: JsonValue) -> mlua::Result<Value> {
    lua.to_value(&value)
}

fn setting_to_lua(lua: &Lua, value: &SettingValue) -> mlua::Result<Value> {
    match value {
        SettingValue::String(value)
        | SettingValue::Time(value)
        | SettingValue::Select(value) => {
            Ok(Value::String(lua.create_string(value)?))
        }
        SettingValue::Integer(value) => Ok(Value::Integer(*value)),
        SettingValue::Boolean(value) => Ok(Value::Boolean(*value)),
    }
}

fn table_to_strings(table: &Table) -> mlua::Result<Vec<String>> {
    table.sequence_values::<String>().collect()
}

fn table_to_json(table: &Table) -> mlua::Result<JsonValue> {
    let value = Value::Table(table.clone());
    value_to_json(&value)
}

fn value_to_json(value: &Value) -> mlua::Result<JsonValue> {
    match value {
        Value::Nil => Ok(JsonValue::Null),
        Value::Boolean(value) => Ok(JsonValue::Bool(*value)),
        Value::Integer(value) => Ok(JsonValue::Number((*value).into())),
        Value::Number(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .ok_or_else(|| {
                mlua::Error::runtime("cannot store a non-finite number")
            }),
        Value::String(value) => {
            Ok(JsonValue::String(value.to_str()?.to_string()))
        }
        Value::Table(table) => {
            let mut object = serde_json::Map::new();
            let mut array = Vec::new();
            let mut is_array = true;
            for pair in table.clone().pairs::<Value, Value>() {
                let (key, value) = pair?;
                let json_value = value_to_json(&value)?;
                match key {
                    Value::String(key) => {
                        is_array = false;
                        object.insert(key.to_str()?.to_string(), json_value);
                    }
                    Value::Integer(index) if index > 0 => {
                        array.push((index as usize, json_value));
                    }
                    _ => is_array = false,
                }
            }
            if is_array && !array.is_empty() {
                array.sort_by_key(|(index, _)| *index);
                if array
                    .iter()
                    .enumerate()
                    .all(|(position, (index, _))| *index == position + 1)
                {
                    return Ok(JsonValue::Array(
                        array.into_iter().map(|(_, value)| value).collect(),
                    ));
                }
            }
            Ok(JsonValue::Object(object))
        }
        _ => Err(mlua::Error::runtime(
            "value cannot be serialized for the host API",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeHost {
        requests: Mutex<Vec<HostRequest>>,
    }

    impl ExtensionHost for FakeHost {
        fn request(
            &self,
            request: ExtensionHostRequest,
        ) -> Result<HostResponse, String> {
            let response = match &request.request {
                HostRequest::StorageSet { .. }
                | HostRequest::Log { .. }
                | HostRequest::Notify { .. } => {
                    HostResponse::Json(serde_json::json!({ "ok": true }))
                }
                HostRequest::TimeNow => {
                    HostResponse::Json(serde_json::json!({ "unix_ms": 1 }))
                }
                _ => HostResponse::Json(JsonValue::Null),
            };
            self.requests
                .lock()
                .map_err(|_| "poisoned".to_string())?
                .push(request.request);
            Ok(response)
        }
    }

    fn invocation() -> ExtensionInvocation {
        let mut settings = BTreeMap::new();
        settings.insert("answer".into(), SettingValue::Integer(7));
        ExtensionInvocation {
            extension_id: "test-extension".into(),
            run_id: 1,
            trigger: ExtensionTrigger::Manual,
            scheduled_at: None,
            started_at: Local::now(),
            settings,
            repositories: Vec::new(),
            events: Vec::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn runtime_loads_full_lua_stdlib_and_host_module() {
        let host = Arc::new(FakeHost::default());
        let runtime = ExtensionRuntime::load(
            "test-extension".into(),
            r#"
                local augur = require("augur")
                return {
                    on_run = function(ctx)
                        local _ = os.date("!*t")
                        local _ = io.type(io.tmpfile())
                        assert(ctx.settings.answer == 7)
                        local count = 0
                        for key, value in pairs(ctx.settings) do
                            if key == "answer" and value == 7 then count = count + 1 end
                        end
                        assert(count == 1)
                        local writable = pcall(function() ctx.settings.answer = 8 end)
                        assert(not writable)
                        augur.log("info", "hello", {run_id = ctx.run_id})
                        return {ok = true, value = 7}
                    end
                }
            "#,
            None,
            host.clone(),
        )
        .expect("runtime should load");
        let result = runtime
            .run(invocation(), "on_run")
            .expect("run should complete");
        assert_eq!(result.get("value").and_then(JsonValue::as_i64), Some(7));
        assert!(
            host.requests
                .lock()
                .expect("requests")
                .iter()
                .any(|request| matches!(request, HostRequest::Log { .. }))
        );
    }
}

fn _utc_now() -> DateTime<Utc> {
    Utc::now()
}
