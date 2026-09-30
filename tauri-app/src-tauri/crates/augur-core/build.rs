use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn main() {
    println!("cargo:rerun-if-env-changed=AUGUR_RELEASE_VERSION");
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("Cargo must provide CARGO_MANIFEST_DIR to build scripts"),
    );

    watch_git_metadata(&manifest_dir);

    let commit = git_output(&manifest_dir, &["rev-parse", "HEAD"])
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());

    let short_commit = commit.chars().take(12).collect::<String>();
    println!("cargo:rustc-env=AUGUR_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=AUGUR_GIT_COMMIT_SHORT={short_commit}");

    if let Ok(version) = std::env::var("AUGUR_RELEASE_VERSION") {
        if !version.trim().is_empty() {
            println!("cargo:rustc-env=AUGUR_RELEASE_VERSION={version}");
        }
    }
}

fn watch_git_metadata(manifest_dir: &Path) {
    watch_git_path(manifest_dir, "HEAD");
    watch_git_path(manifest_dir, "packed-refs");

    if let Some(output) = git_output(manifest_dir, &["symbolic-ref", "--quiet", "HEAD"])
        .filter(|output| output.status.success())
    {
        let reference = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !reference.is_empty() {
            watch_git_path(manifest_dir, &reference);
        }
    }
}

fn watch_git_path(manifest_dir: &Path, path: &str) {
    let Some(output) = git_output(manifest_dir, &["rev-parse", "--git-path", path])
        .filter(|output| output.status.success())
    else {
        return;
    };

    let git_path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let watched_path = if git_path.is_absolute() {
        git_path
    } else {
        manifest_dir.join(git_path)
    };

    println!("cargo:rerun-if-changed={}", watched_path.display());
}

fn git_output(manifest_dir: &Path, args: &[&str]) -> Option<Output> {
    Command::new("git")
        .arg("-C")
        .arg(manifest_dir)
        .args(args)
        .output()
        .ok()
}
