//! User-level Windows PATH registration shared by NSIS and Settings.

use super::{ShellDetection, Status, WindowsPathStatus};
use crate::build_info::APP_BINARY;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS, HANDLE, WAIT_ABANDONED,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};

const APP_REGISTRY_KEY: &str = "Software\\com.augur.git.tauri\\Cli";
const ENVIRONMENT_REGISTRY_KEY: &str = "Environment";
const STARTUP_MUTEX: &str = "com.augur.git.tauri.startup";
const REG_ENABLED: &str = "Enabled";
const REG_INSTALL_DIR: &str = "InstallDir";
const REG_PATH_ADDED: &str = "PathAdded";
const REG_PATH_PRESENT: &str = "PathPresent";
const REG_PATH_TYPE: &str = "PathType";
const REG_WAIT_MILLIS: u32 = 15_000;

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

fn registry_error(code: u32) -> String {
    std::io::Error::from_raw_os_error(code as i32).to_string()
}

struct Key(HKEY);

impl Key {
    fn create(path: &str) -> Result<Self, String> {
        let mut key = std::ptr::null_mut();
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if result != ERROR_SUCCESS {
            return Err(registry_error(result));
        }
        Ok(Self(key))
    }

    fn optional(path: &str, access: u32) -> Result<Option<Self>, String> {
        let mut key = std::ptr::null_mut();
        let result =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, access, &mut key) };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err(registry_error(result));
        }
        Ok(Some(Self(key)))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

struct NamedMutex(HANDLE);

impl NamedMutex {
    fn acquire(name: &str, timeout_ms: u32) -> Result<Self, String> {
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide(name).as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let result = unsafe { WaitForSingleObject(handle, timeout_ms) };
        if result == WAIT_OBJECT_0 || result == WAIT_ABANDONED {
            return Ok(Self(handle));
        }
        unsafe { CloseHandle(handle) };
        if result == WAIT_TIMEOUT {
            Err(format!("timed out waiting for {name}"))
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

/// Hold the startup lock until the application has built its single-instance
/// window target. This closes the race where a second launch sees the mutex
/// before the first process can receive the forwarded paths.
pub struct StartupGate {
    _guard: NamedMutex,
}

pub fn acquire_startup_gate() -> Result<StartupGate, String> {
    NamedMutex::acquire(STARTUP_MUTEX, REG_WAIT_MILLIS).map(|guard| StartupGate { _guard: guard })
}

#[derive(Clone, Debug)]
struct PathValue {
    value: OsString,
    kind: u32,
}

fn read_raw_value(key: HKEY, name: &str) -> Result<Option<(u32, Vec<u8>)>, String> {
    let name = wide(name);
    let mut kind = 0;
    let mut size = 0;
    let first = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if first == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if first != ERROR_SUCCESS && first != ERROR_MORE_DATA {
        return Err(registry_error(first));
    }
    let mut bytes = vec![0u8; size as usize];
    let second = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            bytes.as_mut_ptr(),
            &mut size,
        )
    };
    if second != ERROR_SUCCESS {
        return Err(registry_error(second));
    }
    bytes.truncate(size as usize);
    Ok(Some((kind, bytes)))
}

fn read_string_value(key: HKEY, name: &str) -> Result<Option<(u32, Vec<u16>)>, String> {
    let Some((kind, bytes)) = read_raw_value(key, name)? else {
        return Ok(None);
    };
    if bytes.len() % 2 != 0 {
        return Err(format!(
            "registry value {name:?} has an invalid UTF-16 length"
        ));
    }
    let words = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|word| *word != 0)
        .collect();
    Ok(Some((kind, words)))
}

fn read_path_value() -> Result<Option<PathValue>, String> {
    let Some(environment) = Key::optional(ENVIRONMENT_REGISTRY_KEY, KEY_READ)? else {
        return Ok(None);
    };
    let Some((kind, value)) = read_string_value(environment.0, "Path")? else {
        return Ok(None);
    };
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        return Err("HKCU\\Environment\\Path has an unsupported registry type".into());
    }
    Ok(Some(PathValue {
        value: OsString::from_wide(&value),
        kind,
    }))
}

