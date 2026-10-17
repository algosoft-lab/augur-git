//! Git LFS discovery and repository-level operations.
//!
//! All arguments are fixed here and every path remains a separate argument.
//! Git LFS itself owns pointer conversion, transfer protocols, extensions,
//! credentials, and cache layout.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::git::GitRepo;

const MAX_DIAGNOSTIC_BYTES: usize = 256 * 1024;
const MAX_ATTRIBUTE_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
static NEXT_PROGRESS_FILE: AtomicU64 = AtomicU64::new(1);
static REPOSITORY_LOCKS: OnceLock<Mutex<std::collections::HashMap<String, Arc<Mutex<()>>>>> =
    OnceLock::new();
static DOWNLOAD_POLICY: OnceLock<RwLock<DownloadPolicy>> = OnceLock::new();
static SKIP_OPEN_DOWNLOAD: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Default)]
struct DownloadPolicy {
    automatic: bool,
    on_open: bool,
    repository_overrides: std::collections::BTreeMap<String, bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsEnvironment {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsRepositoryState {
    pub uses_lfs: bool,
    pub available: bool,
    pub initialized: bool,
    pub hook_ready: bool,
    pub hook_conflict: bool,
    pub head: Option<String>,
    pub endpoint: Option<String>,
    pub endpoint_source: Option<String>,
    pub storage: Option<String>,
    pub fetch_include: Option<String>,
    pub fetch_exclude: Option<String>,
    pub concurrent_transfers: Option<u8>,
    pub transfer_retries: Option<u8>,
    pub locks_verify: Option<String>,
    pub automatic_download: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsFile {
    pub path: String,
    pub oid: String,
    pub size: Option<u64>,
    pub present: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsLock {
    pub id: String,
    pub path: String,
    pub owner: String,
    pub locked_at: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsRepositoryPreferences {
    pub automatic_download: Option<bool>,
    pub fetch_include: Option<String>,
    pub fetch_exclude: Option<String>,
    pub concurrent_transfers: Option<u8>,
    pub transfer_retries: Option<u8>,
    pub locks_verify: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsCachePreview {
    pub objects: usize,
    pub bytes: u64,
    pub candidates: usize,
    pub candidate_bytes: u64,
    pub output: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsTransferProgress {
    pub direction: String,
    pub current_file: usize,
    pub total_files: usize,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub path: String,
}

fn command(repo: &GitRepo, args: &[&str]) -> Result<Output, String> {
    let output = repo
        .command()
        .arg("-C")
        .arg(repo.path())
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.stdout.len() + output.stderr.len() > MAX_DIAGNOSTIC_BYTES {
        return Err("Git LFS returned more than 256 KiB of diagnostic output".into());
    }
    Ok(output)
}

fn detail(output: &Output) -> String {
    let stderr = output_text(output, false);
    let stdout = output_text(output, true);
    let value = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    value.trim().to_string()
}

fn output_text(output: &Output, stdout: bool) -> String {
    let bytes = if stdout {
        &output.stdout
    } else {
        &output.stderr
    };
    if cfg!(windows) {
        crate::git::location::decode_wsl_output(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Return the process-wide mutation lock for a repository and its linked worktrees.
pub fn operation_lock(repo: &GitRepo) -> Arc<Mutex<()>> {
    let common_dir = command(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .filter(|output| output.status.success())
    .map(|output| output_text(&output, true).trim().to_string())
    .filter(|path| !path.is_empty())
    .unwrap_or_else(|| repo.path().to_string());
    let key = format!(
        "{}:{}",
        repo.location().distro().unwrap_or("local"),
        common_dir
    );
    let locks = REPOSITORY_LOCKS.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Replace the in-memory policy snapshot after the settings document changes.
pub fn configure_download_policy(
    automatic: bool,
    on_open: bool,
    repository_overrides: std::collections::BTreeMap<String, bool>,
) {
    let policy = DOWNLOAD_POLICY.get_or_init(|| RwLock::new(DownloadPolicy::default()));
    *policy
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = DownloadPolicy {
        automatic,
        on_open,
        repository_overrides,
    };
}

pub fn repository_key(repo: &GitRepo) -> String {
    let mut path = repo.path().replace('\\', "/");
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    if cfg!(windows) {
        path.make_ascii_lowercase();
    }
    let location = repo
        .location()
        .distro()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "local".into());
    format!("{location}:{path}")
}

pub fn should_auto_download(repo: &GitRepo) -> bool {
    let Some(policy) = DOWNLOAD_POLICY.get() else {
        return true;
    };
    let policy = policy
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    policy
        .repository_overrides
        .get(&repository_key(repo))
        .copied()
        .unwrap_or(policy.automatic)
}

pub fn skip_open_download_once(repo: &GitRepo) {
    let skips = SKIP_OPEN_DOWNLOAD.get_or_init(|| Mutex::new(HashSet::new()));
    skips
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(repository_key(repo));
}

pub fn should_download_on_open(repo: &GitRepo) -> bool {
    if SKIP_OPEN_DOWNLOAD.get().is_some_and(|skips| {
        skips
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&repository_key(repo))
    }) {
        return false;
    }
    DOWNLOAD_POLICY.get().is_some_and(|policy| {
        let policy = policy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        policy.on_open
            && policy
                .repository_overrides
                .get(&repository_key(repo))
                .copied()
                .unwrap_or(policy.automatic)
    })
}

fn require_success(label: &str, output: Output) -> Result<Output, String> {
    if output.status.success() {
        Ok(output)
    } else {
        let detail = detail(&output);
        Err(if detail.is_empty() {
            format!("{label} failed with {:?}", output.status.code())
        } else {
            detail
        })
    }
}

pub fn run_transfer(
    repo: &GitRepo,
    args: &[&str],
    report: impl FnMut(LfsTransferProgress),
) -> Result<String, String> {
    let progress_path = std::env::temp_dir().join(format!(
        "augur-lfs-{}-{}.progress",
        std::process::id(),
        NEXT_PROGRESS_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&progress_path)
        .map_err(|error| format!("cannot create transfer progress file: {error}"))?;
    let result = run_transfer_inner(repo, args, &progress_path, None, report);
    let _ = std::fs::remove_file(progress_path);
    result
}

fn run_transfer_inner(
    repo: &GitRepo,
    args: &[&str],
    progress_path: &Path,
    cancel: Option<&AtomicBool>,
    mut report: impl FnMut(LfsTransferProgress),
) -> Result<String, String> {
    // WSLENV translates this Windows-side path for the distro process, while
    // keeping the host-visible path available for the polling loop.
    let progress_value = progress_path.to_string_lossy().into_owned();
    let mut command = repo.command();
    command
        .arg("-C")
        .arg(repo.path())
        .args(args)
        .env("GIT_LFS_PROGRESS", progress_value)
        .env("GIT_LFS_FORCE_PROGRESS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::git::configure_process_tree(&mut command);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = thread::spawn(move || read_capped(stdout));
    let stderr_reader = thread::spawn(move || read_capped(stderr));
    let mut offset = 0usize;
    let status = loop {
        let output = std::fs::read(progress_path).unwrap_or_default();
        if offset <= output.len() {
            for line in output[offset..]
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                if let Some(progress) = parse_progress(line) {
                    report(progress);
                }
            }
            offset = output.len();
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire)) {
            crate::git::terminate_process_tree(&mut child);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("Git LFS transfer cancelled".into());
        }
        thread::sleep(Duration::from_millis(60));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "failed to read Git LFS output".to_string())??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "failed to read Git LFS output".to_string())??;
    let output = Output {
        status,
        stdout,
        stderr,
    };
    require_success("Git LFS transfer", output).map(|output| detail(&output))
}

fn read_capped(mut reader: impl std::io::Read) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(output)
}

fn parse_progress(bytes: &[u8]) -> Option<LfsTransferProgress> {
    let line = std::str::from_utf8(bytes).ok()?.trim();
    let mut fields = line.splitn(5, ' ');
    let direction = fields.next()?.to_string();
    let current = fields.next()?.split_once('/')?;
    let transferred = fields.next()?.split_once('/')?;
    Some(LfsTransferProgress {
        direction,
        current_file: current.0.parse().ok()?,
        total_files: current.1.parse().ok()?,
        transferred_bytes: transferred.0.parse().ok()?,
        total_bytes: transferred.1.parse().ok()?,
        path: fields.next().unwrap_or_default().to_string(),
    })
}

pub fn probe(repo: &GitRepo) -> LfsEnvironment {
    let output = match command(repo, &["lfs", "version"]) {
        Ok(output) => output,
        Err(error) => {
            return LfsEnvironment {
                installed: false,
                version: None,
                path: None,
                detail: Some(error),
            };
        }
    };
    if !output.status.success() {
        return LfsEnvironment {
            installed: false,
            version: None,
            path: None,
            detail: Some(detail(&output)),
        };
    }
    let version = output_text(&output, true).trim().to_string();
    let path = repo
        .command_in_location(if cfg!(windows) && !repo.is_wsl() {
            "where.exe"
        } else {
            "which"
        })
        .arg("git-lfs")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let text = if repo.is_wsl() {
                crate::git::location::decode_wsl_output(&output.stdout)
            } else {
                String::from_utf8_lossy(&output.stdout).into_owned()
            };
            text.lines().next().map(str::to_string)
        });
    LfsEnvironment {
        installed: true,
        version: Some(version),
        path,
        detail: None,
    }
}

fn has_lfs_attributes(repo: &GitRepo) -> bool {
    let paths = match command(
        repo,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    ) {
        Ok(output) if output.status.success() => output_text(&output, true).into_bytes(),
        _ => return false,
    };
    if paths.is_empty() {
        return false;
    }
    let Some(output) = check_attr_for_paths(repo, &paths) else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let text = output_text(&output, true);
    let fields = text
        .split('\0')
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    fields
        .chunks_exact(3)
        .any(|fields| fields[1] == "filter" && fields[2] == "lfs")
}

/// Return the worktree paths whose effective filter attribute is `lfs`.
pub fn paths_with_lfs_attributes(repo: &GitRepo, paths: &[String]) -> HashSet<String> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let mut input = Vec::new();
    for path in paths {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
    }
    let Some(output) = check_attr_for_paths(repo, &input) else {
        return HashSet::new();
    };
    if !output.status.success() {
        return HashSet::new();
    }
    let text = output_text(&output, true);
    text.split('\0')
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>()
        .chunks_exact(3)
        .filter(|fields| fields[1] == "filter" && fields[2] == "lfs")
        .map(|fields| fields[0].to_string())
        .collect()
}

fn has_pointer_in_head(repo: &GitRepo) -> bool {
    let output = match command(
        repo,
        &[
            "grep",
            "-I",
            "-l",
            "-z",
            "--full-name",
            "-F",
            "version https://git-lfs.github.com/spec/v1",
            "HEAD",
            "--",
        ],
    ) {
        Ok(output) => output,
        // Git grep exits 1 when there are no matches.
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    // Source code and documentation may mention the pointer header. Verify
    // complete blobs, checking size first to avoid reading large candidates.
    output_text(&output, true)
        .split('\0')
        .filter(|object| !object.is_empty())
        .any(|object| {
            let Ok(size) = command(repo, &["cat-file", "-s", object]) else {
                return false;
            };
            if !size.status.success()
                || !output_text(&size, true)
                    .trim()
                    .parse::<usize>()
                    .is_ok_and(|size| size <= 1024)
            {
                return false;
            }
            command(repo, &["cat-file", "blob", object])
                .ok()
                .is_some_and(|blob| blob.status.success() && parse_pointer(&blob.stdout).is_some())
        })
}

fn effective_config(repo: &GitRepo, key: &str) -> Option<String> {
    let output = command(repo, &["config", "--get", key]).ok()?;
    output
        .status
        .success()
        .then(|| output_text(&output, true).trim().to_string())
}

fn effective_config_origin(repo: &GitRepo, key: &str) -> Option<String> {
    let output = command(repo, &["config", "--show-origin", "--get", key]).ok()?;
    if !output.status.success() {
        return None;
    }
    output_text(&output, true)
        .trim()
        .split_once('\t')
        .map(|(origin, _)| origin.to_string())
}

fn lfs_endpoint(repo: &GitRepo) -> (Option<String>, Option<String>) {
    if let Some(endpoint) = effective_config(repo, "lfs.url") {
        let source =
            effective_config_origin(repo, "lfs.url").unwrap_or_else(|| "Git configuration".into());
        return (Some(redact_endpoint(&endpoint)), Some(source));
    }
    if probe(repo).installed {
        if let Ok(output) = command(repo, &["lfs", "env"]) {
            if output.status.success() {
                if let Some(endpoint) = output_text(&output, true)
                    .lines()
                    .find_map(|line| line.strip_prefix("Endpoint="))
                {
                    let endpoint = endpoint.split(" (auth=").next().unwrap_or(endpoint);
                    let source = if Path::new(repo.path()).join(".lfsconfig").is_file() {
                        ".lfsconfig".to_string()
                    } else {
                        "Resolved from Git remotes".to_string()
                    };
                    return (Some(redact_endpoint(endpoint)), Some(source));
                }
            }
        }
    }
    (None, None)
}

fn redact_endpoint(endpoint: &str) -> String {
    let Some(scheme) = endpoint.find("://") else {
        return endpoint.to_string();
    };
    let authority_start = scheme + 3;
    let authority_end = endpoint[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(endpoint.len());
    let authority = &endpoint[authority_start..authority_end];
    authority
        .rfind('@')
        .map(|at| {
            format!(
                "{}{}***@{}{}",
                &endpoint[..authority_start],
                &authority[..at].split(':').next().unwrap_or(""),
                &authority[at + 1..],
                &endpoint[authority_end..]
            )
        })
        .unwrap_or_else(|| endpoint.to_string())
}

fn check_attr_for_paths(repo: &GitRepo, input: &[u8]) -> Option<Output> {
    let mut child = repo
        .command()
        .arg("-C")
        .arg(repo.path())
        .args(["check-attr", "-z", "filter", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 8192];
        let mut overflow = false;
        loop {
            let read = stdout.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            let remaining = MAX_ATTRIBUTE_OUTPUT_BYTES.saturating_sub(output.len());
            output.extend_from_slice(&buffer[..read.min(remaining)]);
            overflow |= read > remaining;
        }
        Some((output, overflow))
    });
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        return None;
    };
    if stdin.write_all(input).is_err() {
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        return None;
    }
    drop(stdin);
    let status = child.wait().ok()?;
    let (stdout, overflow) = reader.join().ok()??;
    (!overflow).then_some(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

fn hook_status(repo: &GitRepo) -> (bool, bool) {
    let hook = effective_config(repo, "core.hooksPath")
        .map(|directory| {
            if if repo.is_wsl() {
                directory.starts_with('/')
            } else {
                Path::new(&directory).is_absolute()
            } {
                directory
            } else {
                format!(
                    "{}/{}",
                    repo.path().trim_end_matches(['/', '\\']),
                    directory.trim_start_matches(['/', '\\'])
                )
            }
        })
        .map(|directory| format!("{}/pre-push", directory.trim_end_matches(['/', '\\'])))
        .or_else(|| {
            command(
                repo,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "hooks/pre-push",
                ],
            )
            .ok()
            .filter(|output| output.status.success())
            .map(|output| output_text(&output, true).trim().to_string())
        });
    let Some(hook) = hook else {
        return (false, false);
    };
    let contents = if repo.is_wsl() {
        repo.command_in_location("cat")
            .arg("--")
            .arg(&hook)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| output_text(&output, true))
    } else {
        std::fs::read_to_string(hook).ok()
    };
    let Some(contents) = contents else {
        return (false, false);
    };
    let ready = contents.contains("git-lfs") || contents.contains("git lfs pre-push");
    (ready, !ready && !contents.trim().is_empty())
}

pub fn repository_state(repo: &GitRepo) -> LfsRepositoryState {
    let uses_lfs = has_lfs_attributes(repo) || has_pointer_in_head(repo);
    let environment = probe(repo);
    if !uses_lfs {
        return LfsRepositoryState {
            available: environment.installed,
            automatic_download: should_auto_download(repo),
            ..LfsRepositoryState::default()
        };
    }
    let head = command(repo, &["rev-parse", "--verify", "HEAD"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output_text(&output, true).trim().to_string());
    if !environment.installed {
        return LfsRepositoryState {
            uses_lfs,
            detail: environment.detail,
            head,
            ..LfsRepositoryState::default()
        };
    }
    let (hook_ready, hook_conflict) = hook_status(repo);
    let filter_process = effective_config(repo, "filter.lfs.process");
    let filter_smudge = effective_config(repo, "filter.lfs.smudge");
    let filter_clean = effective_config(repo, "filter.lfs.clean");
    let initialized = filter_process
        .as_deref()
        .is_some_and(|v| v.contains("git-lfs"))
        && filter_smudge
            .as_deref()
            .is_some_and(|v| v.contains("git-lfs"))
        && filter_clean
            .as_deref()
            .is_some_and(|v| v.contains("git-lfs"));
    let (endpoint, endpoint_source) = lfs_endpoint(repo);
    let storage = effective_config(repo, "lfs.storage");
    let fetch_include = effective_config(repo, "lfs.fetchinclude");
    let fetch_exclude = effective_config(repo, "lfs.fetchexclude");
    let concurrent_transfers = effective_config(repo, "lfs.concurrenttransfers")
        .and_then(|value| value.parse::<u8>().ok());
    let transfer_retries = effective_config(repo, "lfs.transfer.maxretries")
        .and_then(|value| value.parse::<u8>().ok());
    let locks_verify = effective_config(repo, "lfs.locksverify");
    let detail = if !initialized {
        Some("Git LFS filters are not configured for this repository".into())
    } else if hook_conflict {
        Some(
            "A custom pre-push hook is present; preserve it and add the Git LFS pre-push command"
                .into(),
        )
    } else if !hook_ready {
        Some("The Git LFS pre-push hook is not installed".into())
    } else {
        None
    };
    LfsRepositoryState {
        uses_lfs,
        available: true,
        initialized,
        hook_ready,
        hook_conflict,
        head,
        endpoint,
        endpoint_source,
        storage,
        fetch_include,
        fetch_exclude,
        concurrent_transfers,
        transfer_retries,
        locks_verify,
        automatic_download: should_auto_download(repo),
        detail,
    }
}

/// Resolve the repository LFS state for an open flow, initializing the
/// repository-local setup once when Git LFS is available but the repository
/// was prepared outside the application. Plain Git clones without a global
/// `git lfs install` lack the filter configuration that status, worktree
/// operations, and downloads require. Initialization is the same local,
/// conflict-aware setup the clone flow applies, and a failed setup keeps the
/// unfiltered state so callers surface the original reason instead of a
/// secondary setup failure.
pub fn prepare_repository(repo: &GitRepo) -> LfsRepositoryState {
    let state = repository_state(repo);
    if !state.uses_lfs || !state.available || state.initialized {
        return state;
    }
    initialize(repo).unwrap_or(state)
}

pub fn initialize(repo: &GitRepo) -> Result<LfsRepositoryState, String> {
    let (_, conflict) = hook_status(repo);
    let args: &[&str] = if conflict {
        &["lfs", "install", "--local", "--skip-repo"]
    } else {
        &["lfs", "install", "--local"]
    };
    require_success("git lfs install --local", command(repo, args)?)?;
    Ok(repository_state(repo))
}

pub fn files(repo: &GitRepo, reference: Option<&str>) -> Result<Vec<LfsFile>, String> {
    if !probe(repo).installed {
        return pointer_files_from_index(repo);
    }
    let output = if let Some(reference) = reference {
        require_success(
            "git lfs ls-files",
            command(
                repo,
                &["lfs", "ls-files", "--json", "--long", "--size", reference],
            )?,
        )?
    } else {
        require_success(
            "git lfs ls-files",
            command(repo, &["lfs", "ls-files", "--json", "--long", "--size"])?,
        )?
    };
    let json = output_text(&output, true);
    let value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|error| format!("Git LFS returned invalid file metadata: {error}"))?;
    let entries = value
        .get("files")
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array())
        .ok_or_else(|| "Git LFS returned an unexpected file list".to_string())?;
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = entry
            .get("name")
            .or_else(|| entry.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let oid = entry
            .get("oid")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if path.is_empty() || !valid_oid(oid) {
            continue;
        }
        let size = entry.get("size").and_then(|v| v.as_u64());
        let present = entry
            .get("checkout")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        result.push(LfsFile {
            path: path.to_string(),
            oid: oid.to_string(),
            size,
            present,
        });
    }
    Ok(result)
}

fn pointer_files_from_index(repo: &GitRepo) -> Result<Vec<LfsFile>, String> {
    let output = require_success(
        "git ls-files",
        command(repo, &["ls-files", "--stage", "-z"])?,
    )?;
    let entries = output_text(&output, true);
    let mut files = Vec::new();
    for entry in entries.split('\0').filter(|entry| !entry.is_empty()) {
        let Some((metadata, path)) = entry.split_once('\t') else {
            continue;
        };
        let mut fields = metadata.split_whitespace();
        let _mode = fields.next();
        let Some(blob_oid) = fields.next() else {
            continue;
        };
        if fields.next() != Some("0") {
            continue;
        }
        let blob = match command(repo, &["cat-file", "blob", blob_oid]) {
            Ok(output) if output.status.success() && output.stdout.len() <= 1024 => output,
            _ => continue,
        };
        let pointer_text = output_text(&blob, true);
        let Some((oid, size)) = parse_pointer(pointer_text.as_bytes()) else {
            continue;
        };
        let present = worktree_file_matches(repo, path, size, &oid);
        files.push(LfsFile {
            path: path.to_string(),
            oid,
            size: Some(size),
            present,
        });
    }
    Ok(files)
}

/// Ensure paths selected for staging were transformed by Git LFS's clean filter.
pub fn validate_staged_pointers(repo: &GitRepo, paths: &[String]) -> Result<(), String> {
    for path in paths {
        let output = require_success(
            "git ls-files",
            command(repo, &["ls-files", "--stage", "-z", "--", path])?,
        )?;
        let records = output_text(&output, true);
        let blob_oid = records
            .split('\0')
            .filter_map(|record| record.split_once('\t'))
            .find_map(|(metadata, indexed_path)| {
                if indexed_path != path {
                    return None;
                }
                let mut fields = metadata.split_whitespace();
                let _mode = fields.next();
                let oid = fields.next()?;
                (fields.next() == Some("0")).then(|| oid.to_string())
            })
            .ok_or_else(|| format!("Git did not stage LFS-managed file {path}"))?;
        let blob = require_success(
            "git cat-file",
            command(repo, &["cat-file", "blob", &blob_oid])?,
        )?;
        if parse_pointer(&blob.stdout).is_none() {
            return Err(format!(
                "Git LFS did not create a pointer for staged file {path}; the staged content was kept. Verify the LFS clean filter before retrying."
            ));
        }
    }
    Ok(())
}

fn worktree_file_matches(repo: &GitRepo, path: &str, size: u64, oid: &str) -> bool {
    if repo.is_wsl()
        || path.is_empty()
        || Path::new(path).is_absolute()
        || path.split(['/', '\\']).any(|part| part == "..")
    {
        return false;
    }
    let Ok(root) = Path::new(repo.path()).canonicalize() else {
        return false;
    };
    let Ok(file) = root.join(path).canonicalize() else {
        return false;
    };
    if !file.starts_with(&root) {
        return false;
    }
    let Ok(metadata) = std::fs::metadata(&file) else {
        return false;
    };
    if metadata.len() != size {
        return false;
    }
    let Ok(bytes) = std::fs::read(file) else {
        return false;
    };
    format!("{:x}", Sha256::digest(bytes)) == oid
}

fn valid_oid(value: &str) -> bool {
    value.strip_prefix("sha256:").unwrap_or(value).len() == 64
        && value
            .strip_prefix("sha256:")
            .unwrap_or(value)
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
}

pub fn fetch(repo: &GitRepo, _reference: Option<&str>) -> Result<(), String> {
    fetch_with_progress(repo, None, |_| {})
}

pub fn fetch_with_progress(
    repo: &GitRepo,
    _reference: Option<&str>,
    report: impl FnMut(LfsTransferProgress),
) -> Result<(), String> {
    run_transfer(repo, &["lfs", "fetch"], report).map(|_| ())
}

pub fn fetch_cancellable(
    repo: &GitRepo,
    cancel: &AtomicBool,
    report: impl FnMut(LfsTransferProgress),
) -> Result<(), String> {
    fetch_with_args_cancellable(repo, &["lfs", "fetch"], cancel, report)
}

pub fn fetch_all_cancellable(
    repo: &GitRepo,
    cancel: &AtomicBool,
    report: impl FnMut(LfsTransferProgress),
) -> Result<(), String> {
    fetch_with_args_cancellable(repo, &["lfs", "fetch", "--all"], cancel, report)
}

pub fn fetch_reference_cancellable(
    repo: &GitRepo,
    remote: &str,
    reference: &str,
    cancel: &AtomicBool,
    report: impl FnMut(LfsTransferProgress),
) -> Result<(), String> {
    validate_fetch_reference(repo, remote, reference)?;
    fetch_with_args_cancellable(repo, &["lfs", "fetch", remote, reference], cancel, report)
}

fn validate_fetch_reference(repo: &GitRepo, remote: &str, reference: &str) -> Result<(), String> {
    if remote.is_empty()
        || remote.starts_with('-')
        || remote.chars().any(char::is_control)
        || remote.chars().any(char::is_whitespace)
    {
        return Err("Choose a valid Git remote before downloading an LFS reference".into());
    }
    let remotes = require_success("git remote", command(repo, &["remote"])?)?;
    let remotes = output_text(&remotes, true);
    if !remotes.lines().any(|candidate| candidate == remote) {
        return Err(format!("Git remote {remote:?} does not exist"));
    }
    let named_ref = reference == "HEAD"
        || reference.starts_with("refs/heads/")
        || reference.starts_with("refs/remotes/")
        || reference.starts_with("refs/tags/");
    if !named_ref || reference.chars().any(char::is_control) {
        return Err("Use HEAD or a fully qualified branch, remote branch, or tag reference".into());
    }
    let commit = format!("{reference}^{{commit}}");
    let resolved = command(repo, &["rev-parse", "--verify", "--quiet", &commit])?;
    if !resolved.status.success() {
        return Err(format!(
            "Git reference {reference:?} does not resolve to a commit"
        ));
    }
    Ok(())
}

fn fetch_with_args_cancellable(
    repo: &GitRepo,
    args: &[&str],
    cancel: &AtomicBool,
    report: impl FnMut(LfsTransferProgress),
) -> Result<(), String> {
    let progress_path = std::env::temp_dir().join(format!(
        "augur-lfs-{}-{}.progress",
        std::process::id(),
        NEXT_PROGRESS_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&progress_path)
        .map_err(|error| format!("cannot create transfer progress file: {error}"))?;
    let result = run_transfer_inner(repo, args, &progress_path, Some(cancel), report);
    let _ = std::fs::remove_file(progress_path);
    result.map(|_| ())
}

pub fn fsck(repo: &GitRepo) -> Result<String, String> {
    let output = require_success("git lfs fsck", command(repo, &["lfs", "fsck"])?)?;
    Ok(detail(&output))
}

pub fn download_file(repo: &GitRepo, path: &str) -> Result<(), String> {
    validate_pattern(path)?;
    let expected_head = repository_state(repo).head;
    let include = format!("--include={path}");
    run_transfer(repo, &["lfs", "fetch", &include], |_| {})?;
    if repository_state(repo).head != expected_head {
        return Err("The checked out commit changed while this LFS object was downloading. No file was populated; retry for the current branch.".into());
    }
    require_success(
        "git lfs checkout",
        command(repo, &["lfs", "checkout", "--", path])?,
    )?;
    Ok(())
}

pub fn checkout(repo: &GitRepo) -> Result<(), String> {
    require_success("git lfs checkout", command(repo, &["lfs", "checkout"])?)?;
    Ok(())
}

pub fn apply_preferences(
    repo: &GitRepo,
    preferences: &LfsRepositoryPreferences,
) -> Result<(), String> {
    let mut entries = Vec::<(&str, Option<String>)>::new();
    if let Some(include) = &preferences.fetch_include {
        entries.push((
            "lfs.fetchinclude",
            (!include.trim().is_empty()).then(|| include.trim().to_string()),
        ));
    }
    if let Some(exclude) = &preferences.fetch_exclude {
        entries.push((
            "lfs.fetchexclude",
            (!exclude.trim().is_empty()).then(|| exclude.trim().to_string()),
        ));
    }
    if let Some(value) = preferences.concurrent_transfers {
        if value == 0 {
            entries.push(("lfs.concurrenttransfers", None));
        } else if !(1..=32).contains(&value) {
            return Err("LFS concurrent transfers must be between 1 and 32".into());
        } else {
            entries.push(("lfs.concurrenttransfers", Some(value.to_string())));
        }
    }
    if let Some(value) = preferences.transfer_retries {
        if value == 0 {
            entries.push(("lfs.transfer.maxretries", None));
        } else if !(1..=20).contains(&value) {
            return Err("LFS transfer retries must be between 1 and 20".into());
        } else {
            entries.push(("lfs.transfer.maxretries", Some(value.to_string())));
        }
    }
    if let Some(value) = &preferences.locks_verify {
        if !["true", "false", "auto"].contains(&value.as_str()) {
            return Err("LFS lock verification must be true, false, or auto".into());
        }
        entries.push(("lfs.locksverify", (value != "auto").then(|| value.clone())));
    }
    if let Some(value) = &preferences.endpoint {
        let value = value.trim();
        if value.is_empty() {
            entries.push(("lfs.url", None));
        } else {
            if !(value.starts_with("https://")
                || value.starts_with("http://")
                || value.starts_with("ssh://"))
                || value.contains(char::is_whitespace)
                || value
                    .split_once("://")
                    .and_then(|(_, authority)| authority.split(['/', '?', '#']).next())
                    .is_some_and(|authority| authority.contains('@'))
            {
                return Err("LFS endpoint must be an HTTP(S) or SSH URL without whitespace".into());
            }
            entries.push(("lfs.url", Some(value.to_string())));
        }
    }
    for (key, value) in entries {
        let output = if let Some(value) = value {
            command(repo, &["config", "--local", key, &value])?
        } else {
            command(repo, &["config", "--local", "--unset-all", key])?
        };
        if !output.status.success() && output.status.code() != Some(5) {
            return Err(detail(&output));
        }
    }
    Ok(())
}

pub fn track(repo: &GitRepo, pattern: &str, lockable: bool) -> Result<(), String> {
    validate_pattern(pattern)?;
    let mut args = vec!["lfs", "track"];
    if lockable {
        args.push("--lockable");
    }
    args.extend(["--", pattern]);
    require_success("git lfs track", command(repo, &args)?)?;
    Ok(())
}

pub fn untrack(repo: &GitRepo, pattern: &str) -> Result<(), String> {
    validate_pattern(pattern)?;
    require_success(
        "git lfs untrack",
        command(repo, &["lfs", "untrack", "--", pattern])?,
    )?;
    Ok(())
}

fn validate_pattern(pattern: &str) -> Result<(), String> {
    if pattern.trim().is_empty() || pattern.chars().any(char::is_control) {
        Err("LFS patterns must be non-empty and contain no control characters".into())
    } else {
        Ok(())
    }
}

pub fn locks(repo: &GitRepo, limit: u32) -> Result<Vec<LfsLock>, String> {
    let limit = limit.clamp(1, 500).to_string();
    let output = require_success(
        "git lfs locks",
        command(repo, &["lfs", "locks", "--json", "--limit", &limit])?,
    )?;
    let json = output_text(&output, true);
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|error| error.to_string())?;
    let entries = value
        .get("locks")
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array())
        .ok_or_else(|| "Git LFS returned an unexpected lock list".to_string())?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(LfsLock {
                id: entry.get("id")?.as_u64()?.to_string(),
                path: entry.get("path")?.as_str()?.to_string(),
                owner: entry
                    .get("owner")?
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
                locked_at: entry
                    .get("locked_at")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
        })
        .collect())
}

pub fn lock(repo: &GitRepo, path: &str) -> Result<(), String> {
    validate_pattern(path)?;
    require_success("git lfs lock", command(repo, &["lfs", "lock", "--", path])?)?;
    Ok(())
}

pub fn unlock(repo: &GitRepo, id: &str, force: bool) -> Result<(), String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid LFS lock id".into());
    }
    let mut args = vec!["lfs", "unlock"];
    if force {
        args.push("--force");
    }
    args.extend(["--id", id]);
    require_success("git lfs unlock", command(repo, &args)?)?;
    Ok(())
}

