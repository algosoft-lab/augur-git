//! Build script.
//!
//! Besides the usual Tauri codegen this script records the commit the
//! binaries were built from, for the About window and `--version` output.

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

    tauri_build::build();
}
