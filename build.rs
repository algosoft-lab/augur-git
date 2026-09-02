//! Build-time metadata and the Windows executable icon.
//!
//! `assets/algogit.ico` is generated from `assets/algogit.svg` at multiple
//! sizes (16/24/32/48/64/128/256). The regeneration command is documented in
//! AGENTS.md.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/algogit.ico");
    watch_git_metadata();
    configure_windows_stack();

    let commit = git_commit_hash().unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=AUGUR_GIT_COMMIT={commit}");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/algogit.ico");
        res.compile().expect("Failed to compile Windows resource");
    }
}

fn configure_windows_stack() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg=/stack:{}", 8 * 1024 * 1024);
    }
}

fn git_commit_hash() -> Option<String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("git")
        .current_dir(manifest_dir)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let hash = String::from_utf8(output.stdout).ok()?.trim().to_string();
    let valid_length = matches!(hash.len(), 40 | 64);
    if valid_length && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(hash)
    } else {
        None
    }
}

fn watch_git_metadata() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Some(git_dir) = resolve_git_dir(manifest_dir) else {
        return;
    };

    let head = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head.display());

    let Ok(head_contents) = fs::read_to_string(&head) else {
        return;
    };
    let Some(reference) = head_contents.strip_prefix("ref: ").map(str::trim)
    else {
        return;
    };

    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join(reference).display()
    );
}

fn resolve_git_dir(manifest_dir: &Path) -> Option<PathBuf> {
    let dot_git = manifest_dir.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }

    let git_file = fs::read_to_string(dot_git).ok()?;
    let git_dir = git_file.strip_prefix("gitdir:")?.trim();
    let git_dir = PathBuf::from(git_dir);
    if git_dir.is_absolute() {
        Some(git_dir)
    } else {
        Some(manifest_dir.join(git_dir))
    }
}
