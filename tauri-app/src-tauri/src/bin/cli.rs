//! The `augurgit-tauri` command.
//!
//! A bare invocation opens the current directory; explicit paths open those
//! repositories. When an instance is already running the paths are forwarded
//! and this process exits without opening a window.
//!
//! Unlike the desktop binary this build keeps the console attached so
//! `--help` and `--version` print to the terminal the user typed them in.

fn main() {
    let invocation = augur_core::cli::parse_from_env();
    augur_git_tauri_lib::run(invocation, false);
}
