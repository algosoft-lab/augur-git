//! Lua extension runtime and the host bridge used by the Workspace.

use std::collections::HashSet;

mod agent_session;
mod api;
mod builtin;
mod file_log;
mod history;
mod host;
mod manager;
mod storage;

pub(crate) use agent_session::{
    AgentSessionOperation, AgentSessionOutcome, AgentSessionRequest,
};
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
    let bundled_ids = definitions
        .iter()
        .map(|definition| definition.package.manifest.id.clone())
        .collect::<HashSet<_>>();
    match crate::core::extension::discover_local_packages() {
        Ok(packages) => {
            for package in packages {
                match package {
                    Ok(package) => {
                        if bundled_ids.contains(&package.manifest.id) {
                            log::warn!(
                                "[extensions] skipped local package with reserved bundled id: {}",
                                package.manifest.id
                            );
                            continue;
                        }
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