pub fn cache_preview(repo: &GitRepo) -> Result<LfsCachePreview, String> {
    let environment = require_success("git lfs env", command(repo, &["lfs", "env"])?)?;
    let text = output_text(&environment, true);
    let storage = text
        .lines()
        .find_map(|line| line.strip_prefix("LocalMediaDir="))
        .map(str::to_string)
        .or_else(|| effective_config(repo, "lfs.storage"))
        .unwrap_or_else(|| {
            Path::new(repo.path())
                .join(".git/lfs/objects")
                .to_string_lossy()
                .into_owned()
        });
    if repo.is_wsl() {
        return Ok(LfsCachePreview {
            output: "Cache size and pruning are unavailable for WSL repositories in this view."
                .into(),
            ..LfsCachePreview::default()
        });
    }
    let path = Path::new(&storage);
    if effective_config(repo, "lfs.storage").is_some() {
        return Ok(LfsCachePreview {
            output: format!(
                "Cache cleanup is disabled because this repository uses custom LFS storage at {storage}."
            ),
            ..LfsCachePreview::default()
        });
    }
    let mut objects: usize = 0;
    let mut bytes: u64 = 0;
    if path.exists() {
        let mut pending = vec![path.to_path_buf()];
        while let Some(entry) = pending.pop() {
            let entries = std::fs::read_dir(&entry).map_err(|error| error.to_string())?;
            for entry in entries.flatten() {
                let metadata = entry.metadata().map_err(|error| error.to_string())?;
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else {
                    objects += 1;
                    bytes = bytes.saturating_add(metadata.len());
                }
            }
        }
    }
    let output = command(
        repo,
        &[
            "lfs",
            "prune",
            "--dry-run",
            "--verbose",
            "--verify-remote",
            "--verify-unreachable",
            "--when-unverified=halt",
        ],
    )?;
    let report = require_success("git lfs prune preview", output)?;
    let report_text = detail(&report);
    let candidates = parse_prune_candidate_count(&report_text).unwrap_or_else(|| {
        report_text
            .lines()
            .filter(|line| line.to_ascii_lowercase().contains("would be pruned"))
            .count()
    });
    Ok(LfsCachePreview {
        objects,
        bytes,
        candidates,
        candidate_bytes: 0,
        output: report_text,
    })
}

