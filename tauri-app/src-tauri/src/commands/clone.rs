//! Native repository cloning with a second, recoverable Git LFS hydration stage.

use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use augur_core::config::LocationConfig;
use augur_core::git::GitRepo;
use augur_core::lfs::{self, LfsTransferProgress};

use crate::commands::repo::CommandError;
use crate::events::LFS_CLONE_PROGRESS_EVENT;
use crate::state::AppState;

const MAX_OUTPUT_BYTES: usize = 256 * 1024;
static TASKS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneProgressEvent {
    pub operation_id: String,
    pub stage: String,
    pub message: String,
    pub progress: Option<LfsTransferProgress>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneResult {
    pub repo_id: u64,
    pub path: String,
    pub uses_lfs: bool,
    pub lfs_available: bool,
    pub lfs_error: Option<String>,
}

#[tauri::command]
pub async fn lfs_clone(
    app: AppHandle,
    state: State<'_, AppState>,
    remote: String,
    parent: String,
    directory_name: String,
    download_lfs: bool,
    operation_id: String,
) -> Result<CloneResult, CommandError> {
    validate_operation_id(&operation_id)?;
    let target = validate_clone_target(&remote, &parent, &directory_name)?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let tasks = TASKS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut tasks = tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if tasks.contains_key(&operation_id) {
            return Err(CommandError::new(
                "err-lfs-clone",
                "A clone task with this operation id is already active.",
            ));
        }
        tasks.insert(operation_id.clone(), cancel.clone());
    }

    let event_app = app.clone();
    let task_operation_id = operation_id.clone();
    let task_remote = remote.trim().to_string();
    let result = tauri::async_runtime::spawn_blocking(move || {
        clone_repository(
            &event_app,
            &task_operation_id,
            &task_remote,
            &target,
            download_lfs,
            &cancel,
        )
    })
    .await
    .map_err(|error| CommandError::new("err-lfs-clone", error.to_string()))?;

    if let Some(tasks) = TASKS.get() {
        tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&operation_id);
    }
    let cloned = result.map_err(|detail| CommandError::new("err-lfs-clone", detail))?;
    let path = cloned.path.to_string_lossy().into_owned();
    let location = LocationConfig::Local;
    let repo_id = state
        .open_repository(&app, path.clone(), location)
        .map_err(|error| CommandError {
            key: error.key.into(),
            detail: error.detail,
        })?;
    Ok(CloneResult {
        repo_id,
        path,
        uses_lfs: cloned.uses_lfs,
        lfs_available: cloned.lfs_available,
        lfs_error: cloned.lfs_error,
    })
}

#[tauri::command]
pub fn cancel_lfs_clone(operation_id: String) -> bool {
    let Some(tasks) = TASKS.get() else {
        return false;
    };
    let tasks = tasks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cancel) = tasks.get(&operation_id) {
        cancel.store(true, Ordering::Release);
        true
    } else {
        false
    }
}

struct CloneOutcome {
    path: PathBuf,
    uses_lfs: bool,
    lfs_available: bool,
    lfs_error: Option<String>,
}

fn clone_repository(
    app: &AppHandle,
    operation_id: &str,
    remote: &str,
    target: &Path,
    download_lfs: bool,
    cancel: &AtomicBool,
) -> Result<CloneOutcome, String> {
    emit_progress(app, operation_id, "git", "Cloning Git repository…", None);
    let target_text = target.to_string_lossy().into_owned();
    run_git_clone(app, operation_id, remote, &target_text, cancel)?;
    if cancel.load(Ordering::Acquire) {
        return Err(
            "Clone cancelled. The partial directory was kept at the selected destination.".into(),
        );
    }

    let repo = GitRepo::local(target_text);
    let has_head = repo
        .command()
        .arg("-C")
        .arg(repo.path())
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_head {
        emit_progress(
            app,
            operation_id,
            "complete",
            "The remote repository is empty.",
            None,
        );
        return Ok(CloneOutcome {
            path: target.to_path_buf(),
            uses_lfs: false,
            lfs_available: lfs::probe(&repo).installed,
            lfs_error: None,
        });
    }

    let environment = lfs::probe(&repo);
    let mut lfs_error = None;
    let initialized = if environment.installed {
        match lfs::initialize(&repo) {
            Ok(state) => state.initialized,
            Err(error) => {
                lfs_error = Some(error);
                false
            }
        }
    } else {
        false
    };

    emit_progress(
        app,
        operation_id,
        "checkout",
        "Checking out the default branch…",
        None,
    );
    if let Err(error) = checkout_pointers(&repo, initialized) {
        lfs_error.get_or_insert_with(|| {
            format!("Git clone completed, but initial checkout failed: {error}")
        });
        return Ok(CloneOutcome {
            path: target.to_path_buf(),
            uses_lfs: true,
            lfs_available: environment.installed,
            lfs_error,
        });
    }

    let state = lfs::repository_state(&repo);
    if state.uses_lfs {
        if !environment.installed {
            lfs_error.get_or_insert_with(|| "Git LFS is not installed. The repository was cloned with pointer files; install Git LFS to download its content.".into());
        } else if download_lfs {
            if !initialized {
                lfs_error.get_or_insert_with(|| {
                    state
                        .detail
                        .clone()
                        .unwrap_or_else(|| "Git LFS filters could not be initialized.".into())
                });
            } else {
                let expected_head = state.head.clone();
                emit_progress(
                    app,
                    operation_id,
                    "lfs-fetch",
                    "Downloading Git LFS objects…",
                    None,
                );
                let lock = lfs::operation_lock(&repo);
                let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Err(error) = lfs::fetch_cancellable(&repo, cancel, |progress| {
                    emit_progress(
                        app,
                        operation_id,
                        "lfs-fetch",
                        "Transferring LFS objects…",
                        Some(progress),
                    );
                }) {
                    lfs_error.get_or_insert(error);
                } else if lfs::repository_state(&repo).head != expected_head {
                    lfs_error.get_or_insert_with(|| "The checked out commit changed during download. No files were populated; retry for the current branch.".into());
                } else if let Err(error) = lfs::checkout(&repo) {
                    lfs_error.get_or_insert(error);
                }
            }
        }
    }

    if !download_lfs {
        lfs::skip_open_download_once(&repo);
    }
    if let Some(error) = lfs_error.as_deref() {
        emit_progress(app, operation_id, "partial", error, None);
    } else {
        emit_progress(
            app,
            operation_id,
            "complete",
            "Repository clone completed.",
            None,
        );
    }
    Ok(CloneOutcome {
        path: target.to_path_buf(),
        uses_lfs: state.uses_lfs,
        lfs_available: environment.installed,
        lfs_error,
    })
}

