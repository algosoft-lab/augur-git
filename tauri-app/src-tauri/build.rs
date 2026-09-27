//! Build script.
//!
//! Besides the usual Tauri codegen this script guarantees the CLI sidecar
//! exists before `tauri_build` validates the bundle manifest. The real
//! `augurgit-tauri` binary is produced by the `sidecar` npm script that runs as
//! Tauri's `beforeBuildCommand`, so a normal `tauri build` and `tauri dev` always
//! bundle the real companion. A bare `cargo build` or `cargo check` has no npm
//! hook, so a clearly labelled placeholder is written instead: it refuses to
//! run and says why, which is better than a build failure with no explanation.

use std::path::{Path, PathBuf};

fn main() {
    // Record the commit the binaries were built from. The core crate degrades
    // to `unknown` when this is absent, so a plain `cargo build` still works.
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=AUGUR_GIT_COMMIT={commit}");

    ensure_sidecar();

    tauri_build::build();
}

/// Path of the sidecar the bundle manifest expects, for the target being built.
///
/// `TAURI_ENV_TARGET_TRIPLE` is only set by `tauri_build`, which runs after
/// this function, so the triple is reconstructed from the `CARGO_CFG_*`
/// variables Cargo provides to build scripts. `rustc -vV` is the fallback.
fn sidecar_path() -> Option<PathBuf> {
    let triple = target_triple()?;
    let name = if cfg!(windows) {
        format!("augurgit-tauri-{triple}.exe")
    } else {
        format!("augurgit-tauri-{triple}")
    };
    Some(Path::new("binaries").join(name))
}

fn target_triple() -> Option<String> {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").ok()?;
    let os = std::env::var("CARGO_CFG_TARGET_OS").ok()?;
    let env = std::env::var("CARGO_CFG_TARGET_ENV").ok().unwrap_or_default();
    let abi = std::env::var("CARGO_CFG_TARGET_ABI").ok();
    let arch = match arch.as_str() {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => other,
    };
    let os = match os.as_str() {
        "macos" => "apple-darwin",
        "windows" => {
            if let Some(abi) = abi.as_deref() {
                return Some(format!("{arch}-pc-windows-{abi}"));
            }
            "windows"
        }
        other => other,
    };
    if os == "apple-darwin" {
        return Some(format!("{arch}-{os}"));
    }
    if env.is_empty() {
        Some(format!("{arch}-{os}"))
    } else {
        Some(format!("{arch}-{os}-{env}"))
    }
}

fn ensure_sidecar() {
    let Some(path) = sidecar_path() else {
        return;
    };
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        log_line(&format!("could not create {}: {error}", parent.display()));
        return;
    }
    let message = concat!(
        "This is a placeholder for the augurgit-tauri CLI companion.\n",
        "It exists only so `cargo build` can run outside the Tauri CLI.\n",
        "Run `npm run tauri:build` or `npm run tauri:dev` to produce the real ",
        "binary.\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = format!("#!/bin/sh\nprintf '%s' '{message}' >&2\nexit 127\n");
        if std::fs::write(&path, script).is_ok() {
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    #[cfg(windows)]
    {
        let _ = std::fs::write(&path, message);
    }
    log_line(&format!(
        "wrote a placeholder CLI sidecar at {}",
        path.display()
    ));
}

fn log_line(message: &str) {
    println!("cargo:warning={message}");
}
