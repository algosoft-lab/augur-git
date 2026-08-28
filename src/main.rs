#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod core;
mod git;
mod theme;
mod workspace;

#[cfg(debug_assertions)]
use std::fs::OpenOptions;
use std::io::{self, Write};

use gpui::{AssetSource, SharedString};

/// Local assets (`assets/icons/*.svg`, Lucide MIT) take precedence over the
/// built-in gpui-component assets.
struct AppAssets;

/// Embedded local assets. Add new assets here as they are introduced.
fn local_asset(path: &str) -> Option<&'static [u8]> {
    match path {
        "augur-git-logo.svg" => {
            Some(include_bytes!("../assets/augur-git-logo.svg").as_slice())
        }
        "icons/download.svg" => {
            Some(include_bytes!("../assets/icons/download.svg").as_slice())
        }
        "icons/git-branch.svg" => {
            Some(include_bytes!("../assets/icons/git-branch.svg").as_slice())
        }
        "icons/refresh-cw.svg" => {
            Some(include_bytes!("../assets/icons/refresh-cw.svg").as_slice())
        }
        "icons/git-commit-horizontal.svg" => Some(
            include_bytes!("../assets/icons/git-commit-horizontal.svg")
                .as_slice(),
        ),
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
                SharedString::from("icons/download.svg"),
                SharedString::from("icons/git-branch.svg"),
                SharedString::from("icons/refresh-cw.svg"),
                SharedString::from("icons/git-commit-horizontal.svg"),
            ]);
        }
        Ok(paths)
    }
}

fn main() {
    init_logging();
    log::info!("[app] starting augur-git");

    let app = gpui_platform::application().with_assets(AppAssets);
    workspace::run(app);
}

/// Initialize file-only logging without making startup depend on log-file creation.
fn init_logging() {
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    );
    builder
        .target(env_logger::Target::Pipe(logging_writer()))
        .write_style(env_logger::WriteStyle::Never);
    let _ = builder.try_init();
}

#[cfg(debug_assertions)]
fn logging_writer() -> Box<dyn Write + Send> {
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.log")
    {
        Ok(file) => Box::new(file),
        Err(_) => Box::new(io::sink()),
    }
}

#[cfg(not(debug_assertions))]
fn logging_writer() -> Box<dyn Write + Send> {
    Box::new(io::sink())
}
