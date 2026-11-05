// Hide the extra console window that Windows would otherwise attach to a
// release build. Debug builds keep it so `println!` from the CLI stays visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The desktop application entry point.

fn main() {
    // Finder and Dock launches inherit launchd's minimal PATH, which hides
    // Homebrew and MacPorts tooling such as git-lfs. This must run before any
    // command-line parsing or Git spawn in this process.
    augur_core::environment::bootstrap_search_path();
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--git-askpass")) {
        std::process::exit(if augur_git_tauri_lib::askpass::run_helper().is_ok() {
            0
        } else {
            1
        });
    }
    if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new("--git-credential-helper"))
    {
        let operation = std::env::args_os()
            .nth(2)
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::process::exit(
            if augur_git_tauri_lib::credentials::run_helper(&operation).is_ok() {
                0
            } else {
                1
            },
        );
    }
    let invocation = augur_core::cli::parse_from_env();
    augur_git_tauri_lib::run(invocation, false);
}
