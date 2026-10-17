//! Process environment bootstrap for desktop launches.
//!
//! A macOS application launched from Finder, the Dock, or Launchpad inherits
//! launchd's minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`), which excludes
//! the directories where package managers install Git tooling. Git itself is
//! found at `/usr/bin/git`, but `git lfs` resolves the `git-lfs` helper through
//! the same `PATH` and fails with "git: 'lfs' is not a git command" when Git
//! LFS came from Homebrew or MacPorts. Extending the process `PATH` once at
//! startup fixes every child spawn — Git, `which git-lfs`, filters, and hooks
//! — without touching how individual commands are built.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Standard tool locations that GUI `PATH` values commonly miss, beyond the
/// entries the system path files already provide.
#[cfg(target_os = "macos")]
const MACOS_EXTRA_DIRECTORIES: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/local/bin",
    "/opt/local/sbin",
];

#[cfg(target_os = "macos")]
const MACOS_PATHS_FILE: &str = "/etc/paths";
#[cfg(target_os = "macos")]
const MACOS_PATHS_D_DIRECTORY: &str = "/etc/paths.d";

/// Extend the process search path for GUI launches that inherit a minimal
/// `PATH`. Missing standard directories are appended; existing entries keep
/// their order so the launch environment never loses precedence. Call this
/// from the process entry point before any thread or child process exists.
pub fn bootstrap_search_path() {
    #[cfg(target_os = "macos")]
    extend_macos_path();
}

#[cfg(target_os = "macos")]
fn extend_macos_path() {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut extra: Vec<PathBuf> = system_path_file_entries(Path::new(MACOS_PATHS_FILE))
        .chain(system_path_file_entries(Path::new(MACOS_PATHS_D_DIRECTORY)))
        .collect();
    extra.extend(MACOS_EXTRA_DIRECTORIES.iter().map(PathBuf::from));
    let augmented = augmented_path(&current, &extra);
    if augmented != current {
        // SAFETY: the bootstrap runs from `main` before any thread is spawned,
        // so no concurrent environment reader can observe the update.
        unsafe { std::env::set_var("PATH", augmented) };
    }
}

/// Read one whitespace-trimmed entry per line. A missing file yields nothing,
/// which lets the same reader handle `/etc/paths`, each `paths.d` fragment,
/// and test fixtures alike.
pub fn read_path_file_lines(file: &Path) -> Vec<String> {
    match std::fs::read_to_string(file) {
        Ok(content) => content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(target_os = "macos")]
fn system_path_file_entries(path: &Path) -> impl Iterator<Item = PathBuf> {
    let fragments: Vec<PathBuf> = if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|fragment| fragment.is_file())
            .collect();
        files.sort();
        files
    } else {
        vec![path.to_path_buf()]
    };
    fragments
        .into_iter()
        .flat_map(|fragment| read_path_file_lines(&fragment))
        .map(PathBuf::from)
}

/// Append every candidate directory that is missing from `current`, keeping
/// the existing entries and their order untouched. Directory arguments that
/// already appear anywhere in the search path are skipped, so a good launch
/// environment is never reordered or duplicated. A `current` value that
/// cannot be re-joined (a pathological entry containing the separator) is
/// returned unchanged rather than corrupted.
pub fn augmented_path(current: &OsString, extra: &[PathBuf]) -> OsString {
    // An unset or empty PATH parses as one empty entry, which would survive
    // as a stray separator; empty components carry no meaning and are dropped.
    let mut parts: Vec<PathBuf> = std::env::split_paths(current)
        .filter(|part| !part.as_os_str().is_empty())
        .collect();
    for candidate in extra {
        if !parts.contains(candidate) {
            parts.push(candidate.clone());
        }
    }
    match std::env::join_paths(&parts) {
        Ok(joined) => joined,
        Err(_) => current.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(value: &str) -> OsString {
        OsString::from(value)
    }

    #[test]
    fn missing_candidates_are_appended_in_order() {
        let extra = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ];
        assert_eq!(
            augmented_path(&os("/usr/bin:/bin"), &extra),
            os("/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin")
        );
    }

    #[test]
    fn present_candidates_are_not_duplicated_or_reordered() {
        let extra = vec![
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
        ];
        assert_eq!(
            augmented_path(&os("/usr/bin:/opt/homebrew/bin:/bin"), &extra),
            os("/usr/bin:/opt/homebrew/bin:/bin:/usr/local/bin")
        );
    }

    #[test]
    fn an_empty_path_accepts_every_candidate() {
        let extra = vec![PathBuf::from("/opt/homebrew/bin")];
        assert_eq!(augmented_path(&os(""), &extra), os("/opt/homebrew/bin"));
    }

    #[test]
    fn a_complete_path_stays_unchanged() {
        let extra = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ];
        assert_eq!(
            augmented_path(&os("/opt/homebrew/bin:/usr/local/bin"), &extra),
            os("/opt/homebrew/bin:/usr/local/bin")
        );
    }

    #[test]
    fn path_file_lines_are_trimmed_and_blank_lines_dropped() {
        let directory = std::env::temp_dir().join(format!(
            "augur-environment-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("paths");
        std::fs::write(&file, "/usr/bin\n\n  /opt/homebrew/bin  \n\n").unwrap();
        assert_eq!(
            read_path_file_lines(&file),
            vec!["/usr/bin".to_string(), "/opt/homebrew/bin".to_string()]
        );
        let missing = directory.join("missing");
        assert!(read_path_file_lines(&missing).is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
