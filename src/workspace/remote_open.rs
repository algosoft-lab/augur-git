//! Opening repository tabs from outside an already-running window.
//!
//! Two sources feed this module: repository paths given on the command line
//! of the primary launch, and open requests forwarded over the loopback
//! socket by a second launch (`core::ipc`). Both funnel through the same
//! window resolution: the active main window receives one repository tab per
//! path; when no window is alive (macOS keeps the process running after the
//! last window closes), a new main window is opened first, mirroring the
//! dock-reopen path in `workspace::run`.

use std::sync::mpsc::Receiver;
use std::time::Duration;

use gpui::{App, AppContext, AsyncApp};

use crate::core::config::{self, LocationConfig};
use crate::core::ipc::InstanceServer;

use super::ActiveWorkspace;
use super::{installed_font_families, normalize_typography, open_main_window};
use crate::theme;

/// Open requests handed to the application at startup.
pub struct PendingOpen {
    pub startup_paths: Vec<String>,
    pub instance_server: Option<InstanceServer>,
}

impl PendingOpen {
    pub fn is_empty(&self) -> bool {
        self.startup_paths.is_empty() && self.instance_server.is_none()
    }
}

/// Start serving pending open requests. Called once, right after the first
/// main window exists; startup paths are applied immediately, then the
/// forward listener keeps running for the lifetime of the process.
pub(super) fn attach(cx: &mut App, pending: PendingOpen) {
    let PendingOpen {
        startup_paths,
        instance_server,
    } = pending;
    cx.spawn(async move |cx| {
        if !startup_paths.is_empty() {
            open_paths(cx, &startup_paths).await;
        }
        if let Some(receiver) =
            instance_server.map(InstanceServer::into_receiver)
        {
            listen(cx, receiver).await;
        }
    })
    .detach();
}

/// Poll the forward channel and apply every request. The loop survives
/// window closes: each request resolves the active window afresh and falls
/// back to opening a new main window when none is alive.
async fn listen(cx: &mut AsyncApp, receiver: Receiver<Vec<String>>) {
    loop {
        let mut requests = Vec::new();
        while let Ok(paths) = receiver.try_recv() {
            requests.push(paths);
        }
        for paths in requests {
            open_paths(cx, &paths).await;
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
}

async fn open_paths(cx: &mut AsyncApp, paths: &[String]) {
    if try_open_in_active_window(cx, paths).is_ok() {
        log::info!(
            "[cli_open] opened {} repository tab(s) in the active window",
            paths.len()
        );
        return;
    }
    log::info!("[cli_open] no live main window; opening a new one");
    reopen_main_window(cx).await;
    if let Err(error) = try_open_in_active_window(cx, paths) {
        log::error!("[cli_open] failed to open requested paths: {error}");
    }
}

/// Open one repository tab per path in the current main window. Fails when
/// the window is gone, so the caller can fall back to re-opening one.
fn try_open_in_active_window(
    cx: &mut AsyncApp,
    paths: &[String],
) -> anyhow::Result<()> {
    let Some(window) = cx.update(|app| {
        app.try_global::<ActiveWorkspace>()
            .and_then(|active| active.window)
    }) else {
        anyhow::bail!("no main window has been created");
    };
    window.update(cx, |_root, window, cx| {
        if let Some(workspace) = cx
            .try_global::<ActiveWorkspace>()
            .and_then(|active| active.workspace.upgrade())
        {
            workspace.update(cx, |workspace, cx| {
                for path in paths {
                    workspace.open_repo_path(
                        path.clone(),
                        LocationConfig::Local,
                        false,
                        window,
                        cx,
                    );
                }
            });
        }
        window.activate_window();
    })?;
    Ok(())
}

/// Recreate a main window after the previous one closed. Mirrors the macOS
/// reopen handler in `workspace::run`: reload persisted configuration, apply
/// the theme, and let `Workspace::new` restore the saved tabs.
async fn reopen_main_window(cx: &mut AsyncApp) {
    let (mut config, ui_state) = cx
        .background_spawn(async { (config::load(), config::load_ui_state()) })
        .await;
    let result = cx.update(|app| {
        let fonts = installed_font_families(app);
        normalize_typography(&mut config, &fonts);
        theme::apply(config.theme, &config.typography, app);
        open_main_window(app, config, ui_state, fonts)
    });
    match result {
        Ok(_) => log::info!("[cli_open] reopened main window"),
        Err(error) => {
            log::error!("[cli_open] failed to reopen main window: {error}")
        }
    }
}
