//! Build-time application metadata embedded in the executable.

pub const APP_NAME: &str = "Augur Git";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

pub fn app_authors_display() -> String {
    APP_AUTHORS.split(':').collect::<Vec<_>>().join(", ")
}
pub const GIT_COMMIT: &str = env!("AUGUR_GIT_COMMIT");

#[cfg(test)]
mod tests {
    use super::GIT_COMMIT;

    #[test]
    fn commit_is_unknown_or_a_valid_git_object_id() {
        assert!(
            GIT_COMMIT == "unknown"
                || (matches!(GIT_COMMIT.len(), 40 | 64)
                    && GIT_COMMIT.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "invalid build commit: {GIT_COMMIT}"
        );
    }
}
