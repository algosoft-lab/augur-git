//! CLI installation status and platform-specific command registration.

use serde::Serialize;
use std::path::PathBuf;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::launch_target;
#[cfg(unix)]
pub use unix::Context;
#[cfg(windows)]
pub use windows::Context;
#[cfg(windows)]
pub use windows::{
    acquire_startup_gate, install_from_launcher, uninstall_from_launcher,
    uninstall_package_from_launcher,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: &'static str,
    pub path: PathBuf,
    pub application_path: Option<PathBuf>,
    pub shell: ShellDetection,
    pub can_install: bool,
    pub can_remove: bool,
    pub package_managed: bool,
    pub path_command: Option<String>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_path: Option<WindowsPathStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsPathStatus {
    pub registered: bool,
    pub process_state: &'static str,
    pub resolved_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellDetection {
    pub state: &'static str,
    pub shell: Option<String>,
    pub path: Option<PathBuf>,
    pub reason: Option<&'static str>,
}

impl Default for ShellDetection {
    fn default() -> Self {
        Self {
            state: "unknown",
            shell: None,
            path: None,
            reason: Some("shell-not-probed"),
        }
    }
}