fn write_path_value(value: Option<&OsStr>, kind: u32) -> Result<(), String> {
    let environment = Key::create(ENVIRONMENT_REGISTRY_KEY)?;
    let name = wide("Path");
    if let Some(value) = value {
        let encoded = value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let bytes = unsafe {
            std::slice::from_raw_parts(
                encoded.as_ptr().cast::<u8>(),
                std::mem::size_of_val(encoded.as_slice()),
            )
        };
        let result = unsafe {
            RegSetValueExW(
                environment.0,
                name.as_ptr(),
                0,
                kind,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(registry_error(result));
        }
    } else {
        let result = unsafe { RegDeleteValueW(environment.0, name.as_ptr()) };
        if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
            return Err(registry_error(result));
        }
    }
    notify_environment_change();
    Ok(())
}

fn notify_environment_change() {
    let mut result = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            wide("Environment").as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1000,
            &mut result,
        );
    }
}

fn read_dword(key: HKEY, name: &str) -> Result<Option<u32>, String> {
    let Some((kind, value)) = read_raw_value(key, name)? else {
        return Ok(None);
    };
    if kind != windows_sys::Win32::System::Registry::REG_DWORD || value.len() != 4 {
        return Err(format!("CLI registration value {name} is invalid"));
    }
    Ok(Some(u32::from_le_bytes(value.try_into().unwrap())))
}

fn write_dword(key: HKEY, name: &str, value: u32) -> Result<(), String> {
    let name = wide(name);
    let bytes = value.to_le_bytes();
    let result = unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            windows_sys::Win32::System::Registry::REG_DWORD,
            bytes.as_ptr(),
            bytes.len() as u32,
        )
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(registry_error(result))
    }
}

fn write_string(key: HKEY, name: &str, value: &OsStr) -> Result<(), String> {
    let name = wide(name);
    let encoded = value.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let bytes = unsafe {
        std::slice::from_raw_parts(
            encoded.as_ptr().cast::<u8>(),
            std::mem::size_of_val(encoded.as_slice()),
        )
    };
    let result = unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            bytes.as_ptr(),
            bytes.len() as u32,
        )
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(registry_error(result))
    }
}

fn write_metadata(
    app_dir: &Path,
    enabled: bool,
    path_added: bool,
    path_present: bool,
    path_type: u32,
) -> Result<(), String> {
    let key = Key::create(APP_REGISTRY_KEY)?;
    write_dword(key.0, REG_ENABLED, u32::from(enabled))?;
    write_dword(key.0, REG_PATH_ADDED, u32::from(path_added))?;
    write_dword(key.0, REG_PATH_PRESENT, u32::from(path_present))?;
    write_dword(key.0, REG_PATH_TYPE, path_type)?;
    write_string(key.0, REG_INSTALL_DIR, app_dir.as_os_str())
}

fn read_metadata() -> Result<Option<(bool, bool, bool, u32, PathBuf)>, String> {
    let Some(key) = Key::optional(APP_REGISTRY_KEY, KEY_READ)? else {
        return Ok(None);
    };
    let enabled = read_dword(key.0, REG_ENABLED)?.unwrap_or_default() != 0;
    let path_added = read_dword(key.0, REG_PATH_ADDED)?.unwrap_or_default() != 0;
    let path_present = read_dword(key.0, REG_PATH_PRESENT)?.unwrap_or_default() != 0;
    let path_type = read_dword(key.0, REG_PATH_TYPE)?.unwrap_or(REG_SZ);
    let install_dir = read_string_value(key.0, REG_INSTALL_DIR)?
        .filter(|(kind, _)| *kind == REG_SZ)
        .map(|(_, value)| PathBuf::from(OsString::from_wide(&value)))
        .ok_or("CLI registration is missing its installation directory")?;
    Ok(Some((
        enabled,
        path_added,
        path_present,
        path_type,
        install_dir,
    )))
}

fn delete_metadata() -> Result<(), String> {
    let Some(key) = Key::optional(APP_REGISTRY_KEY, KEY_WRITE)? else {
        return Ok(());
    };
    for name in [
        REG_ENABLED,
        REG_INSTALL_DIR,
        REG_PATH_ADDED,
        REG_PATH_PRESENT,
        REG_PATH_TYPE,
    ] {
        let result = unsafe { RegDeleteValueW(key.0, wide(name).as_ptr()) };
        if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
            return Err(registry_error(result));
        }
    }
    drop(key);

    let result = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(APP_REGISTRY_KEY).as_ptr()) };
    if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
        return Err(registry_error(result));
    }
    Ok(())
}

fn user_path() -> Result<Option<PathValue>, String> {
    read_path_value()
}

