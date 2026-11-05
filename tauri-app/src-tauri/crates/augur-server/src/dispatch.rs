//! The HTTP command dispatcher: one named command in, one JSON result out.
//!
//! The command names and parameter spellings match what the Tauri invoke
//! protocol carries, so the shared frontend bridge works against either
//! transport without knowing which one is underneath.

use augur_service::commands::{agent_prompt, app, clone_cmd, credentials, git_config, lfs, repo};
use augur_service::error::{CommandError, Result};
use augur_service::state::AppState;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Execute one named command with a JSON argument object.
pub async fn dispatch(
    state: &AppState,
    command: &str,
    args: Value,
) -> std::result::Result<Value, CommandError> {
    let args = args.as_object().cloned().unwrap_or_default();
    let value = dispatch_typed(state, command, &args).await?;
    serde_json::to_value(value).map_err(|error| {
        CommandError::new("err-encode", format!("result encoding failed: {error}"))
    })
}

async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Map<String, Value>,
) -> Result<Value> {
    match command {
        // Repository lifecycle and reads.
        "bootstrap" => res(Ok(repo::bootstrap(state, "web"))),
        "open_repository" => res(repo::open_repository(
            state,
            arg(args, "path")?,
            arg(args, "location")?,
        )),
        "close_repository" => res(Ok({ repo::close_repository(state, arg(args, "repoId")?); })),
        "refresh_repository" => res(repo::refresh_repository(state, arg(args, "repoId")?)),
        "set_log_scope" => res(repo::set_log_scope(
            state,
            arg(args, "repoId")?,
            arg(args, "upstream")?,
        )),
        "load_more_log_page" => res(repo::load_more_log_page(state, arg(args, "repoId")?)),
        "select_commit" => res(repo::select_commit(
            state,
            arg(args, "repoId")?,
            arg(args, "requestId")?,
            arg(args, "oid")?,
        )),
        "request_commit_message" => res(repo::request_commit_message(
            state,
            arg(args, "repoId")?,
            arg(args, "oid")?,
        )),
        "load_commit_file_diff" => res(repo::load_commit_file_diff(
            state,
            arg(args, "repoId")?,
            arg(args, "requestId")?,
            arg(args, "oid")?,
            arg(args, "mergeParent")?,
            arg(args, "file")?,
        )),
        "load_working_tree_diff" => res(repo::load_working_tree_diff(
            state,
            arg(args, "repoId")?,
            arg(args, "requestId")?,
            arg(args, "kind")?,
            arg(args, "file")?,
        )),
        "load_image_preview" => {
            res(repo::load_image_preview(state, arg(args, "repoId")?, arg(args, "target")?).await)
        }
        "working_tree_operation" => res(repo::working_tree_operation(
            state,
            arg(args, "repoId")?,
            arg(args, "action")?,
            arg(args, "files")?,
            arg(args, "all")?,
        )),
        "run_action" => res(repo::run_action(state, arg(args, "repoId")?, arg(args, "action")?)),
        "checkout" => res(repo::checkout(state, arg(args, "repoId")?, arg(args, "target")?)),
        "start_compare" => res(repo::start_compare(
            state,
            arg(args, "repoId")?,
            arg(args, "base")?,
            arg(args, "target")?,
        )),
        "cancel_compare" => res(repo::cancel_compare(state, arg(args, "repoId")?)),
        "export_patch" => res(repo::export_patch(
            state,
            arg(args, "repoId")?,
            arg(args, "base")?,
            arg(args, "target")?,
            arg(args, "destination")?,
        )),
        "probe_merge" => res(repo::probe_merge(state, arg(args, "repoId")?, arg(args, "source")?).await),
        "probe_rebase" => {
            res(repo::probe_rebase(state, arg(args, "repoId")?, arg(args, "target")?).await)
        }
        "preview_reset" => {
            res(repo::preview_reset(state, arg(args, "repoId")?, arg(args, "target")?).await)
        }
        "read_commit_message" => {
            res(repo::read_commit_message(state, arg(args, "repoId")?, arg(args, "oid")?).await)
        }
        "graph_layout" => res(Ok(repo::graph_layout(arg(args, "rows")?, arg(args, "remoteNames")?))),
        "column_visibility" => res(Ok(repo::column_visibility(
            arg(args, "totalWidth")?,
            arg(args, "treeWidth")?,
        ))),
        "set_graph_history" => res(repo::set_graph_history(state, arg(args, "preference")?)),

        // Application settings and session.
        "update_settings" => res(app::update_settings(state, arg(args, "patch")?)),
        "set_lfs_download_preferences" => res(Ok({
            app::set_lfs_download_preferences(state, arg(args, "automatic")?, arg(args, "onOpen")?);
        })),
        "set_language" => res(Ok({ app::set_language(state, arg(args, "language")?); })),
        "set_theme" => res(Ok({ app::set_theme(state, arg(args, "theme")?); })),
        "set_view" => res(Ok({ app::set_view(state, arg(args, "view")?); })),
        "set_auto_refresh_target" => res(app::set_auto_refresh_target(
            state,
            arg(args, "repoId")?,
            arg(args, "generation")?,
        )),
        "set_typography" => res(Ok({ app::set_typography(state, arg(args, "typography")?); })),
        "set_commit_action" => res(Ok({ app::set_commit_action(state, arg(args, "action")?); })),
        "set_diff_layout" => res(Ok({ app::set_diff_layout(state, arg(args, "layout")?); })),
        "set_layout" => res(app::set_layout(state, arg(args, "layout")?)),
        "set_workspace_tabs" => res(app::set_workspace_tabs(
            state,
            arg(args, "tabs")?,
            arg(args, "active")?,
        )),
        "set_shortcut" => res(app::set_shortcut(
            state,
            arg(args, "command")?,
            arg(args, "keys")?,
        )),
        "validate_shortcut" => res(
            app::validate_shortcut(arg(args, "value")?)
                .map_err(|detail| CommandError::new("err-invalid-shortcut", detail)),
        ),
        "flush_state" => res(Ok(app::flush_state(state))),
        "take_pending_paths" => res(Ok(app::take_pending_paths(state))),
        "notify" => res(Ok(app::notify(state, arg(args, "level")?, arg(args, "message")?))),
        "request_open_paths" => res(Ok({
            let sink = state.sink().clone();
            app::deliver_open_paths(state, sink.as_ref(), arg(args, "paths")?);
        })),
        "repository_summary" => res(Ok(app::repository_summary_full(state, arg(args, "repoId")?))),
        "current_config" => res(Ok(app::current_config(state))),
        "theme_options" => res(Ok(augur_core::config::ThemePreference::ALL.to_vec())),
        // The browser reports its own focus, keeping the auto-refresh monitor
        // honest for whichever tab is visible.
        "set_main_window_focused" => res(Ok({
            state.set_main_window_focused(arg(args, "focused")?);
        })),

        // Git LFS.
        "lfs_snapshot" => res(lfs::lfs_snapshot(state, arg(args, "repoId")?).await),
        "lfs_environment" => res(Ok(lfs::lfs_environment())),
        "lfs_credential_response" => res(Ok(lfs::lfs_credential_response(
            arg(args, "requestId")?,
            arg(args, "response")?,
        ))),
        "lfs_initialize" => res(lfs::lfs_initialize(state, arg(args, "repoId")?).await),
        "lfs_fetch" => res(lfs::lfs_fetch(
            state,
            arg(args, "repoId")?,
            arg(args, "operationId")?,
            arg(args, "allHistory")?,
            arg(args, "remote")?,
            arg(args, "reference")?,
        )
        .await),
        "lfs_fsck" => res(lfs::lfs_fsck(state, arg(args, "repoId")?).await),
        "lfs_cancel" => res(Ok(lfs::lfs_cancel(arg(args, "operationId")?))),
        "lfs_download_file" => res(lfs::lfs_download_file(state, arg(args, "repoId")?, arg(args, "path")?).await),
        "lfs_track" => res(lfs::lfs_track(
            state,
            arg(args, "repoId")?,
            arg(args, "pattern")?,
            arg(args, "lockable")?,
        )
        .await),
        "lfs_untrack" => res(lfs::lfs_untrack(state, arg(args, "repoId")?, arg(args, "pattern")?).await),
        "lfs_locks" => res(lfs::lfs_locks(state, arg(args, "repoId")?).await),
        "lfs_lock" => res(lfs::lfs_lock(state, arg(args, "repoId")?, arg(args, "path")?).await),
        "lfs_unlock" => res(lfs::lfs_unlock(
            state,
            arg(args, "repoId")?,
            arg(args, "id")?,
            arg(args, "force")?,
        )
        .await),
        "lfs_cache_preview" => res(lfs::lfs_cache_preview(state, arg(args, "repoId")?).await),
        "lfs_prune" => res(lfs::lfs_prune(state, arg(args, "repoId")?).await),
        "lfs_update_preferences" => res(lfs::lfs_update_preferences(
            state,
            arg(args, "repoId")?,
            arg(args, "preferences")?,
        )
        .await),

        // Cloning.
        "lfs_clone" => res(lfs::lfs_clone(
            state,
            arg(args, "remote")?,
            arg(args, "parent")?,
            arg(args, "directoryName")?,
            arg(args, "downloadLfs")?,
            arg(args, "operationId")?,
        )
        .await),
        "cancel_lfs_clone" => res(Ok(clone_cmd::cancel_task(&arg::<String>(args, "operationId")?))),

        // Global Git configuration.
        "credential_snapshot" => res(Ok(credentials::snapshot(state))),
        "update_credential_settings" => {
            res(credentials::update_settings(state, arg(args, "patch")?))
        }
        "save_credential" => res(credentials::save_entry(state, arg(args, "input")?)),
        "delete_credential" => res(credentials::delete_entry(state, arg(args, "id")?)),
        "credential_login_response" => res(Ok(credentials::login_response(
            state,
            arg(args, "requestId")?,
            arg(args, "answer")?,
        ))),
        "import_git_credentials" => res(credentials::import_git_store(state)),
        "credential_lookup" => res(Ok(credentials::lookup(state, arg(args, "url")?))),
        "test_credential" => res(credentials::test_entry(state, arg(args, "id")?).await),

        "read_git_config" => res(Ok(git_config::read_git_config(arg(args, "location")?))),
        "write_git_config" => res(git_config::write_git_config(
            arg(args, "entries")?,
            arg(args, "location")?,
        )),
        "probe_commit_identity" => {
            res(git_config::probe_commit_identity(state, arg(args, "repoId")?).await)
        }

        // Agent prompts.
        "generate_agent_prompt" => res(agent_prompt::generate_agent_prompt(
            state,
            arg(args, "repoId")?,
            arg(args, "request")?,
        )
        .await),

        // The web frontend has no updater; a null snapshot reads as "no
        // update information" instead of an error on every boot.
        "get_update_snapshot" => res(Ok(Value::Null)),

        // The server has no windows to open; the shared frontend hides these
        // entry points in web mode, and a stray call is a no-op.
        "open_about_window" | "open_settings_window" | "open_compare_window"
        | "focus_main_window" | "close_compare_window" => Ok(Value::Null),

        // Desktop-only capability probes the web frontend never calls.
        "list_wsl_distros" => res(Ok(Vec::<String>::new())),
        "list_font_families" => res(Ok(web_font_families())),

        unknown => Err(CommandError::new(
            "err-unknown-command",
            format!("{unknown} is not a command of this server"),
        )),
    }
}

