//! Lua extension runtime and the host bridge used by the Workspace.

mod api;

pub use api::{
    AgentPromptOptions, AgentRequest, ExtensionHost, ExtensionInvocation,
    ExtensionRuntime, ExtensionRuntimeError, ExtensionTrigger, HostRequest,
    HostResponse, LuaRepository, RepositoryOperation, RepositorySnapshot,
};