fn checkout_pointers(repo: &GitRepo, lfs_initialized: bool) -> Result<(), String> {
    let mut command = repo.command();
    if !lfs_initialized {
        command.args([
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.smudge=",
            "-c",
            "filter.lfs.required=false",
        ]);
    }
    let output = command
        .arg("-C")
        .arg(repo.path())
        .args(["checkout", "--force", "HEAD"])
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(output_detail(&output))
    }
}

fn run_git_clone(
    app: &AppHandle,
    operation_id: &str,
    remote: &str,
    target: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let repo = GitRepo::local(target);
    let mut command = repo.command();
    command
        .args(["clone", "--progress", "--no-checkout", "--", remote, target])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    augur_core::git::configure_process_tree(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start Git clone: {error}"))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = thread::spawn(move || read_capped(stdout));
    let (line_tx, line_rx) = mpsc::channel();
    let stderr_reader = thread::spawn(move || read_lines_capped(stderr, line_tx));
    let status = loop {
        for line in line_rx.try_iter() {
            emit_progress(app, operation_id, "git", &line, None);
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if cancel.load(Ordering::Acquire) {
            augur_core::git::terminate_process_tree(&mut child);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(
                "Clone cancelled. The partial directory was kept at the selected destination."
                    .into(),
            );
        }
        thread::sleep(Duration::from_millis(60));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Could not read Git clone output".to_string())??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Could not read Git clone output".to_string())??;
    let output = Output {
        status,
        stdout,
        stderr,
    };
    if output.status.success() {
        Ok(())
    } else {
        Err(output_detail(&output))
    }
}

fn read_capped(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(output)
}

fn read_lines_capped(
    mut reader: impl Read,
    sender: mpsc::Sender<String>,
) -> Result<Vec<u8>, String> {
    let mut reader = std::io::BufReader::new(&mut reader);
    let mut output = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        if output.len() < MAX_OUTPUT_BYTES {
            let remaining = MAX_OUTPUT_BYTES - output.len();
            output.extend_from_slice(&line[..line.len().min(remaining)]);
        }
        let text = String::from_utf8_lossy(&line).trim().to_string();
        if !text.is_empty() {
            let _ = sender.send(text);
        }
    }
    Ok(output)
}

fn output_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let text = if stderr.is_empty() { stdout } else { stderr };
    if text.is_empty() {
        format!("Git exited with status {:?}", output.status.code())
    } else {
        text
    }
}

fn emit_progress(
    app: &AppHandle,
    operation_id: &str,
    stage: &str,
    message: &str,
    progress: Option<LfsTransferProgress>,
) {
    let _ = app.emit(
        LFS_CLONE_PROGRESS_EVENT,
        CloneProgressEvent {
            operation_id: operation_id.to_string(),
            stage: stage.to_string(),
            message: message.to_string(),
            progress,
        },
    );
}

fn validate_operation_id(operation_id: &str) -> Result<(), CommandError> {
    if operation_id.is_empty()
        || operation_id.len() > 128
        || operation_id.chars().any(char::is_control)
    {
        Err(CommandError::new(
            "err-lfs-clone",
            "Invalid clone operation id.",
        ))
    } else {
        Ok(())
    }
}

fn validate_clone_target(remote: &str, parent: &str, name: &str) -> Result<PathBuf, CommandError> {
    let remote = remote.trim();
    if remote.is_empty()
        || remote.starts_with('-')
        || remote.chars().any(char::is_control)
        || (remote.starts_with("https://")
            && remote.split("//").nth(1).is_some_and(|authority| {
                authority
                    .split('/')
                    .next()
                    .is_some_and(|host| host.contains('@'))
            }))
    {
        return Err(CommandError::new(
            "err-lfs-clone",
            "Enter a valid remote URL without embedded HTTPS credentials.",
        ));
    }
    let looks_like_remote = ["https://", "http://", "ssh://", "git://", "file://", "git@"]
        .iter()
        .any(|prefix| remote.starts_with(prefix));
    if !looks_like_remote && !Path::new(remote).is_absolute() {
        return Err(CommandError::new(
            "err-lfs-clone",
            "Use an HTTPS, SSH, Git, or file URL.",
        ));
    }
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || Path::new(name)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CommandError::new(
            "err-lfs-clone",
            "Choose a new directory name without path separators.",
        ));
    }
    let parent = Path::new(parent)
        .canonicalize()
        .map_err(|error| CommandError::new("err-lfs-clone", error.to_string()))?;
    if !parent.is_dir() {
        return Err(CommandError::new(
            "err-lfs-clone",
            "The selected parent path is not a directory.",
        ));
    }
    let target = parent.join(name);
    if target.exists() {
        return Err(CommandError::new(
            "err-lfs-clone",
            "The clone target must be a new directory.",
        ));
    }
    Ok(target)
}
