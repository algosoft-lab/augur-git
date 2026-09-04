#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod agent;
mod core;
mod dropdown;
mod extension;
mod git;
mod logging;
mod terminal;
mod theme;
mod workspace;

use gpui::{AssetSource, SharedString};

/// Local assets (`assets/icons/*.svg`, Lucide MIT) take precedence over the
/// built-in gpui-component assets.
struct AppAssets;

/// Embedded local assets. Add new assets here as they are introduced.
/// Every file under `assets/icons/` must be registered: sidebar and toolbar
/// icons reference these paths via `crate::git::lucide`, and the bundled
/// gpui-component asset set does not contain them. A unit test keeps this
/// list in sync with the directory.
fn local_asset(path: &str) -> Option<&'static [u8]> {
    match path {
        "augur-git-logo.svg" => {
            Some(include_bytes!("../assets/augur-git-logo.svg").as_slice())
        }
        "icons/archive.svg" => {
            Some(include_bytes!("../assets/icons/archive.svg").as_slice())
        }
        "icons/archive-restore.svg" => Some(
            include_bytes!("../assets/icons/archive-restore.svg").as_slice(),
        ),
        "icons/download.svg" => {
            Some(include_bytes!("../assets/icons/download.svg").as_slice())
        }
        "icons/git-branch.svg" => {
            Some(include_bytes!("../assets/icons/git-branch.svg").as_slice())
        }
        "icons/git-branch-plus.svg" => Some(
            include_bytes!("../assets/icons/git-branch-plus.svg").as_slice(),
        ),
        "icons/git-commit-horizontal.svg" => Some(
            include_bytes!("../assets/icons/git-commit-horizontal.svg")
                .as_slice(),
        ),
        "icons/git-merge.svg" => {
            Some(include_bytes!("../assets/icons/git-merge.svg").as_slice())
        }
        "icons/pencil.svg" => {
            Some(include_bytes!("../assets/icons/pencil.svg").as_slice())
        }
        "icons/refresh-cw.svg" => {
            Some(include_bytes!("../assets/icons/refresh-cw.svg").as_slice())
        }
        "icons/trash-2.svg" => {
            Some(include_bytes!("../assets/icons/trash-2.svg").as_slice())
        }
        "icons/upload.svg" => {
            Some(include_bytes!("../assets/icons/upload.svg").as_slice())
        }
        _ => None,
    }
}

impl AssetSource for AppAssets {
    fn load(
        &self,
        path: &str,
    ) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(bytes) = local_asset(path) {
            return Ok(Some(std::borrow::Cow::Borrowed(bytes)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut paths = gpui_component_assets::Assets.list(path)?;
        if path.is_empty() {
            paths.extend([
                SharedString::from("augur-git-logo.svg"),
                SharedString::from("icons/archive.svg"),
                SharedString::from("icons/archive-restore.svg"),
                SharedString::from("icons/download.svg"),
                SharedString::from("icons/git-branch.svg"),
                SharedString::from("icons/git-branch-plus.svg"),
                SharedString::from("icons/git-commit-horizontal.svg"),
                SharedString::from("icons/git-merge.svg"),
                SharedString::from("icons/pencil.svg"),
                SharedString::from("icons/refresh-cw.svg"),
                SharedString::from("icons/trash-2.svg"),
                SharedString::from("icons/upload.svg"),
            ]);
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::local_asset;

    /// Every icon file shipped in `assets/icons/` must be embedded via
    /// `local_asset`, otherwise the sidebar and toolbar render it blank and
    /// `debug.log` fills with "could not find asset" errors.
    #[test]
    fn local_assets_register_every_icon_on_disk() {
        let entries = std::fs::read_dir("assets/icons")
            .expect("assets/icons directory must exist");
        let mut checked = 0;
        for entry in entries {
            let entry = entry.expect("readable directory entry");
            let name = entry.file_name();
            let name = name.to_string_lossy();
            assert!(
                name.ends_with(".svg"),
                "unexpected non-SVG file in assets/icons: {name}"
            );
            let path = format!("icons/{name}");
            assert!(
                local_asset(&path).is_some(),
                "{path} exists on disk but is not registered in local_asset"
            );
            checked += 1;
        }
        assert!(checked >= 10, "expected the full local icon set");
    }

    #[test]
    fn logo_asset_is_registered() {
        assert!(local_asset("augur-git-logo.svg").is_some());
    }
}

fn main() {
    configure_windows_error_mode();
    logging::init();
    log::info!("[app] starting augur-git");

    let app = gpui_platform::application().with_assets(AppAssets);
    workspace::run(app);
}

#[cfg(windows)]
fn configure_windows_error_mode() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        GetErrorMode, SEM_FAILCRITICALERRORS, SetErrorMode,
    };

    unsafe {
        let error_mode = GetErrorMode();
        SetErrorMode(error_mode | SEM_FAILCRITICALERRORS);
    }
}

#[cfg(not(windows))]
fn configure_windows_error_mode() {}
