//! Nightly update checks and the signed Windows installation flow.

#[cfg(any(target_os = "macos", test))]
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::Update;
#[cfg(windows)]
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::{Mutex, Notify};

use augur_core::build_info::{APP_VERSION, GIT_COMMIT};

use crate::state::AppState;

const RELEASE_API: &str =
    "https://api.github.com/repos/algosoft-lab/augur-git/releases/tags/tauri-nightly";
const RELEASE_PAGE: &str = "https://github.com/algosoft-lab/augur-git/releases/tag/tauri-nightly";
const UPDATE_EVENT: &str = "augur://update-event";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const STARTUP_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotice {
    pub commit_sha: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub state: UpdateState,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub latest_commit_sha: Option<String>,
    pub can_install: bool,
    pub progress: Option<f64>,
    pub error: Option<String>,
    pub install_channel: InstallChannel,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Downloaded,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallChannel {
    WindowsInstaller,
    HomebrewCask,
    Manual,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub status: UpdateStatus,
    pub notice: Option<UpdateNotice>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum UpdateEvent {
    Status { status: UpdateStatus },
    Notice { notice: Option<UpdateNotice> },
}

#[derive(Deserialize)]
struct GithubRelease {
    body: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
}

#[derive(Clone, Debug, PartialEq)]
struct NightlyRelease {
    commit_sha: String,
    version: String,
}

struct UpdateInner {
    status: UpdateStatus,
    notice: Option<UpdateNotice>,
    pending_update: Option<Update>,
    downloaded: Option<Vec<u8>>,
    checking: bool,
    downloading: bool,
}

#[derive(Clone)]
pub struct UpdateManager {
    app: AppHandle,
    inner: Arc<Mutex<UpdateInner>>,
    automatic_checks_enabled: Arc<AtomicBool>,
    wake: Arc<Notify>,
    current_sha: &'static str,
}

impl UpdateManager {
    pub fn new(app: AppHandle, config: augur_core::config::AppConfig) -> Self {
        let status = UpdateStatus {
            state: UpdateState::Idle,
            current_version: APP_VERSION.to_owned(),
            latest_version: None,
            latest_commit_sha: None,
            can_install: false,
            progress: None,
            error: None,
            install_channel: current_install_channel(),
        };
        Self {
            app,
            inner: Arc::new(Mutex::new(UpdateInner {
                status,
                notice: None,
                pending_update: None,
                downloaded: None,
                checking: false,
                downloading: false,
            })),
            automatic_checks_enabled: Arc::new(AtomicBool::new(config.auto_check_updates)),
            wake: Arc::new(Notify::new()),
            current_sha: GIT_COMMIT,
        }
    }

    pub fn start(&self) {
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(STARTUP_DELAY).await;
            loop {
                if manager.automatic_checks_enabled.load(Ordering::Acquire) {
                    manager.check(true).await;
                }
                tokio::select! {
                    _ = tokio::time::sleep(CHECK_INTERVAL) => {},
                    _ = manager.wake.notified() => {},
                }
            }
        });
    }

    pub fn set_automatic_checks_enabled(&self, enabled: bool) {
        self.automatic_checks_enabled
            .store(enabled, Ordering::Release);
        if !enabled {
            let manager = self.clone();
            tauri::async_runtime::spawn(async move {
                manager.set_notice(None).await;
            });
        }
        self.wake.notify_one();
    }

    pub async fn snapshot(&self) -> UpdateSnapshot {
        let inner = self.inner.lock().await;
        UpdateSnapshot {
            status: inner.status.clone(),
            notice: inner.notice.clone(),
        }
    }

    pub async fn check(&self, automatic: bool) -> UpdateStatus {
        if automatic && !self.automatic_checks_enabled.load(Ordering::Acquire) {
            return self.snapshot().await.status;
        }
        {
            let mut inner = self.inner.lock().await;
            if !claim_check(&mut inner.checking) {
                return inner.status.clone();
            }
            inner.status = with_state(&inner.status, UpdateState::Checking);
            self.emit(UpdateEvent::Status {
                status: inner.status.clone(),
            });
        }

        let result = self.perform_check(automatic).await;
        let mut inner = self.inner.lock().await;
        inner.checking = false;
        match result {
            Ok((release, pending_update)) => {
                if release.commit_sha == self.current_sha && release.version == APP_VERSION {
                    inner.pending_update = None;
                    inner.downloaded = None;
                    inner.status = UpdateStatus {
                        state: UpdateState::UpToDate,
                        current_version: APP_VERSION.to_owned(),
                        latest_version: Some(release.version),
                        latest_commit_sha: Some(release.commit_sha),
                        can_install: false,
                        progress: None,
                        error: None,
                        install_channel: current_install_channel(),
                    };
                    inner.notice = None;
                    self.emit(UpdateEvent::Notice { notice: None });
                } else {
                    let can_install = pending_update.is_some();
                    inner.pending_update = pending_update;
                    inner.downloaded = None;
                    inner.status = UpdateStatus {
                        state: UpdateState::Available,
                        current_version: APP_VERSION.to_owned(),
                        latest_version: Some(release.version.clone()),
                        latest_commit_sha: Some(release.commit_sha.clone()),
                        can_install,
                        progress: None,
                        error: None,
                        install_channel: current_install_channel(),
                    };
                    let dismissed = self
                        .app
                        .try_state::<AppState>()
                        .map(|state| state.config().dismissed_update_commit);
                    if dismissed.flatten().as_deref() != Some(&release.commit_sha) {
                        inner.notice = Some(UpdateNotice {
                            commit_sha: release.commit_sha,
                            version: release.version,
                        });
                        self.emit(UpdateEvent::Notice {
                            notice: inner.notice.clone(),
                        });
                    }
                }
            }
            Err(error) => {
                inner.status = failure_status(error);
            }
        }
        let status = inner.status.clone();
        self.emit(UpdateEvent::Status {
            status: status.clone(),
        });
        status
    }

    async fn perform_check(
        &self,
        _automatic: bool,
    ) -> Result<(NightlyRelease, Option<Update>), String> {
        if cfg!(debug_assertions) {
            return Err("Update checks are available in packaged builds".to_owned());
        }
        if !valid_sha(self.current_sha) {
            return Err("This build does not include a valid commit SHA".to_owned());
        }
        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent(format!("Augur-Git/{}", APP_VERSION))
            .build()
            .map_err(|error| error.to_string())?
            .get(RELEASE_API)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("GitHub returned HTTP {}", response.status()));
        }
        let body = response.text().await.map_err(|error| error.to_string())?;
        if body.len() > 512_000 {
            return Err("GitHub release metadata exceeded the size limit".to_owned());
        }
        let release: GithubRelease = serde_json::from_str(&body)
            .map_err(|error| format!("Invalid release metadata: {error}"))?;
        let release = parse_release(&release.body, &release.html_url, &release.assets)?;

        #[cfg(windows)]
        let pending_update = if release.commit_sha != self.current_sha
            || release.version != APP_VERSION
        {
            let update = self
                .app
                .updater()
                .map_err(|error| error.to_string())?
                .check()
                .await
                .map_err(|error| error.to_string())?;
            if update
                .as_ref()
                .is_some_and(|update| update.version.to_string() != release.version)
            {
                return Err(
                    "Nightly release metadata does not match the signed updater feed".to_owned(),
                );
            }
            update
        } else {
            None
        };
        #[cfg(not(windows))]
        let pending_update = None;

        Ok((release, pending_update))
    }

    pub async fn download(&self) -> Result<UpdateStatus, String> {
        let update = {
            let mut inner = self.inner.lock().await;
            if inner.downloading {
                return Err("An update download is already in progress".to_owned());
            }
            if !can_download_update(
                &inner.status,
                inner.pending_update.is_some(),
                inner.downloading,
            ) {
                return Err("No signed Windows update is available".to_owned());
            }
            let Some(update) = inner.pending_update.take() else {
                return Err("No signed Windows update is available".to_owned());
            };
            inner.downloading = true;
            inner.status.state = UpdateState::Downloading;
            inner.status.progress = Some(0.0);
            inner.status.error = None;
            self.emit(UpdateEvent::Status {
                status: inner.status.clone(),
            });
            update
        };

        let manager = self.clone();
        let mut downloaded = 0_u64;
        let result = update
            .download(
                move |chunk, total| {
                    downloaded = downloaded.saturating_add(chunk as u64);
                    let progress = total
                        .filter(|total| *total > 0)
                        .map(|total| (downloaded as f64 / total as f64 * 100.0).clamp(0.0, 100.0));
                    let manager = manager.clone();
                    tauri::async_runtime::spawn(async move {
                        manager.set_progress(progress).await;
                    });
                },
                || {},
            )
            .await;

        let mut inner = self.inner.lock().await;
        inner.downloading = false;
        match result {
            Ok(bytes) => {
                inner.downloaded = Some(bytes);
                inner.pending_update = Some(update);
                inner.status.state = UpdateState::Downloaded;
                inner.status.progress = Some(100.0);
                inner.status.error = None;
            }
            Err(error) => {
                inner.pending_update = Some(update);
                inner.status.state = UpdateState::Available;
                inner.status.progress = None;
                inner.status.error = Some(error.to_string());
            }
        }
        let status = inner.status.clone();
        self.emit(UpdateEvent::Status {
            status: status.clone(),
        });
        if status.state == UpdateState::Available {
            Err(status
                .error
                .clone()
                .unwrap_or_else(|| "Update download failed".to_owned()))
        } else {
            Ok(status)
        }
    }

    async fn set_progress(&self, progress: Option<f64>) {
        let mut inner = self.inner.lock().await;
        if inner.status.state != UpdateState::Downloading {
            return;
        }
        inner.status.progress = progress;
        self.emit(UpdateEvent::Status {
            status: inner.status.clone(),
        });
    }

    pub async fn install(&self) -> Result<(), String> {
        let (update, bytes) = {
            let mut inner = self.inner.lock().await;
            if !can_install_update(
                &inner.status,
                inner.pending_update.is_some(),
                inner.downloaded.is_some(),
            ) {
                return Err("No downloaded Windows update is ready to install".to_owned());
            }
            let Some(update) = inner.pending_update.take() else {
                return Err("No downloaded Windows update is ready to install".to_owned());
            };
            let Some(bytes) = inner.downloaded.take() else {
                inner.pending_update = Some(update);
                return Err("The downloaded update is no longer available".to_owned());
            };
            (update, bytes)
        };
        if let Err(error) = update.install(&bytes) {
            let mut inner = self.inner.lock().await;
            inner.pending_update = Some(update);
            inner.downloaded = Some(bytes);
            inner.status.error = Some(error.to_string().chars().take(512).collect());
            self.emit(UpdateEvent::Status {
                status: inner.status.clone(),
            });
            return Err(error.to_string());
        }
        Ok(())
    }

    pub async fn dismiss(&self, commit_sha: String) -> Result<(), String> {
        if !valid_sha(&commit_sha) {
            return Err("Invalid update commit identifier".to_owned());
        }
        let current_notice = self.inner.lock().await.notice.clone();
        if current_notice
            .as_ref()
            .map(|notice| notice.commit_sha.as_str())
            != Some(&commit_sha)
        {
            return Err("The update notice is no longer active".to_owned());
        }
        let state = self
            .app
            .try_state::<AppState>()
            .ok_or_else(|| "Application settings are unavailable".to_owned())?;
        state.update_settings(|settings| {
            settings.config.dismissed_update_commit = Some(commit_sha);
        });
        self.set_notice(None).await;
        Ok(())
    }

    async fn set_notice(&self, notice: Option<UpdateNotice>) {
        let mut inner = self.inner.lock().await;
        inner.notice = notice.clone();
        self.emit(UpdateEvent::Notice { notice });
    }

    fn emit(&self, event: UpdateEvent) {
        let _ = self.app.emit(UPDATE_EVENT, event);
    }
}

