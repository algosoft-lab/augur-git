#![cfg(unix)]

use augur_core::cli::{Parsed, parse_at};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
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

#[test]
fn resolves_subdirectories_worktrees_and_dash_names_without_partial_launches() {
    let fixture = Fixture::new();
    let first = fixture.repo("-repo with spaces");
    let second = fixture.repo("second");
    let Parsed::Run(parsed) = parse_at(
        &[
            "--".into(),
            "-repo with spaces/nested directory".into(),
            "second".into(),
        ],
        &fixture.0,
    ) else {
        panic!("expected repository roots")
    };
    assert_eq!(
        parsed.paths,
        vec![first.to_string_lossy(), second.to_string_lossy()]
    );
    assert!(matches!(
        parse_at(&["second".into(), "missing".into()], &fixture.0),
        Parsed::UsageError(_)
    ));
    assert!(matches!(
        parse_at(&[".".into()], &fixture.0),
        Parsed::UsageError(_)
    ));
    let linked = fixture.0.join("linked worktree");
    // An unborn repository has no commit to link; create one without global Git config.
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&second)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "Initial",
                "--quiet"
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&second)
            .args(["worktree", "add", "--detach", "--quiet"])
            .arg(&linked)
            .arg("HEAD")
            .status()
            .unwrap()
            .success()
    );
    let Parsed::Run(parsed) = parse_at(&[linked.into_os_string()], &fixture.0) else {
        panic!("expected linked worktree")
    };
    assert_eq!(parsed.paths.len(), 1);
}

#[test]
fn launcher_returns_before_gui_exits_and_preserves_argument_boundaries() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo 'quoted' 中");
    let launcher = fixture.0.join("agit");
    fs::copy(env!("CARGO_BIN_EXE_agit"), &launcher).unwrap();
    let gui = fixture.0.join("augur-git-tauri");
    fs::write(
        &gui,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/arguments\"\nsleep 2\n",
    )
    .unwrap();
    fs::set_permissions(&gui, fs::Permissions::from_mode(0o755)).unwrap();
    let start = Instant::now();
    let output = Command::new(&launcher)
        .arg(repo.join("nested directory"))
        .env("XDG_CONFIG_HOME", fixture.0.join("config"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "launcher waited for the GUI"
    );
    let arguments = fixture.0.join("arguments");
    while !arguments.exists() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::read_to_string(arguments).unwrap(),
        format!("--\n{}\n", repo.display())
    );
}

#[test]
fn help_version_and_invalid_paths_work_without_a_gui() {
    let fixture = Fixture::new();
    let launcher = fixture.0.join("agit");
    fs::copy(env!("CARGO_BIN_EXE_agit"), &launcher).unwrap();
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
        .env("XDG_CONFIG_HOME", fixture.0.join("config"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}
