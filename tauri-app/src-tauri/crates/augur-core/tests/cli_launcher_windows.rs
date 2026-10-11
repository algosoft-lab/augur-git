#![cfg(windows)]

use augur_core::cli::{Parsed, parse_at, parse_launcher_at};
use augur_core::paths::normalize_extended_path;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
struct LaunchRecord {
    pid: u32,
    arguments: Vec<String>,
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "augur-cli-launch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn repo(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.join("nested directory")).unwrap();
        assert!(
            Command::new("git")
                .arg("init")
                .arg("--quiet")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn plain(path: &Path) -> String {
    normalize_extended_path(path).to_string_lossy().into_owned()
}

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn copy_launcher(directory: &Path) -> PathBuf {
    let launcher = directory.join("agit.exe");
    fs::copy(env!("CARGO_BIN_EXE_agit"), &launcher).unwrap();
    launcher
}

#[test]
fn resolves_multiple_repositories_and_rejects_invalid_arguments_before_launch() {
    let fixture = Fixture::new();
    let first = fixture.repo("-repo with spaces café");
    let second = fixture.repo("second");
    let parsed = parse_at(
        &[
            "--".into(),
            "-repo with spaces café\\nested directory".into(),
            format!("{}\\", second.display()).into(),
            first.join("nested directory").into_os_string(),
        ],
        &fixture.0,
    );
    let Parsed::Run(parsed) = parsed else {
        panic!("expected validated repository roots: {parsed:?}");
    };
    assert_eq!(parsed.paths, vec![plain(&first), plain(&second)]);
    assert!(matches!(
        parse_at(&["second".into(), "missing".into()], &fixture.0),
        Parsed::UsageError(_)
    ));
    assert!(matches!(
        parse_at(&[".".into()], &fixture.0),
        Parsed::UsageError(_)
    ));
}

#[test]
fn bare_launcher_opens_the_current_worktree_root() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo with spaces café");
    let nested = repo.join("nested directory");
    let Parsed::Run(parsed) = parse_launcher_at(&[], &nested) else {
        panic!("expected current repository");
    };
    assert_eq!(parsed.paths, vec![plain(&repo)]);
    assert!(matches!(
        parse_launcher_at(&[], &fixture.0),
        Parsed::UsageError(_)
    ));
}

#[test]
fn resolves_linked_worktrees_and_submodules() {
    let fixture = Fixture::new();
    let source = fixture.repo("source repo");
    git(
        &source,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "Initial",
        ],
    );

    let linked = fixture.0.join("linked worktree");
    git(
        &source,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    fs::create_dir_all(linked.join("nested directory")).unwrap();
    let Parsed::Run(parsed) = parse_at(
        &[linked.join("nested directory").into_os_string()],
        &fixture.0,
    ) else {
        panic!("expected linked worktree");
    };
    assert_eq!(parsed.paths, vec![plain(&linked)]);

    let parent = fixture.repo("parent repo");
    git(&parent, &["config", "user.name", "Test"]);
    git(&parent, &["config", "user.email", "test@example.invalid"]);
    let source_text = source.to_str().unwrap();
    git(
        &parent,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            source_text,
            "submodule",
        ],
    );
    git(&parent, &["add", "."]);
    git(&parent, &["commit", "--quiet", "-m", "Add submodule"]);
    let submodule = parent.join("submodule");
    fs::create_dir_all(submodule.join("nested directory")).unwrap();
    let Parsed::Run(parsed) = parse_at(
        &[submodule.join("nested directory").into_os_string()],
        &fixture.0,
    ) else {
        panic!("expected submodule working tree");
    };
    assert_eq!(parsed.paths, vec![plain(&submodule)]);
}

#[test]
fn launcher_detaches_and_preserves_windows_argument_boundaries() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo 'quoted' café");
    let launcher = copy_launcher(&fixture.0);
    let gui = fixture.0.join("augur-git-tauri.exe");
    fs::copy(env!("CARGO_BIN_EXE_augur-git-test-gui"), &gui).unwrap();
    let record = fixture.0.join("arguments.json");
    let start = Instant::now();
    let status = Command::new(&launcher)
        .arg(repo.join("nested directory"))
        .env("AUGUR_TEST_GUI_RECORD", &record)
        .env("AUGUR_TEST_GUI_DELAY_MS", "8000")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(
        status.success(),
        "launcher exited with status {:?}",
        status.code()
    );
    assert!(start.elapsed() < Duration::from_secs(5));

    let deadline = Instant::now() + Duration::from_secs(3);
    while !record.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let launch: LaunchRecord = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(launch.arguments, vec!["--".to_string(), plain(&repo)]);
    let pid = launch.pid.to_string();
    assert!(
        Command::new("taskkill")
            .args(["/PID", pid.as_str(), "/T", "/F"])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn help_version_and_exit_codes_work_without_a_gui() {
    let fixture = Fixture::new();
    let launcher = copy_launcher(&fixture.0);
    for flag in ["--help", "--version"] {
        let output = Command::new(&launcher).arg(flag).output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Augur Git"));
    }
    let output = Command::new(&launcher)
        .arg("missing")
        .current_dir(&fixture.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    let output = Command::new(&launcher)
        .arg(fixture.0.join("missing repository"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn reports_a_missing_gui_binary() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo");
    let launcher = copy_launcher(&fixture.0);
    let output = Command::new(launcher).arg(repo).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not locate Augur Git"));
}
