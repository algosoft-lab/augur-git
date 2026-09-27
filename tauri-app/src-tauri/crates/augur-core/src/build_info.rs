//! Build-time application metadata.
//!
//! The Git commit is injected by the workspace build script. When the crate is
//! built without it (for example `cargo test` inside the crate alone) the
//! value degrades to `unknown` instead of failing to compile.

/// Product name shown in the interface, window titles, and About window.
pub const APP_NAME: &str = "Augur Git Tauri";
/// Executable name of the desktop application.
pub const APP_BINARY: &str = "augur-git-tauri";
/// Command the shell installer exposes to users.
pub const CLI_COMMAND_NAME: &str = "augurgit-tauri";
/// Bundle and application identifier. Must differ from the GPUI application
/// (`com.augur.git`) so both products can be installed side by side.
pub const APP_IDENTIFIER: &str = "com.augur.git.tauri";

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

/// Comma-separated author list for the About window.
pub fn app_authors_display() -> String {
    APP_AUTHORS.split(':').collect::<Vec<_>>().join(", ")
}

/// Commit the binaries were built from, or `unknown`.
pub const GIT_COMMIT: &str = match option_env!("AUGUR_GIT_COMMIT") {
    Some(value) => value,
    None => "unknown",
};

/// `Name 0.1.0 (abcdef1)` used by `--version` and the About window.
pub fn version_line() -> String {
    if GIT_COMMIT == "unknown" {
        format!("{APP_NAME} {APP_VERSION}")
    } else {
        format!("{APP_NAME} {APP_VERSION} ({GIT_COMMIT})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_distinct_from_the_gpui_application() {
        assert_ne!(APP_IDENTIFIER, "com.augur.git");
        assert_ne!(CLI_COMMAND_NAME, "augurgit");
    }

    #[test]
    fn commit_is_unknown_or_a_valid_git_object_id() {
        assert!(
            GIT_COMMIT == "unknown"
                || (matches!(GIT_COMMIT.len(), 7 | 40 | 64)
                    && GIT_COMMIT.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "invalid build commit: {GIT_COMMIT}"
        );
    }
}
