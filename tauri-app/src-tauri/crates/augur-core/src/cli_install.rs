//! User-owned CLI installation. Package manager artifacts are never modified.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{CString, OsString};
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};

use crate::build_info::{APP_BINARY, APP_IDENTIFIER};

#[derive(Clone)]
pub struct Context {
    pub home: PathBuf,
    pub config: PathBuf,
    pub executable: PathBuf,
    pub appimage: Option<PathBuf>,
    pub search_path: OsString,
    pub shell: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: &'static str,
    pub path: PathBuf,
    pub can_install: bool,
    pub can_remove: bool,
    pub package_managed: bool,
    pub path_command: Option<String>,
    pub detail: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    path: PathBuf,
    source: PathBuf,
    target: PathBuf,
    digest: Option<String>,
}

fn receipt_path(config: &Path) -> PathBuf {
    config.join("cli-install.json")
}
fn installation_directory(home: &Path) -> PathBuf {
    home.join(".local/share").join(APP_IDENTIFIER)
}
fn read_receipt(config: &Path) -> Result<Option<Receipt>, String> {
    match fs::read(receipt_path(config)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| format!("Invalid CLI installation record: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn digest(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|error| error.to_string())
}
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
fn owned(receipt: &Receipt) -> bool {
    if let Some(expected) = &receipt.digest {
        fs::symlink_metadata(&receipt.path).is_ok_and(|m| m.is_file())
            && digest(&receipt.path).is_ok_and(|actual| actual == *expected)
    } else {
        fs::read_link(&receipt.path).is_ok_and(|target| target == receipt.source)
    }
}
fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn writable(path: &Path) -> bool {
    CString::new(path.as_os_str().as_bytes())
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 })
}
fn same(a: &Path, b: &Path) -> bool {
    fs::canonicalize(a)
        .ok()
        .zip(fs::canonicalize(b).ok())
        .is_some_and(|(a, b)| a == b)
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl Context {
    pub fn from_env() -> Result<Self, String> {
        let home = dirs::home_dir().ok_or("Could not find the home directory")?;
        Ok(Self {
            // Keep the receipt stable when the CLI is launched outside an AppImage
            // environment that may override XDG_CONFIG_HOME.
            config: installation_directory(&home),
            home,
            executable: std::env::current_exe().map_err(|error| error.to_string())?,
            appimage: std::env::var_os("APPIMAGE").map(PathBuf::from),
            search_path: std::env::var_os("PATH").unwrap_or_default(),
            shell: std::env::var("SHELL").unwrap_or_default(),
        })
    }
    fn source(&self) -> PathBuf {
        self.executable.with_file_name("agit")
    }
    fn target(&self) -> PathBuf {
        self.appimage
            .clone()
            .unwrap_or_else(|| self.executable.clone())
    }
    fn destination(&self, receipt: Option<&Receipt>) -> PathBuf {
        if let Some(receipt) = receipt {
            return receipt.path.clone();
        }
        for dir in std::env::split_paths(&self.search_path) {
            if dir.is_absolute()
                && dir.starts_with(&self.home)
                && dir.is_dir()
                && writable(&dir)
                && fs::canonicalize(&dir).is_ok_and(|path| path.starts_with(&self.home))
            {
                return dir.join("agit");
            }
        }
        self.home.join(".local/bin/agit")
    }
    fn blocked(&self) -> Option<String> {
        if cfg!(target_os = "macos")
            && (self.executable.starts_with("/Volumes")
                || self
                    .executable
                    .components()
                    .any(|part| part.as_os_str() == "AppTranslocation"))
        {
            return Some(
                "Move Augur Git to a permanent location, such as Applications, then install agit."
                    .into(),
            );
        }
        if !executable(&self.target()) {
            return Some("The application executable is missing or is not executable.".into());
        }
        if !executable(&self.source()) {
            return Some(
                "This build does not include agit. Install a packaged macOS or Linux build.".into(),
            );
        }
        None
    }
    fn command(&self, path: &Path) -> String {
        let directory = shell_quote(&path.parent().unwrap().to_string_lossy());
        if self.shell.ends_with("fish") {
            format!("fish_add_path {directory}")
        } else if self.shell.ends_with("zsh") {
            format!(
                "printf '%s\\n' {} >> ~/.zshrc",
                shell_quote(&format!("export PATH={directory}:\"$PATH\""))
            )
        } else {
            let profile = if self.shell.ends_with("bash") && cfg!(target_os = "macos") {
                "~/.bash_profile"
            } else if self.shell.ends_with("bash") {
                "~/.bashrc"
            } else {
                "~/.profile"
            };
            format!(
                "printf '%s\\n' {} >> {profile}",
                shell_quote(&format!("export PATH={directory}:\"$PATH\""))
            )
        }
    }
    fn validate_destination(&self, path: &Path) -> Result<(), String> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
            || !path.starts_with(&self.home)
            || path.file_name().is_none_or(|name| name != "agit")
        {
            return Err("CLI installation record points outside the user directory".into());
        }
        let mut parent = path.parent().ok_or("Invalid CLI path")?;
        while !parent.exists() {
            parent = parent.parent().ok_or("Invalid CLI directory")?;
        }
        if !fs::canonicalize(parent)
            .map_err(|e| e.to_string())?
            .starts_with(fs::canonicalize(&self.home).map_err(|e| e.to_string())?)
        {
            return Err("CLI installation directory points outside the user directory".into());
        }
        Ok(())
    }
    pub fn status(&self) -> Result<Status, String> {
        let receipt = read_receipt(&self.config)?;
        let path = self.destination(receipt.as_ref());
        self.validate_destination(&path)?;
        let ours = receipt.as_ref().is_some_and(owned);
        let installed = exists(&path);
        let active = std::env::split_paths(&self.search_path)
            .map(|dir| dir.join("agit"))
            .find(|candidate| executable(candidate));
        let mut status = Status {
            state: "not-installed",
            path: path.clone(),
            can_install: true,
            can_remove: ours,
            package_managed: false,
            path_command: None,
            detail: String::new(),
        };
        // A symlink to this bundled launcher is externally managed unless we recorded it.
        if let Some(active) = &active {
            if !same(active, &path) && same(active, &self.source()) {
                status.state = "available";
                status.path = active.clone();
                status.package_managed = true;
                status.can_remove = false;
                status.can_install = false;
                return Ok(status);
            }
            if !same(active, &path) {
                status.state = "conflict";
                status.detail = active.to_string_lossy().into_owned();
                status.can_install = false;
                return Ok(status);
            }
        }
        if active.is_none() {
            let mut packaged = vec![PathBuf::from("/usr/bin/agit")];
            if cfg!(target_os = "macos") {
                packaged = vec![
                    PathBuf::from("/opt/homebrew/bin/agit"),
                    PathBuf::from("/usr/local/bin/agit"),
                ];
                if let Some(prefix) = std::env::var_os("HOMEBREW_PREFIX") {
                    packaged.insert(0, PathBuf::from(prefix).join("bin/agit"));
                }
            }
            if let Some(external) = packaged
                .into_iter()
                .find(|candidate| same(candidate, &self.source()) && !same(candidate, &path))
            {
                status.state = "not-on-path";
                status.path_command = Some(self.command(&external));
                status.path = external;
                status.package_managed = true;
                status.can_install = false;
                status.can_remove = false;
                return Ok(status);
            }
        }
        if installed && !ours {
            if same(&path, &self.source()) {
                status.state = "available";
                status.package_managed = true;
                status.can_remove = false;
                status.can_install = false;
            } else {
                status.state = "conflict";
                status.can_install = false;
            }
            return Ok(status);
        }
        if ours {
            let receipt = receipt.as_ref().unwrap();
            if !executable(&path)
                || !executable(&receipt.target)
                || receipt.target != self.target()
                || (receipt.digest.is_none() && receipt.source != self.source())
                || receipt.digest.as_ref().is_some_and(|expected| {
                    digest(&self.source()).is_ok_and(|actual| actual != *expected)
                })
            {
                status.state = "broken";
            } else if active.as_ref().is_some_and(|active| same(active, &path)) {
                status.state = "available";
            } else {
                status.state = "not-on-path";
            }
        }
        let directory_on_path =
            std::env::split_paths(&self.search_path).any(|dir| same(&dir, path.parent().unwrap()));
        if !directory_on_path && !status.package_managed {
            status.path_command = Some(self.command(&path));
        }
        if let Some(reason) = self.blocked() {
            status.can_install = false;
            status.detail = reason;
        }
        Ok(status)
    }
    pub fn install(&self) -> Result<Status, String> {
        let status = self.status()?;
        if status.package_managed {
            return Ok(status);
        }
        if !status.can_install {
            return Err(if status.detail.is_empty() {
                "The agit command is occupied by another installation".into()
            } else {
                status.detail
            });
        }
        let path = status.path;
        let parent = path.parent().ok_or("Invalid CLI path")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        fs::create_dir_all(&self.config).map_err(|error| error.to_string())?;
        let source = self.source();
        let mut receipt = Receipt {
            path: path.clone(),
            source: source.clone(),
            target: self.target(),
            digest: None,
        };
        let staged = parent.join(format!(".agit-{}", std::process::id()));
        if exists(&staged) {
            return Err("A CLI installation is already in progress".into());
        }
        let staged_result = (|| {
            if self.appimage.is_some() {
                let bytes = fs::read(&source).map_err(|error| error.to_string())?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&staged)
                    .map_err(|error| error.to_string())?;
                file.write_all(&bytes).map_err(|error| error.to_string())?;
                file.sync_all().map_err(|error| error.to_string())?;
                fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
                    .map_err(|error| error.to_string())?;
                receipt.digest = Some(digest(&staged)?);
            } else {
                symlink(&source, &staged).map_err(|error| error.to_string())?;
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = staged_result {
            let _ = fs::remove_file(&staged);
            return Err(error);
        }
        let result = (|| {
            // Recheck before committing so a changed destination is not replaced.
            let old = read_receipt(&self.config)?;
            if exists(&path) && !old.as_ref().is_some_and(owned) {
                return Err("The agit command changed during installation".into());
            }
            let record = receipt_path(&self.config);
            let record_temp = self
                .config
                .join(format!(".cli-install-{}.json", std::process::id()));
            let backup = parent.join(format!(".agit-backup-{}", std::process::id()));
            if exists(&backup) {
                return Err("A CLI repair is already in progress".into());
            }
            fs::write(
                &record_temp,
                serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let had_old = exists(&path);
            if had_old {
                if let Ok(link) = fs::read_link(&path) {
                    symlink(link, &backup).map_err(|e| e.to_string())?;
                } else {
                    fs::copy(&path, &backup).map_err(|e| e.to_string())?;
                }
            }
            let commit = fs::rename(&staged, &path).and_then(|_| fs::rename(&record_temp, &record));
            if let Err(error) = commit {
                let _ = fs::remove_file(&path);
                if had_old {
                    let _ = fs::rename(&backup, &path);
                }
                let _ = fs::remove_file(&record_temp);
                return Err(error.to_string());
            }
            if had_old {
                let _ = fs::remove_file(&backup);
            }
            self.status()
        })();
        let _ = fs::remove_file(&staged);
        result
    }
    pub fn uninstall(&self) -> Result<Status, String> {
        let status = self.status()?;
        if status.package_managed {
            return Err("Remove this command with its package manager".into());
        }
        if let Some(receipt) = read_receipt(&self.config)? {
            if exists(&receipt.path) {
                if !owned(&receipt) {
                    return Err("The agit command was replaced; it will not be removed".into());
                }
                fs::remove_file(&receipt.path).map_err(|e| e.to_string())?;
            }
            fs::remove_file(receipt_path(&self.config)).map_err(|e| e.to_string())?;
        }
        self.status()
    }
}

pub fn launch_target(executable_path: &Path) -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    launch_target_in(executable_path, &installation_directory(&home))
}

fn launch_target_in(executable_path: &Path, config: &Path) -> Result<PathBuf, String> {
    if let Some(receipt) = read_receipt(config)? {
        if receipt.digest.is_some() && same(&receipt.path, executable_path) {
            if !executable(&receipt.target) {
                return Err(
                    "The AppImage was moved or removed. Open the app and repair agit in Settings."
                        .into(),
                );
            }
            return Ok(receipt.target);
        }
    }
    let target = executable_path.with_file_name(APP_BINARY);
    if !executable(&target) {
        return Err("Augur Git was moved or removed. Repair agit in Settings.".into());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        context: Context,
        root: PathBuf,
    }
    impl Fixture {
        fn new(on_path: bool) -> Self {
            let root = std::env::temp_dir().join(format!(
                "augur-cli-install-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let home = root.join("home");
            let bin = home.join(".local/bin");
            let app = root.join("app");
            fs::create_dir_all(&bin).unwrap();
            fs::create_dir_all(&app).unwrap();
            for name in ["agit", APP_BINARY] {
                fs::write(app.join(name), "#!/bin/sh\nexit 0\n").unwrap();
                fs::set_permissions(app.join(name), fs::Permissions::from_mode(0o755)).unwrap();
            }
            let context = Context {
                home: home.clone(),
                config: home.join("config"),
                executable: app.join(APP_BINARY),
                appimage: None,
                search_path: if on_path {
                    bin.into_os_string()
                } else {
                    OsString::new()
                },
                shell: "/bin/zsh".into(),
            };
            Self { context, root }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn installs_idempotently_and_removes_only_its_link() {
        let fixture = Fixture::new(true);
        assert_eq!(fixture.context.status().unwrap().state, "not-installed");
        let installed = fixture.context.install().unwrap();
        assert_eq!(installed.state, "available");
        assert!(installed.can_remove);
        assert_eq!(fixture.context.install().unwrap().state, "available");
        assert_eq!(fixture.context.uninstall().unwrap().state, "not-installed");
        assert_eq!(fixture.context.uninstall().unwrap().state, "not-installed");
    }
    #[test]
    fn refuses_foreign_files_and_replaced_owned_files() {
        let fixture = Fixture::new(true);
        let path = fixture.context.install().unwrap().path;
        fs::remove_file(&path).unwrap();
        fs::write(&path, "foreign").unwrap();
        assert_eq!(fixture.context.status().unwrap().state, "conflict");
        assert!(fixture.context.install().is_err());
        assert!(fixture.context.uninstall().is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "foreign");
    }
    #[test]
    fn repairs_a_link_after_the_bundle_is_moved() {
        let mut fixture = Fixture::new(true);
        fixture.context.install().unwrap();
        let relocated = fixture.root.join("relocated app");
        fs::rename(fixture.context.executable.parent().unwrap(), &relocated).unwrap();
        fixture.context.executable = relocated.join(APP_BINARY);
        assert_eq!(fixture.context.status().unwrap().state, "broken");
        assert_eq!(fixture.context.install().unwrap().state, "available");
    }
    #[test]
    fn copied_appimage_launcher_records_a_persistent_target_and_repairs_it() {
        let mut fixture = Fixture::new(true);
        let image = fixture.root.join("Augur Git.AppImage");
        fs::copy(&fixture.context.executable, &image).unwrap();
        fixture.context.appimage = Some(image.clone());
        let status = fixture.context.install().unwrap();
        assert!(fs::symlink_metadata(&status.path).unwrap().is_file());
        let record = read_receipt(&fixture.context.config).unwrap().unwrap();
        assert_eq!(record.target, image);
        assert!(record.digest.is_some());
        assert_eq!(
            launch_target_in(&status.path, &fixture.context.config).unwrap(),
            image
        );
        let moved = fixture.root.join("updated.AppImage");
        fs::rename(image, &moved).unwrap();
        assert!(launch_target_in(&status.path, &fixture.context.config).is_err());
        fixture.context.appimage = Some(moved);
        assert_eq!(fixture.context.status().unwrap().state, "broken");
        assert_eq!(fixture.context.install().unwrap().state, "available");
        assert_eq!(fixture.context.uninstall().unwrap().state, "not-installed");
    }
    #[test]
    fn missing_path_reports_shell_instructions() {
        let fixture = Fixture::new(false);
        let status = fixture.context.install().unwrap();
        assert_eq!(status.state, "not-on-path");
        assert!(status.path_command.unwrap().contains("~/.zshrc"));
    }
    #[test]
    fn externally_installed_launcher_is_not_modified() {
        let mut fixture = Fixture::new(true);
        let external = fixture.root.join("package bin");
        fs::create_dir(&external).unwrap();
        symlink(fixture.context.source(), external.join("agit")).unwrap();
        fixture.context.search_path = external.into_os_string();
        let status = fixture.context.install().unwrap();
        assert!(status.package_managed);
        assert!(fixture.context.uninstall().is_err());
        assert!(status.path.exists());
    }
    #[test]
    fn path_shadowing_is_reported_without_overwriting_either_command() {
        let mut fixture = Fixture::new(true);
        let path = fixture.context.install().unwrap().path;
        let external = fixture.root.join("foreign bin");
        fs::create_dir(&external).unwrap();
        fs::copy(fixture.context.source(), external.join("agit")).unwrap();
        fixture.context.search_path =
            std::env::join_paths([external, path.parent().unwrap().to_path_buf()]).unwrap();
        assert_eq!(fixture.context.status().unwrap().state, "conflict");
        assert!(fixture.context.install().is_err());
        assert!(path.exists());
    }
}
