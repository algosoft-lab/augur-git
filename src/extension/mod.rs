//! Lua extension runtime and the host bridge used by the Workspace.

mod agent_runner;
mod api;
mod builtin;
mod history;
mod host;
mod manager;
mod storage;

#[allow(unused_imports)]
pub use api::{
    AgentPromptOptions, AgentRequest, ExtensionEventPayload, ExtensionHost,
    ExtensionInvocation, ExtensionRunAdmission, ExtensionRuntime,
    ExtensionRuntimeError, ExtensionTrigger, HostRequest, HostResponse,
    LuaRepository, RepositoryOperation, RepositorySnapshot,
};
pub use builtin::bundled_definitions;
#[allow(unused_imports)]
pub use history::{append_run_history, load_run_history};
pub use host::{HostBridge, HostEvent};
pub use manager::{
    ExtensionDefinition, ExtensionEvent, ExtensionManager, ExtensionRunRequest,
};

/// Load bundled packages and valid local packages. A broken local package is
/// reported and skipped so it cannot prevent the application from starting.
pub fn discover_definitions() -> Vec<ExtensionDefinition> {
    let mut definitions = match bundled_definitions() {
        Ok(definitions) => definitions,
        Err(error) => {
            log::error!(
                "[extensions] bundled extension failed validation: {error}"
            );
            Vec::new()
        }
    };
    match crate::core::extension::discover_local_packages() {
        Ok(packages) => {
            for package in packages {
                match package {
                    Ok(package) => {
                        let Some(root) = package.root.clone() else {
                            continue;
                        };
                        let entrypoint =
                            root.join(&package.manifest.entrypoint);
                        match std::fs::read_to_string(&entrypoint) {
                            Ok(source) => definitions
                                .push(ExtensionDefinition { package, source }),
                            Err(error) => log::warn!(
                                "[extensions] failed to read local extension source: {error}"
                            ),
                        }
                    }
                    Err(error) => {
                        log::warn!(
                            "[extensions] skipped invalid local package: {error}"
                        );
                    }
                }
            }
        }
        Err(error) => log::warn!(
            "[extensions] failed to discover local packages: {error}"
        ),
    }
    definitions
}