fn expand(path: &OsStr) -> PathBuf {
    let value = path.to_string_lossy();
    let mut expanded = value.to_string();
    let mut index = 0;
    while let Some(start) = expanded[index..].find('%') {
        let start = index + start;
        let Some(end_offset) = expanded[start + 1..].find('%') else {
            break;
        };
        let end = start + 1 + end_offset;
        let name = &expanded[start + 1..end];
        if let Some(value) = std::env::var_os(name) {
            let value = value.to_string_lossy();
            expanded.replace_range(start..=end, &value);
            index = start + value.len();
        } else {
            index = end + 1;
        }
    }
    PathBuf::from(expanded)
}

fn normalized(path: &Path) -> String {
    let plain = crate::paths::normalize_extended_path(path);
    let value = plain.to_string_lossy();
    let value = value.trim().trim_matches('"').replace('/', "\\");
    let value = value.trim_end_matches('\\');
    value.to_lowercase()
}

fn contains_path(value: &OsStr, target: &Path) -> bool {
    if value.is_empty() {
        return false;
    }
    std::env::split_paths(value)
        .any(|part| normalized(&expand(part.as_os_str())) == normalized(target))
}

fn first_command(value: &OsStr) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    let expanded = expand(value);
    for directory in std::env::split_paths(&expanded) {
        for extension in ["exe", "com", "bat", "cmd"] {
            let candidate = directory.join(format!("agit.{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn command_state(
    candidate: Option<PathBuf>,
    expected: Option<&Path>,
) -> (Option<PathBuf>, &'static str) {
    let state = match candidate.as_ref() {
        None => "missing",
        Some(candidate)
            if expected.is_some_and(|expected| normalized(candidate) == normalized(expected)) =>
        {
            "available"
        }
        Some(_) => "conflict",
    };
    (candidate, state)
}

fn with_path(value: &OsStr, target: &Path, add: bool) -> Result<OsString, String> {
    let parts = if value.is_empty() {
        Vec::new()
    } else {
        std::env::split_paths(value).collect::<Vec<_>>()
    };
    let target_norm = normalized(target);
    if add {
        if parts
            .iter()
            .any(|part| normalized(&expand(part.as_os_str())) == target_norm)
        {
            return Ok(value.to_os_string());
        }
        let mut entries = parts;
        entries.push(target.to_path_buf());
        std::env::join_paths(entries).map_err(|error| error.to_string())
    } else {
        let mut removed = false;
        let entries = parts
            .into_iter()
            .filter(|part| {
                if !removed && normalized(&expand(part.as_os_str())) == target_norm {
                    removed = true;
                    false
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        std::env::join_paths(entries).map_err(|error| error.to_string())
    }
}

fn process_command() -> (Option<PathBuf>, &'static str) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let expected = std::env::current_exe()
        .ok()
        .and_then(|exe| executable_path(&exe).ok());
    command_state(first_command(&path), expected.as_deref())
}

fn application_dir(executable: &Path) -> Result<PathBuf, String> {
    let parent = executable
        .parent()
        .ok_or("Could not determine the agit installation directory")?;
    let directory = if parent
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("bin"))
    {
        parent
            .parent()
            .map(Path::to_path_buf)
            .ok_or("Could not determine the agit installation directory")?
    } else {
        parent.to_path_buf()
    };
    Ok(crate::paths::normalize_extended_path(&directory))
}

fn executable_path(executable: &Path) -> Result<PathBuf, String> {
    let app_dir = application_dir(executable)?;
    let installed = app_dir.join("bin").join("agit.exe");
    if normalized(executable).ends_with("\\bin\\agit.exe") {
        return Ok(crate::paths::normalize_extended_path(executable));
    }
    if installed.is_file() {
        Ok(installed)
    } else {
        Ok(executable.to_path_buf())
    }
}

fn gui_path(executable: &Path) -> Result<PathBuf, String> {
    let app_dir = application_dir(executable)?;
    let target = app_dir.join(format!("{APP_BINARY}.exe"));
    if target.is_file() {
        Ok(target)
    } else {
        Err(format!(
            "could not locate Augur Git at {}",
            target.display()
        ))
    }
}

fn apply_registration(app_dir: &Path, enabled: bool, purge: bool) -> Result<(), String> {
    let _lock = NamedMutex::acquire("com.augur.git.tauri.cli-install", REG_WAIT_MILLIS)?;
    let bin = app_dir.join("bin");
    let launcher = bin.join("agit.exe");
    let old_metadata = read_metadata()?;
    let mut current_path = user_path()?;

    if enabled {
        if !launcher.is_file() {
            return Err(format!(
                "the bundled CLI is missing: {}",
                launcher.display()
            ));
        }
        let process_hit = command_state(
            first_command(&std::env::var_os("PATH").unwrap_or_default()),
            Some(&launcher),
        );
        let registry_hit = current_path
            .as_ref()
            .and_then(|value| first_command(&value.value))
            .map(|candidate| command_state(Some(candidate), Some(&launcher)));
        if process_hit.1 == "conflict"
            || registry_hit
                .as_ref()
                .is_some_and(|(_, state)| *state == "conflict")
        {
            return Err(format!(
                "another agit command is already first on PATH: {}",
                process_hit
                    .0
                    .or_else(|| registry_hit.and_then(|(candidate, _)| candidate))
                    .unwrap()
                    .display()
            ));
        }
        let path_present = old_metadata
            .as_ref()
            .map_or(current_path.is_some(), |(_, _, present, _, _)| *present);
        let path_type = old_metadata.as_ref().map_or(
            current_path.as_ref().map_or(REG_SZ, |value| value.kind),
            |(_, _, _, kind, _)| *kind,
        );
        let old_dir = old_metadata.as_ref().map(|(_, _, _, _, dir)| dir.as_path());
        let old_added = old_metadata
            .as_ref()
            .is_some_and(|(_, added, _, _, _)| *added);
        if old_added && old_dir.is_some_and(|old| normalized(old) != normalized(&app_dir)) {
            let old_bin = old_dir.unwrap().join("bin");
            let base = current_path
                .as_ref()
                .map(|value| value.value.clone())
                .unwrap_or_default();
            let updated = with_path(&base, &old_bin, false)?;
            if updated != base {
                write_path_value(
                    Some(&updated),
                    current_path.as_ref().map_or(REG_SZ, |v| v.kind),
                )?;
                current_path = Some(PathValue {
                    value: updated,
                    kind: current_path.as_ref().map_or(REG_SZ, |v| v.kind),
                });
            }
        }
        let base = current_path
            .as_ref()
            .map(|value| value.value.clone())
            .unwrap_or_default();
        let was_present = contains_path(&base, &bin);
        let updated = with_path(&base, &bin, true)?;
        if updated != base {
            let kind = current_path.as_ref().map_or(REG_SZ, |value| value.kind);
            write_path_value(Some(&updated), kind)?;
        }
        write_metadata(
            &app_dir,
            true,
            old_added || !was_present,
            path_present,
            path_type,
        )
    } else {
        if let Some((_, was_added, path_present, path_type, previous_dir)) = old_metadata {
            if was_added {
                let bin = previous_dir.join("bin");
                let base = current_path
                    .as_ref()
                    .map(|value| value.value.clone())
                    .unwrap_or_default();
                let updated = with_path(&base, &bin, false)?;
                if updated != base {
                    if updated.is_empty() && !path_present {
                        write_path_value(None, path_type)?;
                    } else {
                        let kind = current_path.as_ref().map_or(path_type, |value| value.kind);
                        write_path_value(Some(&updated), kind)?;
                    }
                }
            }
            if purge {
                delete_metadata()?;
                return Ok(());
            }
            write_metadata(&app_dir, false, false, path_present, path_type)
        } else if purge {
            delete_metadata()
        } else {
            write_metadata(
                &app_dir,
                false,
                false,
                current_path.is_some(),
                current_path.as_ref().map_or(REG_SZ, |value| value.kind),
            )
        }
    }
}

#[derive(Clone)]
pub struct Context {
    executable: PathBuf,
    app_dir: PathBuf,
}

impl Context {
    pub fn from_env() -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let app_dir = application_dir(&executable)?;
        Ok(Self {
            executable,
            app_dir,
        })
    }

    pub fn status(&self) -> Result<Status, String> {
        let launcher = self.app_dir.join("bin").join("agit.exe");
        let metadata = read_metadata()?;
        let path = user_path()?;
        let desired = metadata
            .as_ref()
            .is_some_and(|(enabled, _, _, _, _)| *enabled);
        let path_added = metadata.as_ref().is_some_and(|(_, added, _, _, _)| *added);
        let has_path_entry = path
            .as_ref()
            .is_some_and(|value| contains_path(&value.value, &self.app_dir.join("bin")));
        let (application_path, process_state) = process_command();
        let registry_conflict = path
            .as_ref()
            .and_then(|value| first_command(&value.value))
            .filter(|candidate| normalized(candidate) != normalized(&launcher));
        let registered = desired && has_path_entry;
        let state = if !launcher.is_file() {
            "broken"
        } else if process_state == "conflict" || registry_conflict.is_some() {
            "conflict"
        } else if registered && process_state == "available" {
            "available"
        } else if registered {
            "not-on-path"
        } else if desired || path_added {
            "broken"
        } else {
            "not-installed"
        };
        let detail = if !launcher.is_file() {
            "The bundled CLI is missing. Re-run the Augur Git installer.".into()
        } else if process_state == "conflict" {
            format!(
                "Another agit command is first on the application's PATH: {}",
                application_path.as_ref().unwrap().display()
            )
        } else if let Some(path) = registry_conflict.as_ref() {
            format!(
                "Another agit command is first on the registered user PATH: {}",
                path.display()
            )
        } else {
            String::new()
        };
        let can_install = launcher.is_file()
            && process_state != "conflict"
            && registry_conflict.is_none()
            && !registered;
        let can_remove = desired || path_added;
        Ok(Status {
            state,
            path: launcher,
            application_path: application_path.clone(),
            shell: ShellDetection::default(),
            can_install,
            can_remove,
            package_managed: false,
            path_command: None,
            detail,
            windows_path: Some(WindowsPathStatus {
                registered,
                process_state,
                resolved_path: application_path,
            }),
        })
    }

    pub fn install(&self) -> Result<Status, String> {
        apply_registration(&self.app_dir, true, false)?;
        self.status()
    }

    pub fn uninstall(&self) -> Result<Status, String> {
        apply_registration(&self.app_dir, false, false)?;
        self.status()
    }

    pub fn uninstall_package(&self) -> Result<(), String> {
        apply_registration(&self.app_dir, false, true)
    }

    pub fn launch_target(&self) -> Result<PathBuf, String> {
        gui_path(&self.executable)
    }
}

pub fn install_from_launcher() -> Result<(), String> {
    Context::from_env()?.install().map(|_| ())
}

pub fn uninstall_from_launcher() -> Result<(), String> {
    Context::from_env()?.uninstall().map(|_| ())
}

pub fn uninstall_package_from_launcher() -> Result<(), String> {
    Context::from_env()?.uninstall_package()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_registration_is_case_insensitive_and_idempotent() {
        let target = Path::new(r"C:\Users\Test\Augur Git\bin");
        let original = OsString::from(r"C:\Windows\System32;C:\Users\Test\Augur Git\bin\");
        assert!(contains_path(&original, target));
        assert_eq!(with_path(&original, target, true).unwrap(), original);
    }

    #[test]
    fn path_removal_preserves_unrelated_entries() {
        let target = Path::new(r"C:\Users\Test\Augur Git\bin");
        let original = OsString::from(r"C:\Windows\System32;C:\Users\Test\Augur Git\bin;C:\Tools");
        assert_eq!(
            with_path(&original, target, false).unwrap(),
            OsString::from(r"C:\Windows\System32;C:\Tools")
        );
    }

    #[test]
    fn path_removal_keeps_user_added_duplicates() {
        let target = Path::new(r"C:\Users\Test\Augur Git\bin");
        let original = OsString::from(
            r"C:\Windows\System32;C:\Users\Test\Augur Git\bin;C:\Users\Test\Augur Git\bin",
        );
        assert_eq!(
            with_path(&original, target, false).unwrap(),
            OsString::from(r"C:\Windows\System32;C:\Users\Test\Augur Git\bin")
        );
    }

    #[test]
    fn registering_with_an_empty_path_does_not_add_a_current_directory_entry() {
        let target = Path::new(r"C:\Users\Test\Augur Git\bin");
        let registered = with_path(OsStr::new(""), target, true).unwrap();
        assert_eq!(registered, target.as_os_str());
        assert!(!contains_path(OsStr::new(""), target));
        assert_eq!(
            with_path(&registered, target, false).unwrap(),
            OsStr::new("")
        );
    }

    #[test]
    fn user_path_expansion_is_used_for_comparison() {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            let expected = PathBuf::from(profile).join("Augur Git Tauri").join("bin");
            let original = OsString::from(r"%USERPROFILE%\Augur Git Tauri\bin");
            assert!(contains_path(&original, &expected));
        }
    }
}
