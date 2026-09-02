//! Executable path resolution for Agent profiles.
//!
//! A desktop application started from a GUI session (Finder, Dock, a window
//! manager, or a service manager) inherits a minimal `PATH` that usually omits
//! the per-user directories where CLI agents are commonly installed, such as a
//! custom npm global prefix on macOS. Bare profile names are therefore
//! resolved against both the process `PATH` and a list of well-known user bin
//! directories. Manually configured paths may start with `~`, which is
//! expanded here because process launchers do not perform shell expansion.

use std::path::{Component, Path, PathBuf};

/// Resolve a profile executable to a launchable path.
///
/// Paths that name a directory (including a leading `~`) must point at an
/// existing file. A bare name is searched for in the process `PATH` first and
/// then in [`extra_executable_directories`], so GUI-launched sessions behave
/// like interactive shells.
pub fn resolve_executable(path: &Path) -> anyhow::Result<PathBuf> {
    let path = expand_user_home(path);
    #[cfg(windows)]
    {
        resolve_windows_executable(&path)
    }
    #[cfg(not(windows))]
    {
        resolve_unix_executable(&path)
    }
}

/// Expand a leading `~` component to the current user's home directory.
///
/// Other paths are returned unchanged. When no home directory can be
/// resolved, a `~` path is also returned unchanged so the caller reports the
/// original spelling in its error message.
fn expand_user_home(path: &Path) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Normal(name)) = components.next() else {
        return path.to_path_buf();
    };
    if name != std::ffi::OsStr::new("~") {
        return path.to_path_buf();
    }
    let Some(home) = dirs::home_dir() else {
        return path.to_path_buf();
    };
    let rest = components.as_path();
    if rest.as_os_str().is_empty() {
        home
    } else {
        home.join(rest)
    }
}

/// Directories that commonly hold user-installed CLI tools but are absent
/// from the minimal `PATH` of GUI-launched desktop sessions.
fn extra_executable_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    #[cfg(windows)]
    {
        let mut push = |base: Option<std::ffi::OsString>, parts: &[&str]| {
            if let Some(base) = base.map(PathBuf::from) {
                directories.push(
                    parts.iter().fold(base, |path, part| path.join(part)),
                );
            }
        };
        push(std::env::var_os("APPDATA"), &["npm"]);
        push(std::env::var_os("USERPROFILE"), &[".npm-global"]);
        push(std::env::var_os("USERPROFILE"), &[".bun", "bin"]);
        push(std::env::var_os("USERPROFILE"), &[".cargo", "bin"]);
        push(std::env::var_os("LOCALAPPDATA"), &["pnpm"]);
    }

    #[cfg(unix)]
    {
        if let Some(home) = dirs::home_dir() {
            directories.push(home.join(".npm-global").join("bin"));
            directories.push(home.join(".local").join("bin"));
            directories.push(home.join(".volta").join("bin"));
            directories.push(home.join(".bun").join("bin"));
            directories.push(home.join(".cargo").join("bin"));
            directories.push(home.join(".yarn").join("bin"));
            #[cfg(target_os = "macos")]
            directories.push(home.join("Library").join("pnpm"));
            #[cfg(not(target_os = "macos"))]
            directories.push(home.join(".local").join("share").join("pnpm"));
        }
        #[cfg(target_os = "macos")]
        directories.push(PathBuf::from("/opt/homebrew/bin"));
        #[cfg(not(target_os = "macos"))]
        directories.push(PathBuf::from("/home/linuxbrew/.linuxbrew/bin"));
        directories.push(PathBuf::from("/usr/local/bin"));
    }

    directories
}

/// Every directory searched for a bare executable name, in order.
fn search_directories() -> Vec<PathBuf> {
    let path_variable = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_variable)
        .filter(|directory| !directory.as_os_str().is_empty())
        .chain(extra_executable_directories())
        .collect()
}

/// Look up a bare executable name in the given directories, in order.
fn find_in_directories(
    name: &Path,
    directories: &[PathBuf],
) -> Option<PathBuf> {
    directories.iter().find_map(|directory| {
        let candidate = directory.join(name);
        #[cfg(windows)]
        let found = windows_existing_candidate(&candidate);
        #[cfg(not(windows))]
        let found = is_executable_file(&candidate).then_some(candidate);
        found
    })
}

