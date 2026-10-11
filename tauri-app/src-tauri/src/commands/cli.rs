//! Serialized CLI installation operations, isolated from repository commands.
#[cfg(unix)]
static INSTALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(any(unix, windows))]
async fn operation(action: &'static str) -> Result<augur_core::cli_install::Status, String> {
    #[allow(unused_mut)]
    let mut status = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(unix)]
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
    .map_err(|error| error.to_string())??;
    #[cfg(unix)]
    let expected = status.path.clone();
    #[cfg(unix)]
    if let Ok(shell) =
        tauri::async_runtime::spawn_blocking(move || crate::cli_shell::detect(&expected)).await
    {
        status.shell = shell;
    }
    Ok(status)
}

#[cfg(any(unix, windows))]
#[tauri::command]
pub async fn get_cli_status() -> Result<augur_core::cli_install::Status, String> {
    operation("status").await
}
#[cfg(any(unix, windows))]
#[tauri::command]
pub async fn install_cli() -> Result<augur_core::cli_install::Status, String> {
    operation("install").await
}
#[cfg(any(unix, windows))]
#[tauri::command]
pub async fn uninstall_cli() -> Result<augur_core::cli_install::Status, String> {
    operation("uninstall").await
}

#[cfg(not(any(unix, windows)))]
#[tauri::command]
pub async fn get_cli_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"state": "unsupported"}))
}
#[cfg(not(any(unix, windows)))]
#[tauri::command]
pub async fn install_cli() -> Result<serde_json::Value, String> {
    Err("CLI installation is currently supported on macOS and Linux.".into())
}
#[cfg(not(any(unix, windows)))]
#[tauri::command]
pub async fn uninstall_cli() -> Result<serde_json::Value, String> {
    install_cli().await
}