/// Serialize one command result. The argument is either an already-computed
/// value or a `Result` from a fallible service call.
fn res<T: serde::Serialize>(value: Result<T>) -> Result<Value> {
    let value = value?;
    serde_json::to_value(value).map_err(|error| {
        CommandError::new("err-encode", format!("result encoding failed: {error}"))
    })
}

/// Read one camelCase-named argument, falling back to its snake_case spelling.
/// A missing argument deserializes as null, which `Option<T>` accepts and
/// required types reject.
fn arg<T: DeserializeOwned>(
    args: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<T> {
    let value = args
        .get(name)
        .or_else(|| {
            let snake = camel_to_snake(name);
            (snake != name).then(|| args.get(snake.as_str())).flatten()
        })
        .cloned()
        .unwrap_or(Value::Null);
    serde_json::from_value(value)
        .map_err(|error| CommandError::new("err-invalid-parameters", format!("{name}: {error}")))
}

fn camel_to_snake(name: &str) -> String {
    let mut snake = String::with_capacity(name.len() + 4);
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            snake.push('_');
            snake.push(character.to_ascii_lowercase());
        } else {
            snake.push(character);
        }
    }
    snake
}

/// A curated stack of families every browser resolves, standing in for the
/// desktop machine's installed fonts.
fn web_font_families() -> Vec<String> {
    [
        "system-ui",
        "ui-sans-serif",
        "ui-serif",
        "ui-monospace",
        "Arial",
        "Georgia",
        "Times New Roman",
        "Courier New",
        "Noto Sans CJK SC",
        "Noto Serif CJK SC",
        "Noto Sans Mono CJK SC",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use augur_service::state::ServiceStartup;
    use augur_service::store::JsonFileStore;
    use std::sync::Arc;

    async fn test_state() -> Arc<AppState> {
        let directory = std::env::temp_dir().join(format!(
            "augur-dispatch-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let settings = JsonFileStore::open_in(&directory, "settings.json").unwrap();
        let workspace = JsonFileStore::open_in(&directory, "workspace.json").unwrap();
        Arc::new(AppState::new(
            Arc::new(crate::sink::BroadcastSink::new(16)),
            ServiceStartup {
                settings_store: Arc::new(settings),
                workspace_store: Arc::new(workspace),
                secrets: augur_service::credentials::SecretStores::memory_only(),
                warnings: Vec::new(),
            },
        ))
    }

    #[tokio::test]
    async fn bootstrap_answers_with_the_web_window_role() {
        let state = test_state().await;
        let value = dispatch(&state, "bootstrap", serde_json::json!({})).await.unwrap();
        assert_eq!(value["window"], "web");
        assert!(value["config"].is_object());
        assert!(value["catalogs"].is_object());
    }

    #[tokio::test]
    async fn pure_commands_answer_without_a_repository() {
        let state = test_state().await;
        let themes = dispatch(&state, "theme_options", serde_json::json!({}))
            .await
            .unwrap();
        assert!(themes.as_array().expect("theme list").len() > 3);

        let layout = dispatch(
            &state,
            "graph_layout",
            serde_json::json!({ "rows": [], "remoteNames": [] }),
        )
        .await
        .unwrap();
        assert_eq!(layout["graph"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn snake_case_arguments_are_accepted_like_the_desktop_sends_them() {
        let state = test_state().await;
        let value = dispatch(
            &state,
            "column_visibility",
            serde_json::json!({ "total_width": 1200.0, "tree_width": 300.0 }),
        )
        .await
        .unwrap();
        assert!(value["author"].is_boolean());
    }

    #[tokio::test]
    async fn an_unknown_command_is_refused_with_a_key() {
        let state = test_state().await;
        let error = dispatch(&state, "not_a_command", serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(error.key, "err-unknown-command");
    }

    #[tokio::test]
    async fn a_missing_required_argument_names_the_parameter() {
        let state = test_state().await;
        let error = dispatch(&state, "refresh_repository", serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(error.key, "err-invalid-parameters");
        assert!(error.detail.contains("repoId"), "{}", error.detail);
    }

    #[test]
    fn camel_case_names_convert_for_the_fallback_lookup() {
        assert_eq!(camel_to_snake("repoId"), "repo_id");
        assert_eq!(camel_to_snake("operationId"), "operation_id");
        assert_eq!(camel_to_snake("path"), "path");
    }
}