#[tauri::command]
pub async fn get_update_snapshot(
    updates: State<'_, UpdateManager>,
) -> Result<UpdateSnapshot, String> {
    Ok(updates.snapshot().await)
}

#[tauri::command]
pub async fn check_for_updates(updates: State<'_, UpdateManager>) -> Result<UpdateStatus, String> {
    Ok(updates.check(false).await)
}

#[tauri::command]
pub async fn download_update(updates: State<'_, UpdateManager>) -> Result<UpdateStatus, String> {
    updates.download().await
}

#[tauri::command]
pub async fn install_update(updates: State<'_, UpdateManager>) -> Result<(), String> {
    updates.install().await
}

#[tauri::command]
pub fn set_auto_check_updates(
    enabled: bool,
    state: State<'_, AppState>,
    updates: State<'_, UpdateManager>,
) {
    state.update_settings(|settings| settings.config.auto_check_updates = enabled);
    updates.set_automatic_checks_enabled(enabled);
}

#[tauri::command]
pub async fn dismiss_update_notice(
    commit_sha: String,
    updates: State<'_, UpdateManager>,
) -> Result<(), String> {
    updates.dismiss(commit_sha).await
}

fn with_state(status: &UpdateStatus, state: UpdateState) -> UpdateStatus {
    UpdateStatus {
        state,
        current_version: status.current_version.clone(),
        latest_version: status.latest_version.clone(),
        latest_commit_sha: status.latest_commit_sha.clone(),
        can_install: false,
        progress: None,
        error: None,
        install_channel: status.install_channel,
    }
}

