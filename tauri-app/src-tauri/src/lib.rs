//! Augur Git Tauri application entry point.
//!
//! The process can start as the desktop application with no arguments, or as a
//! second launch forwarding repository paths that are already open in a
//! running instance. Both share one command-line parser and one
//! single-instance lock, so a second launch never produces a second window.

use tauri::{Emitter, Listener, Manager, RunEvent, WindowEvent};
use tauri_plugin_window_state::StateFlags;

pub mod auto_refresh;
pub mod commands;
pub mod events;
pub mod fonts;
pub mod git_args;
pub mod menu;
pub mod persistence;
pub mod repo;
pub mod state;
pub mod updates;

use augur_core::build_info;
use augur_core::cli::{self, CliInvocation};
use augur_core::i18n;

use crate::state::AppState;

/// Build and run the application with the paths a launch requested.
pub fn run(invocation: CliInvocation, forwarded: bool) {
    let builder = install_window_hooks(tauri::Builder::default())
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            handle_second_launch(app, &args, &cwd);
        }))
        .plugin(tauri_plugin_store::Builder::default().build())
        // The settings window is fixed-size, so a saved size can only go stale;
        // a window-state entry left by an older build would otherwise shrink it.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // Saved decorations from older builds must not override each window's configuration.
                .with_state_flags(StateFlags::all() & !StateFlags::DECORATIONS)
                .skip_initial_state("settings")
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(log_plugin())
        .invoke_handler(tauri::generate_handler![
            commands::agent_prompt::generate_agent_prompt,
            commands::cli::get_cli_status,
            commands::cli::install_cli,
            commands::cli::uninstall_cli,
            commands::repo::bootstrap,
            commands::repo::open_repository,
            commands::repo::close_repository,
            commands::repo::refresh_repository,
            commands::repo::set_log_scope,
            commands::repo::load_more_log_page,
            commands::repo::select_commit,
            commands::repo::request_commit_message,
            commands::repo::load_commit_file_diff,
            commands::repo::load_working_tree_diff,
            commands::repo::load_image_preview,
            commands::repo::working_tree_operation,
            commands::repo::run_action,
            commands::repo::checkout,
            commands::repo::start_compare,
            commands::repo::cancel_compare,
            commands::repo::export_patch,
            commands::repo::probe_merge,
            commands::repo::probe_rebase,
            commands::repo::preview_reset,
            commands::repo::read_commit_message,
            commands::repo::set_graph_history,
            commands::repo::graph_layout,
            commands::repo::column_visibility,
            commands::app::theme_options,
            commands::app::list_font_families,
            commands::app::list_wsl_distros,
            commands::app::probe_wsl_repository,
            commands::app::update_settings,
            commands::app::set_language,
            commands::app::set_theme,
            commands::app::set_view,
            commands::app::set_auto_refresh_target,
            commands::app::set_typography,
            commands::app::set_commit_action,
            commands::app::set_diff_layout,
            commands::app::set_layout,
            commands::app::set_window_mode,
            commands::app::save_window_bounds,
            commands::app::set_workspace_tabs,
            commands::app::set_shortcut,
            commands::app::validate_shortcut,
            commands::app::flush_state,
            commands::app::open_about_window,
            commands::app::open_settings_window,
            commands::app::open_compare_window,
            commands::app::close_compare_window,
            commands::app::focus_main_window,
            commands::app::request_open_paths,
            commands::app::take_pending_paths,
            commands::app::notify,
            commands::app::repository_summary,
            commands::app::current_config,
            updates::get_update_snapshot,
            updates::check_for_updates,
            updates::download_update,
            updates::install_update,
            updates::set_auto_check_updates,
            updates::dismiss_update_notice,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let pending = invocation.paths.clone();
            log::info!("[startup] setup entered with {} path(s)", pending.len());

            let report = persistence::LoadReport::default();
            app.manage(AppState::new(handle.clone(), report));
            let update_manager =
                updates::UpdateManager::new(handle.clone(), app.state::<AppState>().config());
            app.manage(update_manager.clone());
            update_manager.start();
            commands::app::restore_main_window(&handle, &app.state::<AppState>());
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(focused) = window.is_focused() {
                    app.state::<AppState>().set_main_window_focused(focused);
                }
            }
            log::info!(
                "[startup] state ready; store files: {:?}",
                app.state::<AppState>().persistence().store_paths()
            );
            install_menu_hooks(&handle);
            log::info!("[startup] hooks installed");
            if forwarded {
                // This process already lost the single-instance race; the
                // plugin forwards the paths and exits without ever building a
                // window. The handle is dropped on the next line.
                log::info!("[cli] paths forwarded to the running instance");
                return Ok(());
            }
            if !pending.is_empty() {
                // Replayed once the webview asks for them; a window that loads
                // late still receives the request.
                app.state::<AppState>().queue_paths(pending);
            }
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match builder {
        Ok(app) => app,
        Err(error) => {
            log::error!("[startup] failed to build the application: {error}");
            eprintln!("{}: failed to start: {error}", build_info::APP_NAME);
            std::process::exit(1);
        }
    };

    app.run(|app_handle, event| match event {
        RunEvent::ExitRequested { .. } | RunEvent::Exit => {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.shutdown();
            }
        }
        RunEvent::WindowEvent {
            event: WindowEvent::Destroyed,
            ..
        } => {}
        _ => {}
    });
}

