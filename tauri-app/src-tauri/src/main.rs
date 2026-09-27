// Hide the extra console window that Windows would otherwise attach to a
// release build. Debug builds keep it so `println!` from the CLI stays visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The desktop application entry point.

fn main() {
    let invocation = augur_core::cli::parse_from_env();
    augur_git_tauri_lib::run(invocation, false);
}