fn claim_check(checking: &mut bool) -> bool {
    if *checking {
        false
    } else {
        *checking = true;
        true
    }
}

fn can_download_update(status: &UpdateStatus, has_pending_update: bool, downloading: bool) -> bool {
    !downloading
        && has_pending_update
        && status.state == UpdateState::Available
        && status.can_install
        && status.install_channel == InstallChannel::WindowsInstaller
}

fn can_install_update(
    status: &UpdateStatus,
    has_pending_update: bool,
    has_downloaded_bytes: bool,
) -> bool {
    has_pending_update
        && has_downloaded_bytes
        && status.state == UpdateState::Downloaded
        && status.can_install
        && status.install_channel == InstallChannel::WindowsInstaller
}

fn failure_status(error: String) -> UpdateStatus {
    UpdateStatus {
        state: UpdateState::Error,
        current_version: APP_VERSION.to_owned(),
        latest_version: None,
        latest_commit_sha: None,
        can_install: false,
        progress: None,
        error: Some(error.chars().take(512).collect()),
        install_channel: current_install_channel(),
    }
}

fn parse_release(
    body: &str,
    html_url: &str,
    assets: &[ReleaseAsset],
) -> Result<NightlyRelease, String> {
    if body.len() > 100_000 || html_url != RELEASE_PAGE {
        return Err("GitHub returned unexpected nightly release metadata".to_owned());
    }
    let commit_sha = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| {
            pair[0]
                .trim_matches(|ch: char| !ch.is_ascii_alphabetic())
                .eq_ignore_ascii_case("commit")
        })
        .map(|pair| {
            pair[1]
                .trim_matches(|ch: char| !ch.is_ascii_hexdigit())
                .to_ascii_lowercase()
        })
        .filter(|sha| valid_sha(sha))
        .ok_or_else(|| "Nightly release does not contain a full commit SHA".to_owned())?;
    let version = body
        .lines()
        .find_map(|line| line.strip_prefix("Version: "))
        .filter(|version| valid_nightly_version(version))
        .ok_or_else(|| "Nightly release does not contain a valid version".to_owned())?;
    if !assets.iter().any(|asset| asset.name == "latest.json") {
        return Err("Nightly release does not contain the signed update feed".to_owned());
    }
    Ok(NightlyRelease {
        commit_sha,
        version: version.to_owned(),
    })
}

