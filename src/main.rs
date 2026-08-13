#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod core;
mod git;
mod workspace;

fn main() {
    env_logger::init();

    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);
    workspace::run(app);
}
