//! Filesystem path helpers shared across the application.

use std::path::{Path, PathBuf};

/// Convert a Windows extended-length (verbatim) path to the plain spelling
/// accepted by shells and suitable for display. `std::fs::canonicalize`
/// returns paths with a `\\?\` prefix on Windows; `cmd.exe` treats that
/// spelling as an unsupported UNC working directory and users do not expect
/// it in the interface. Keep non-verbatim and non-drive extended paths
/// unchanged. On other platforms this is the identity function.
pub(crate) fn normalize_extended_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            let is_drive_path = rest.as_bytes().get(1) == Some(&b':');
            if is_drive_path {
                return PathBuf::from(rest);
            }
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::normalize_extended_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn plain_paths_stay_unchanged() {
        assert_eq!(
            normalize_extended_path(Path::new("/home/example/repo")),
            PathBuf::from("/home/example/repo")
        );
        assert_eq!(
            normalize_extended_path(Path::new("relative/repo")),
            PathBuf::from("relative/repo")
        );
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_drive_paths_drop_the_prefix() {
        assert_eq!(
            normalize_extended_path(Path::new(r"\\?\C:\Users\example\repo")),
            PathBuf::from(r"C:\Users\example\repo")
        );
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_unc_paths_become_regular_unc_paths() {
        assert_eq!(
            normalize_extended_path(Path::new(r"\\?\UNC\server\share\repo")),
            PathBuf::from(r"\\server\share\repo")
        );
    }

    #[cfg(windows)]
    #[test]
    fn non_drive_extended_paths_stay_unchanged() {
        assert_eq!(
            normalize_extended_path(Path::new(
                r"\\?\Volume{9cd7d4d2-0000-0000-0000-010000000000}"
            )),
            PathBuf::from(r"\\?\Volume{9cd7d4d2-0000-0000-0000-010000000000}")
        );
    }
}