#[cfg(not(windows))]
fn resolve_unix_executable(path: &Path) -> anyhow::Result<PathBuf> {
    let has_directory_component =
        path.is_absolute() || path.components().count() > 1;
    if has_directory_component {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        anyhow::bail!("executable '{}' was not found", path.display());
    }
    if let Some(candidate) = find_in_directories(path, &search_directories()) {
        log::debug!(
            "[agent_terminal] resolved '{}' to {}",
            path.display(),
            candidate.display()
        );
        return Ok(candidate);
    }
    anyhow::bail!(
        "executable '{}' was not found in PATH or common user bin directories; install the CLI or configure an explicit executable path in Settings",
        path.display()
    );
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(windows)]
fn resolve_windows_executable(path: &Path) -> anyhow::Result<PathBuf> {
    let has_directory = path.is_absolute()
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
    if has_directory {
        if let Some(candidate) = windows_existing_candidate(path) {
            return Ok(candidate);
        }
        anyhow::bail!("executable '{}' was not found", path.display());
    }

    if let Some(candidate) = find_in_directories(path, &search_directories()) {
        log::debug!(
            "[agent_terminal] resolved '{}' to {}",
            path.display(),
            candidate.display()
        );
        return Ok(candidate);
    }
    anyhow::bail!(
        "executable '{}' was not found in PATH or common user bin \
         directories; configure an explicit executable path in Settings",
        path.display()
    );
}

/// Resolve a Windows executable using the same executable suffixes that an
/// interactive command shell accepts for npm-installed CLI shims.
#[cfg(windows)]
fn windows_existing_candidate(path: &Path) -> Option<PathBuf> {
    let candidates = if path.extension().is_some() {
        vec![path.to_path_buf()]
    } else {
        vec![
            path.with_extension("exe"),
            path.with_extension("cmd"),
            path.with_extension("bat"),
        ]
    };
    candidates.into_iter().find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_temp_dir(label: &str) -> PathBuf {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "augur-agent-resolve-{label}-{}-{timestamp}",
            std::process::id()
        ))
    }

    #[test]
    fn user_home_expansion_rewrites_only_leading_tilde() {
        let nested = Path::new("~/.npm-global/bin/claude");
        let expanded = expand_user_home(nested);
        match dirs::home_dir() {
            Some(home) => assert_eq!(
                expanded,
                home.join(".npm-global").join("bin").join("claude")
            ),
            None => assert_eq!(expanded, PathBuf::from(nested)),
        }
        assert_eq!(
            expand_user_home(Path::new("./agent")),
            PathBuf::from("./agent")
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_search_requires_the_execute_permission() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_dir("unix");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("test bin directory");
        let data_file = bin.join("augur-fake-agent");
        fs::write(&data_file, "not executable").expect("test file");

        assert!(!is_executable_file(&data_file));
        assert!(
            find_in_directories(Path::new("augur-fake-agent"), &[bin.clone()])
                .is_none()
        );

        fs::set_permissions(&data_file, fs::Permissions::from_mode(0o755))
            .expect("chmod test file");
        assert!(is_executable_file(&data_file));
        assert_eq!(
            find_in_directories(Path::new("augur-fake-agent"), &[bin])
                .expect("executable found after chmod"),
            data_file
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn directory_search_accepts_cmd_shims_without_an_extension() {
        let root = unique_temp_dir("windows");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("test bin directory");
        let shim = bin.join("augur-fake-agent.cmd");
        fs::write(&shim, "@echo off\r\n").expect("test shim");

        assert_eq!(
            find_in_directories(Path::new("augur-fake-agent"), &[bin])
                .expect("cmd shim should resolve"),
            shim
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_bare_executable_reports_search_guidance() {
        let error = resolve_executable(Path::new(
            "augur-definitely-missing-agent-0e9f",
        ))
        .expect_err("a name that does not exist must not resolve");
        assert!(error.to_string().contains("not found"));
        assert!(error.to_string().contains("Settings"));
    }
}