fn valid_sha(sha: &str) -> bool {
    (sha.len() == 40 || sha.len() == 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_nightly_version(version: &str) -> bool {
    let Some((base, build)) = version.split_once("-nightly.") else {
        return false;
    };
    let mut parts = base.split('.');
    parts.clone().count() == 3
        && parts.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && !build.is_empty()
        && build.bytes().all(|byte| byte.is_ascii_digit())
}

fn current_install_channel() -> InstallChannel {
    if cfg!(windows) && !cfg!(debug_assertions) {
        return InstallChannel::WindowsInstaller;
    }
    #[cfg(target_os = "macos")]
    if homebrew_cask_installed(
        &std::env::var("HOMEBREW_PREFIX").ok(),
        std::env::consts::ARCH,
    ) {
        return InstallChannel::HomebrewCask;
    }
    InstallChannel::Manual
}

#[cfg(any(target_os = "macos", test))]
fn homebrew_cask_path(prefix: &Option<String>, arch: &str) -> Option<PathBuf> {
    let root = prefix
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(Path::new)
        .or_else(|| match arch {
            "aarch64" => Some(Path::new("/opt/homebrew")),
            _ => Some(Path::new("/usr/local")),
        });
    root.map(|root| root.join("Caskroom/augur-git"))
}

#[cfg(any(target_os = "macos", test))]
fn homebrew_cask_installed(prefix: &Option<String>, arch: &str) -> bool {
    homebrew_cask_path(prefix, arch).is_some_and(|path| path.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_valid_nightly_release_metadata() {
        let release = parse_release(
            "Automated Tauri nightly build for commit 0123456789abcdef0123456789abcdef01234567.\nVersion: 0.1.1-nightly.24",
            RELEASE_PAGE,
            &[ReleaseAsset { name: "latest.json".to_owned() }],
        )
        .unwrap();
        assert_eq!(
            release.commit_sha,
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(release.version, "0.1.1-nightly.24");
    }

    #[test]
    fn release_metadata_rejects_missing_commit_version_or_untrusted_page() {
        let assets = [ReleaseAsset {
            name: "latest.json".to_owned(),
        }];
        assert!(parse_release("Version: 0.1.1-nightly.2", RELEASE_PAGE, &assets).is_err());
        assert!(
            parse_release(
                "commit 0123456789abcdef0123456789abcdef01234567",
                RELEASE_PAGE,
                &assets
            )
            .is_err()
        );
        assert!(parse_release("", "https://example.com", &assets).is_err());
        assert!(
            parse_release(
                "commit 0123456789abcdef0123456789abcdef01234567\nVersion: 0.1.1-nightly.2",
                RELEASE_PAGE,
                &[]
            )
            .is_err()
        );
        assert!(
            parse_release(
                "commit 0123456789abcdef0123456789abcdef01234567\nVersion: 0.1.1-nightly.2",
                RELEASE_PAGE,
                &[ReleaseAsset {
                    name: "augur-git.exe".to_owned()
                }]
            )
            .is_err()
        );
    }

    #[test]
    fn nightly_version_requires_three_numeric_parts_and_run_number() {
        assert!(valid_nightly_version("0.1.1-nightly.24"));
        assert!(!valid_nightly_version("0.1.1"));
        assert!(!valid_nightly_version("0.1.1-nightly.x"));
        assert!(!valid_nightly_version("0.1-nightly.24"));
    }

    #[test]
    fn updater_failures_are_reported_as_bounded_error_states() {
        let status = failure_status("network failure".repeat(100));
        assert_eq!(status.state, UpdateState::Error);
        assert!(!status.can_install);
        assert_eq!(status.error.unwrap().len(), 512);
    }

    #[test]
    fn duplicate_checks_share_one_in_flight_claim() {
        let mut checking = false;
        assert!(claim_check(&mut checking));
        assert!(!claim_check(&mut checking));
        checking = false;
        assert!(claim_check(&mut checking));
    }

    #[test]
    fn download_and_install_actions_are_guarded_to_a_ready_windows_update() {
        let mut status = UpdateStatus {
            state: UpdateState::Available,
            current_version: "0.1.0".to_owned(),
            latest_version: Some("0.1.1-nightly.24".to_owned()),
            latest_commit_sha: Some("a".repeat(40)),
            can_install: true,
            progress: None,
            error: None,
            install_channel: InstallChannel::WindowsInstaller,
        };
        assert!(can_download_update(&status, true, false));
        assert!(!can_download_update(&status, false, false));
        assert!(!can_download_update(&status, true, true));
        status.install_channel = InstallChannel::HomebrewCask;
        assert!(!can_download_update(&status, true, false));

        status.install_channel = InstallChannel::WindowsInstaller;
        status.state = UpdateState::Downloaded;
        assert!(can_install_update(&status, true, true));
        assert!(!can_install_update(&status, true, false));
        assert!(!can_install_update(&status, false, true));
        status.install_channel = InstallChannel::Manual;
        assert!(!can_install_update(&status, true, true));
    }

    #[test]
    fn homebrew_detection_uses_the_selected_prefix_and_cask_token() {
        use std::fs;

        let prefix =
            std::env::temp_dir().join(format!("augur-homebrew-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&prefix);
        let prefix_string = prefix.to_string_lossy().into_owned();
        assert!(!homebrew_cask_installed(
            &Some(prefix_string.clone()),
            "aarch64"
        ));

        fs::create_dir_all(prefix.join("Caskroom/augur-git")).unwrap();
        assert!(homebrew_cask_installed(
            &Some(prefix_string.clone()),
            "aarch64"
        ));
        assert_eq!(
            homebrew_cask_path(&Some(prefix_string), "aarch64"),
            Some(prefix.join("Caskroom/augur-git"))
        );
        assert_eq!(
            homebrew_cask_path(&None, "aarch64"),
            Some(PathBuf::from("/opt/homebrew/Caskroom/augur-git"))
        );
        assert_eq!(
            homebrew_cask_path(&None, "x86_64"),
            Some(PathBuf::from("/usr/local/Caskroom/augur-git"))
        );
        fs::remove_dir_all(prefix).unwrap();
    }
}