/// A second launch parsed its own arguments; forward the paths to the running
/// instance and focus its window.
fn handle_second_launch(app: &tauri::AppHandle, args: &[String], cwd: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let app = app.clone();
    let args = args
        .iter()
        .skip(1)
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    let cwd = std::path::PathBuf::from(cwd);
    // Repository discovery invokes Git; it must not block the native event loop.
    tauri::async_runtime::spawn_blocking(move || match cli::parse_at(&args, &cwd) {
        cli::Parsed::Run(invocation) => AppState::deliver_open_paths(&app, invocation.paths),
        cli::Parsed::UsageError(error) => log::warn!("[cli] forwarded launch rejected: {error}"),
        _ => {}
    });
}

/// Rebuild the native menu when the language or shortcuts change, and forward
/// activations to the webview.
fn install_menu_hooks(app: &tauri::AppHandle) {
    let handle = app.clone();
    app.on_menu_event(move |app, event| {
        menu::dispatch(app, event.id().0.as_str());
    });
    let handle_for_rebuild = handle.clone();
    app.listen(events::APP_EVENT, move |event| {
        if event.payload().to_string().contains("settingsChanged")
            || event.payload().to_string().contains("workspaceChanged")
        {
            rebuild_menu(&handle_for_rebuild);
        }
    });
    rebuild_menu(&handle);
}

fn rebuild_menu(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let persistence = state.persistence();
    let config = persistence.config();
    let locale = i18n::resolve(&config.language);
    let shortcuts = persistence.resolved_shortcuts();
    let window_mode = persistence.workspace().window_mode;
    menu::install(app, locale, &shortcuts, &config.recent_repos, window_mode);
}

/// Write logs to stdout in a debug build and to the platform log directory
/// always, so a release build still leaves a diagnosable trail.
fn log_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use log::LevelFilter;
    use tauri_plugin_log::{Target, TargetKind};

    tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::LogDir { file_name: None }),
        ])
        .level(if cfg!(debug_assertions) {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        })
        .build()
}

/// Track main-window focus for the active-repository refresh monitor, and
/// forward dropped folders to the window that received them.
fn install_window_hooks(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(focused) if window.label() == "main" => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.set_main_window_focused(*focused);
                }
            }
            // A destroyed webview never runs its own cleanup, so a comparison
            // started by it would keep running and keep parsing diffs for
            // nobody. Cancelling here is the only place that still knows which
            // repository the window belonged to.
            WindowEvent::Destroyed => {
                if let Some(state) = window.app_handle().try_state::<AppState>()
                    && window.label() == "main"
                {
                    state.set_main_window_focused(false);
                }
                if let Some(repo_id) = window
                    .label()
                    .strip_prefix("compare-")
                    .and_then(|rest| rest.parse::<u64>().ok())
                    && let Some(state) = window.app_handle().try_state::<AppState>()
                {
                    state.with_repo(repo_id, |session| session.cancel_compare());
                }
            }
            _ => {}
        })
        .on_webview_event(|webview, event| {
            if let tauri::WebviewEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let folders: Vec<String> = paths
                    .iter()
                    .filter(|path| path.is_dir())
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
                if !folders.is_empty() {
                    let _ = webview.emit(
                        events::DROP_EVENT,
                        events::OpenPathsPayload { paths: folders },
                    );
                }
            }
        })
}