pub fn prune(repo: &GitRepo) -> Result<String, String> {
    let preview = cache_preview(repo)?;
    if preview.output.contains("cleanup is disabled") {
        return Err(preview.output);
    }
    let output = command(
        repo,
        &[
            "lfs",
            "prune",
            "--verify-remote",
            "--verify-unreachable",
            "--when-unverified=halt",
        ],
    )?;
    require_success("git lfs prune", output).map(|output| detail(&output))
}

fn parse_prune_candidate_count(output: &str) -> Option<usize> {
    for line in output.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let Some(local_index) = fields.iter().position(|field| *field == "local") else {
            continue;
        };
        let total = local_index
            .checked_sub(1)
            .and_then(|index| fields.get(index))?;
        let total = total.parse::<usize>().ok()?;
        let retained_index = fields.iter().position(|field| *field == "retained,")?;
        let retained = fields
            .get(retained_index.checked_sub(1)?)
            .and_then(|value| value.parse::<usize>().ok())?;
        return Some(total.saturating_sub(retained));
    }
    None
}

pub fn parse_pointer(bytes: &[u8]) -> Option<(String, u64)> {
    if bytes.len() > 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "version https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let mut oid = None;
    let mut size = None;
    for line in lines {
        let (key, value) = line.split_once(' ')?;
        if key.is_empty()
            || !key.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            })
            || value.is_empty()
            || value.starts_with(' ')
        {
            return None;
        }
        match key {
            "version" => return None,
            "oid" => {
                let value = value.strip_prefix("sha256:")?;
                if oid.is_some()
                    || value.len() != 64
                    || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return None;
                }
                oid = Some(value.to_string());
            }
            "size" => {
                if size.is_some() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                size = Some(value.parse::<u64>().ok()?);
            }
            // Extension keys are valid metadata and do not change the main OID.
            _ => {}
        }
    }
    Some((oid?, size?))
}

