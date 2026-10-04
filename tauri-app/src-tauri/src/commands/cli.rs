//! Serialized CLI installation operations, isolated from repository commands.
#[cfg(unix)]
static INSTALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
async fn operation(action: &'static str) -> Result<augur_core::cli_install::Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = INSTALL_LOCK
            .lock()
            .map_err(|_| "CLI installation lock failed")?;
        let context = augur_core::cli_install::Context::from_env()?;
        match action {
            "install" => context.install(),
            "uninstall" => context.uninstall(),
            _ => context.status(),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(unix)]
#[tauri::command]
pub async fn get_cli_status() -> Result<augur_core::cli_install::Status, String> {
    operation("status").await
}
#[cfg(unix)]
#[tauri::command]
pub async fn install_cli() -> Result<augur_core::cli_install::Status, String> {
    operation("install").await
}
#[cfg(unix)]
#[tauri::command]
pub async fn uninstall_cli() -> Result<augur_core::cli_install::Status, String> {
    operation("uninstall").await
}

#[cfg(not(unix))]
#[tauri::command]
pub async fn get_cli_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"state": "unsupported"}))
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn install_cli() -> Result<serde_json::Value, String> {
    Err("CLI installation is currently supported on macOS and Linux.".into())
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn uninstall_cli() -> Result<serde_json::Value, String> {
    install_cli().await
}
