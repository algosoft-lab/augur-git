#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod core;
mod git;
mod workspace;

use gpui::{AssetSource, SharedString};

/// 本地资产（assets/icons/*.svg，lucide MIT）：优先于 gpui-component 内置集
struct AppAssets;

/// 编译期内嵌本地图标（新增图标在此登记一行）
fn local_asset(path: &str) -> Option<&'static [u8]> {
    match path {
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
    env_logger::init();

    let app = gpui_platform::application().with_assets(AppAssets);
    workspace::run(app);
}