/// Read an already cached LFS object without contacting its endpoint.
pub fn read_cached_object(
    repo: &GitRepo,
    oid: &str,
    expected_size: u64,
    max_size: usize,
) -> Result<Option<Vec<u8>>, String> {
    if repo.is_wsl() {
        return Ok(None);
    }
    let oid = oid.strip_prefix("sha256:").unwrap_or(oid);
    if !valid_oid(oid) {
        return Err("Git LFS pointer contains an invalid object id".into());
    }
    if expected_size > max_size as u64 {
        return Err("Git LFS object exceeds the preview size limit".into());
    }
    let storage = local_object_storage(repo)?;
    let object = storage.join(&oid[..2]).join(&oid[2..4]).join(oid);
    if !object.exists() {
        return Ok(None);
    }
    let metadata = std::fs::metadata(&object).map_err(|error| error.to_string())?;
    if metadata.len() != expected_size || metadata.len() > max_size as u64 {
        return Err("Cached Git LFS object has an unexpected size".into());
    }
    let bytes = std::fs::read(object).map_err(|error| error.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != oid {
        return Err("Cached Git LFS object failed its SHA-256 integrity check".into());
    }
    Ok(Some(bytes))
}

fn local_object_storage(repo: &GitRepo) -> Result<PathBuf, String> {
    if probe(repo).installed {
        if let Ok(output) = command(repo, &["lfs", "env"]) {
            if output.status.success() {
                if let Some(path) = output_text(&output, true)
                    .lines()
                    .find_map(|line| line.strip_prefix("LocalMediaDir="))
                {
                    return Ok(PathBuf::from(path));
                }
            }
        }
    }
    if let Some(storage) = effective_config(repo, "lfs.storage") {
        let storage = PathBuf::from(storage);
        let storage = if storage.is_absolute() {
            storage
        } else {
            let git_dir = require_success(
                "git rev-parse",
                command(repo, &["rev-parse", "--path-format=absolute", "--git-dir"])?,
            )?;
            PathBuf::from(output_text(&git_dir, true).trim()).join(storage)
        };
        return Ok(storage.join("objects"));
    }
    let output = require_success(
        "git rev-parse",
        command(
            repo,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "lfs/objects",
            ],
        )?,
    )?;
    Ok(PathBuf::from(output_text(&output, true).trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let path = std::env::temp_dir().join(format!(
                "augur-lfs-test-{}-{}-{label}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(path: &Path, args: &[&str]) -> Output {
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap()
    }

    fn successful_git(path: &Path, args: &[&str]) {
        let output = git(path, args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repository(directory: &TempDir) -> GitRepo {
        successful_git(directory.path(), &["init"]);
        successful_git(directory.path(), &["config", "user.name", "LFS Test"]);
        successful_git(
            directory.path(),
            &["config", "user.email", "lfs@example.com"],
        );
        GitRepo::local(directory.path().to_string_lossy())
    }

    fn assert_initial_status_available(repo: &GitRepo) {
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = crate::git::spawn_open(repo.clone(), tx).unwrap();
        let result = loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(crate::git::GitEvent::Status { .. }) => break Ok(()),
                Ok(crate::git::GitEvent::StatusError(error)) => break Err(format!("{error:?}")),
                Ok(_) => {}
                Err(error) => break Err(error.to_string()),
            }
        };
        handle.close();
        result.unwrap();
    }

    #[test]
    fn pointer_mentions_do_not_make_an_ordinary_repository_use_lfs() {
        let directory = TempDir::new("pointer-mentions");
        let repo = init_repository(&directory);
        std::fs::write(
            directory.path().join("README.md"),
            "Pointer format:\nversion https://git-lfs.github.com/spec/v1\noid sha256:invalid\nsize 42\n",
        ).unwrap();
        std::fs::write(
            directory.path().join("source.rs"),
            "const VERSION: &str = \"version https://git-lfs.github.com/spec/v1\";\n",
        )
        .unwrap();
        std::fs::write(directory.path().join("example.md"), format!(
            "Pointer example:\nversion https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n", "a".repeat(64)
        )).unwrap();
        successful_git(directory.path(), &["add", "."]);
        successful_git(directory.path(), &["commit", "-m", "Add ordinary files"]);
        assert!(!repository_state(&repo).uses_lfs);
        assert_initial_status_available(&repo);
    }

    #[test]
    fn parses_only_valid_v1_pointers() {
        let oid = "a".repeat(64);
        let pointer =
            format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 42\n");
        assert_eq!(parse_pointer(pointer.as_bytes()), Some((oid, 42)));
        assert!(
            parse_pointer(b"version https://git-lfs.github.com/spec/v1\noid sha256:bad\nsize 1\n")
                .is_none()
        );
        assert!(parse_pointer(format!("Pointer example:\n{pointer}").as_bytes()).is_none());
        assert!(parse_pointer(format!("{pointer}Not a pointer\n").as_bytes()).is_none());
        assert_eq!(
            parse_pointer(pointer.replace('\n', "\r\n").as_bytes()),
            parse_pointer(pointer.as_bytes())
        );
        assert!(parse_pointer(b"ordinary file").is_none());
        assert!(parse_pointer(&vec![b'x'; 1025]).is_none());
    }

    #[test]
    fn validates_oid_prefix_and_pattern_inputs() {
        assert!(valid_oid(&format!("sha256:{}", "b".repeat(64))));
        assert!(!valid_oid("sha256:bad"));
        assert!(validate_pattern("*.psd").is_ok());
        assert!(validate_pattern("bad\npattern").is_err());
    }

    #[test]
    fn linked_worktrees_share_the_lfs_mutation_lock() {
        let directory = TempDir::new("worktree-lock");
        let primary = directory.path().join("primary");
        let linked = directory.path().join("linked");
        std::fs::create_dir_all(&primary).unwrap();
        successful_git(&primary, &["init", "-b", "main"]);
        successful_git(&primary, &["config", "user.name", "LFS Test"]);
        successful_git(
            &primary,
            &["config", "user.email", "lfs-test@example.invalid"],
        );
        std::fs::write(primary.join("tracked.txt"), "shared git directory\n").unwrap();
        successful_git(&primary, &["add", "tracked.txt"]);
        successful_git(&primary, &["commit", "-m", "Add worktree fixture"]);
        successful_git(
            &primary,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );

        let primary_repo = GitRepo::local(primary.to_string_lossy());
        let linked_repo = GitRepo::local(linked.to_string_lossy());
        assert!(Arc::ptr_eq(
            &operation_lock(&primary_repo),
            &operation_lock(&linked_repo)
        ));
    }

    #[test]
    fn parses_official_progress_records() {
        let progress = parse_progress(b"download 2/4 1024/4096 assets/tree.psd").unwrap();
        assert_eq!(progress.direction, "download");
        assert_eq!(progress.current_file, 2);
        assert_eq!(progress.total_files, 4);
        assert_eq!(progress.transferred_bytes, 1024);
        assert_eq!(progress.total_bytes, 4096);
        assert_eq!(progress.path, "assets/tree.psd");
    }

    #[test]
    fn parses_prune_candidate_summary_without_enabling_empty_cleanup() {
        assert_eq!(
            parse_prune_candidate_count("1 local object, 1 retained, done."),
            Some(0)
        );
        assert_eq!(
            parse_prune_candidate_count("4 local objects, 2 retained, done."),
            Some(2)
        );
        assert_eq!(parse_prune_candidate_count("No local objects."), None);
    }

    #[test]
    fn prepare_repository_leaves_repositories_without_lfs_untouched() {
        let directory = TempDir::new("prepare-non-lfs");
        let output = Command::new("git")
            .arg("init")
            .arg(directory.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let repo = GitRepo::local(directory.path().to_string_lossy());
        let state = prepare_repository(&repo);
        assert!(!state.uses_lfs, "{state:?}");
        let config = std::fs::read_to_string(directory.path().join(".git/config")).unwrap();
        assert!(!config.contains("filter.lfs"), "{config}");
    }
}
