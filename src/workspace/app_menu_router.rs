//! Application-level routing for File menu actions.
//!
//! Native menu validation follows the currently focused GPUI dispatch tree.
//! File actions must remain available while a settings page, dialog, or
//! secondary window owns focus, so they are registered globally and then
//! executed against the main workspace window.

use gpui::{App, AppContext, Context, Window};

use crate::core::config::{self, AppConfig, UiState};
use crate::theme;

use super::app_menu::{
    InstallCli, NewTab, OpenRepository, OpenWslRepository, RemoveCli,
};
use super::{
    ActiveWorkspace, Workspace, installed_font_families, normalize_typography,
    open_main_window,
};

/// Install global handlers for File menu actions.
pub(super) fn install(cx: &mut App) {
    cx.on_action(|_: &OpenRepository, cx| {
        dispatch(cx, RoutedAction::OpenRepository);
    });
    cx.on_action(|_: &OpenWslRepository, cx| {
        dispatch(cx, RoutedAction::OpenWslRepository);
    });
    cx.on_action(|_: &NewTab, cx| {
        dispatch(cx, RoutedAction::NewTab);
    });
    cx.on_action(|_: &InstallCli, cx| {
        dispatch(cx, RoutedAction::InstallCli);
    });
    cx.on_action(|_: &RemoveCli, cx| {
        dispatch(cx, RoutedAction::RemoveCli);
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoutedAction {
    OpenRepository,
    OpenWslRepository,
    NewTab,
    InstallCli,
    RemoveCli,
}

impl RoutedAction {
    fn label(self) -> &'static str {
        match self {
            Self::OpenRepository => "open repository",
            Self::OpenWslRepository => "open WSL repository",
            Self::NewTab => "new tab",
            Self::InstallCli => "install CLI",
            Self::RemoveCli => "remove CLI",
        }
    }

    fn apply(
        self,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        match self {
            Self::OpenRepository => {
                workspace.handle_open_repository(&OpenRepository, window, cx)
            }
            Self::OpenWslRepository => workspace.handle_open_wsl_repository(
                &OpenWslRepository,
                window,
                cx,
            ),
            Self::NewTab => workspace.handle_new_tab(&NewTab, window, cx),
            Self::InstallCli => {
                workspace.handle_install_cli(&InstallCli, window, cx)
            }
            Self::RemoveCli => {
                workspace.handle_remove_cli(&RemoveCli, window, cx)
            }
        }
    }
}

fn dispatch(cx: &mut App, action: RoutedAction) {
    log::info!("[app_menu] received {} action", action.label());
    // App-level action listeners run while the active window is dispatching
    // the action. Defer the window update so actions originating in the main
    // window do not try to borrow that same window recursively.
    cx.defer(move |cx| {
        if dispatch_to_live_window(cx, action) {
            return;
        }

        log::info!(
            "[app_menu] main window unavailable; reopening before {} action",
            action.label()
        );
        reopen_and_dispatch(cx, action);
    });
}

/// Route an action through the main window so operations that need a Window
/// (file prompts, dialogs, and background tasks) use the correct root.
fn dispatch_to_live_window(cx: &mut App, action: RoutedAction) -> bool {
    let Some(window) = cx
        .try_global::<ActiveWorkspace>()
        .and_then(|active| active.window)
    else {
        log::debug!(
            "[app_menu] no main window handle for {} action",
            action.label()
        );
        return false;
    };

    let result = window.update(cx, |_root, window, cx| {
        let Some(workspace) = cx
            .try_global::<ActiveWorkspace>()
            .and_then(|active| active.workspace.upgrade())
        else {
            log::debug!(
                "[app_menu] main workspace unavailable for {} action",
                action.label()
            );
            return false;
        };

        window.activate_window();
        workspace.update(cx, |workspace, cx| {
            action.apply(workspace, window, cx);
        });
        true
    });

    match result {
        Ok(true) => {
            log::info!(
                "[app_menu] routed {} action to the main window",
                action.label()
            );
            true
        }
        Ok(false) => false,
        Err(error) => {
            log::warn!(
                "[app_menu] failed to route {} action to the main window: {error}",
                action.label()
            );
            false
        }
    }
}

/// Recreate the main window after the last window was closed, then retry the
/// action once the new Workspace and Root have been installed.
fn reopen_and_dispatch(cx: &mut App, action: RoutedAction) {
    cx.spawn(async move |cx| {
        let (mut config, ui_state): (AppConfig, UiState) = cx
            .background_spawn(async {
                (config::load(), config::load_ui_state())
            })
            .await;

        let reopen_result = cx.update(|app| {
            let fonts = installed_font_families(app);
            normalize_typography(&mut config, &fonts);
            theme::apply(config.theme, &config.typography, app);
            open_main_window(app, config, ui_state, fonts)
        });

        if let Err(error) = reopen_result {
            log::error!(
                "[app_menu] failed to reopen the main window for {} action: {error}",
                action.label()
            );
            return;
        }

        let routed = cx.update(|app| dispatch_to_live_window(app, action));
        if !routed {
            log::error!(
                "[app_menu] reopened the main window but could not route {} action",
                action.label()
            );
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use gpui::{EmptyView, TestAppContext};

    use super::*;

    #[test]
    fn routed_action_labels_are_stable_for_diagnostics() {
        assert_eq!(RoutedAction::OpenRepository.label(), "open repository");
        assert_eq!(
            RoutedAction::OpenWslRepository.label(),
            "open WSL repository"
        );
        assert_eq!(RoutedAction::NewTab.label(), "new tab");
        assert_eq!(RoutedAction::InstallCli.label(), "install CLI");
        assert_eq!(RoutedAction::RemoveCli.label(), "remove CLI");
    }

    #[gpui::test]
    fn file_actions_are_available_without_a_focused_workspace(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            install(cx);
            assert!(cx.is_action_available(&OpenRepository));
            assert!(cx.is_action_available(&OpenWslRepository));
            assert!(cx.is_action_available(&NewTab));
            assert!(cx.is_action_available(&InstallCli));
            assert!(cx.is_action_available(&RemoveCli));
        });
    }

    #[gpui::test]
    fn file_actions_remain_available_with_an_active_window(
        cx: &mut TestAppContext,
    ) {
        let window = cx.add_window(|_, _| EmptyView);
        cx.update(|cx| {
            install(cx);
            window
                .update(cx, |_, window, _| window.activate_window())
                .expect("test window should remain available");
            assert!(cx.is_action_available(&OpenRepository));
            assert!(cx.is_action_available(&OpenWslRepository));
            assert!(cx.is_action_available(&NewTab));
            assert!(cx.is_action_available(&InstallCli));
            assert!(cx.is_action_available(&RemoveCli));
        });
    }
}
