//! Installing and removing the `augurgit-tauri` command in user shell
//! configuration files.
//!
//! The installer writes a clearly marked block into the rc files of common
//! shells: bash/zsh and fish on Unix, Windows PowerShell profiles on Windows.
//! The markers make repeated installs update the block in place and make removal
//! exact; every other byte of the file is preserved. Both operations are
//! user-initiated from the File menu and report per-file results.
//!
//! The marker text and command name intentionally differ from the GPUI
//! application so the two installers never touch each other's block.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Begin marker of the installer block. Also the anchor for in-place updates.
pub const BLOCK_BEGIN: &str = "# >>> augur-git-tauri cli >>>";
pub const BLOCK_END: &str = "# <<< augur-git-tauri cli <<<";

/// The command name exposed by the block.
const COMMAND_NAME: &str = crate::build_info::CLI_COMMAND_NAME;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellKind {
    /// POSIX-style shells (bash, zsh) using a shell function.
    Posix,
    /// fish using its own function syntax.
    Fish,
    /// Windows PowerShell (both editions) using a PowerShell function.
    #[cfg_attr(not(windows), allow(dead_code))]
    PowerShell,
}

/// Which installer operation produced a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Install,
    Remove,
}

/// Result of touching one configuration file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Updated,
    Unchanged,
    NotInstalled,
    Failed,
}

/// One shell configuration file the installer knows how to touch.
#[derive(Clone)]
struct RcTarget {
    kind: ShellKind,
    path: PathBuf,
    /// Create missing parent directories and the file itself. `false` for
    /// optional targets that are only touched when they already exist.
    create: bool,
}

/// Result for a single configuration file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetResult {
    pub path: PathBuf,
    pub outcome: Outcome,
}

/// Per-file results of an install or uninstall run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangeReport {
    pub operation: Operation,
    pub results: Vec<TargetResult>,
    /// True when no dedicated `augurgit-tauri` binary sits next to the running
    /// executable, so the installed command points at the application binary
    /// itself and a bare invocation cannot open the current directory.
    pub fallback_binary: bool,
}

#[cfg(not(windows))]
fn shell_targets() -> Vec<RcTarget> {
    let Some(home) = dirs::home_dir() else {
        log::warn!("[cli_install] no home directory; nothing to install");
        return Vec::new();
    };
    let mut targets = vec![
        // zsh resolves its rc inside $ZDOTDIR when that variable is set.
        RcTarget {
            kind: ShellKind::Posix,
            path: std::env::var_os("ZDOTDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.clone())
                .join(".zshrc"),
            create: true,
        },
        RcTarget {
            kind: ShellKind::Posix,
            path: home.join(".bashrc"),
            create: true,
        },
    ];
    let fish_config = home.join(".config").join("fish").join("config.fish");
    if fish_config.is_file() {
        targets.push(RcTarget {
            kind: ShellKind::Fish,
            path: fish_config,
            create: false,
        });
    }
    targets
}

#[cfg(windows)]
fn shell_targets() -> Vec<RcTarget> {
    let Some(documents) = dirs::document_dir() else {
        log::warn!("[cli_install] no documents directory; nothing to install");
        return Vec::new();
    };
    vec![
        // PowerShell 7+ and Windows PowerShell 5.1 use separate profiles.
        RcTarget {
            kind: ShellKind::PowerShell,
            path: documents
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
            create: true,
        },
        RcTarget {
            kind: ShellKind::PowerShell,
            path: documents
                .join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
            create: true,
        },
    ]
}

/// Locate the binary the installed command should invoke: the
/// `augurgit-tauri` executable next to the running one. Falls back to the
/// running executable when the sibling is missing (single-binary layouts).
pub fn resolve_cli_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        format!("{COMMAND_NAME}.exe")
    } else {
        COMMAND_NAME.to_string()
    };
    if let Some(dir) = exe.parent() {
        let candidate = dir.join(&name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    log::warn!(
        "[cli_install] no dedicated {name} next to {}; the installed command will target the application binary",
        exe.display()
    );
    Some(exe)
}

/// Render the full marked block for one shell kind.
fn build_block(kind: ShellKind, binary: &Path) -> String {
    let body = match kind {
        ShellKind::Posix => {
            format!("{COMMAND_NAME}() {{ {} \"$@\"; }}", quote_posix(binary))
        }
        ShellKind::Fish => format!("function {COMMAND_NAME}; {} $argv; end", quote_fish(binary)),
        ShellKind::PowerShell => format!(
            "function {COMMAND_NAME} {{ & {} @args }}",
            quote_powershell(binary)
        ),
    };
    format!("{BLOCK_BEGIN}\n{body}\n{BLOCK_END}")
}

fn quote_posix(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn quote_fish(path: &Path) -> String {
    format!(
        "'{}'",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    )
}

fn quote_powershell(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockLocation {
    None,
    Whole { start: usize, end: usize },
    Malformed,
}

fn locate_block(existing: &str) -> BlockLocation {
    let Some(begin) = existing.find(BLOCK_BEGIN) else {
        return BlockLocation::None;
    };
    match existing[begin..].find(BLOCK_END) {
        Some(offset) => {
            let end = begin + offset + BLOCK_END.len();
            BlockLocation::Whole { start: begin, end }
        }
        None => BlockLocation::Malformed,
    }
}

/// Insert `block` into `existing`, replacing any previously installed block.
/// A half-installed block is reported instead of guessing what to keep.
fn upsert_block(existing: &str, block: &str) -> Result<String, String> {
    match locate_block(existing) {
        BlockLocation::None => {
            let mut out = String::with_capacity(existing.len() + block.len() + 2);
            out.push_str(existing);
            if !out.is_empty() {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str(block);
            out.push('\n');
            Ok(out)
        }
        BlockLocation::Whole { start, end } => {
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..start]);
            out.push_str(block);
            out.push_str(&existing[end..]);
            Ok(out)
        }
        BlockLocation::Malformed => Err(format!(
            "existing {COMMAND_NAME} block is missing its '{BLOCK_END}' marker; restore or remove it manually, then retry"
        )),
    }
}

/// Remove the installed block, leaving the rest of the file untouched. A file
/// without a block is returned unchanged so the caller can report
/// "not installed".
fn strip_block(existing: &str) -> Result<String, String> {
    match locate_block(existing) {
        BlockLocation::None => Ok(existing.to_string()),
        BlockLocation::Whole { start, end } => {
            // Drop one newline on each side so a file containing only the block
            // becomes empty.
            let before = &existing[..start];
            let after = &existing[end..];
            let mut out = String::with_capacity(existing.len());
            out.push_str(before.strip_suffix('\n').unwrap_or(before));
            out.push_str(after.strip_prefix('\n').unwrap_or(after));
            Ok(out)
        }
        BlockLocation::Malformed => Err(format!(
            "existing {COMMAND_NAME} block is missing its '{BLOCK_END}' marker; restore or remove it manually, then retry"
        )),
    }
}

fn apply(target: &RcTarget, binary: &Path, operation: Operation) -> TargetResult {
    let existing = match std::fs::read_to_string(&target.path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if operation == Operation::Remove || !target.create {
                return TargetResult {
                    path: target.path.clone(),
                    outcome: Outcome::NotInstalled,
                };
            }
            String::new()
        }
        Err(error) => {
            log::warn!(
                "[cli_install] failed to read {}: {error}",
                target.path.display()
            );
            return TargetResult {
                path: target.path.clone(),
                outcome: Outcome::Failed,
            };
        }
    };

    let block = build_block(target.kind, binary);
    let updated = match operation {
        Operation::Install => upsert_block(&existing, &block),
        Operation::Remove => strip_block(&existing),
    };
    let updated = match updated {
        Ok(text) => text,
        Err(error) => {
            log::warn!("[cli_install] {}: {error}", target.path.display());
            return TargetResult {
                path: target.path.clone(),
                outcome: Outcome::Failed,
            };
        }
    };

    let outcome = if updated == existing {
        match operation {
            Operation::Install => Outcome::Unchanged,
            Operation::Remove => Outcome::NotInstalled,
        }
    } else {
        Outcome::Updated
    };

    if outcome == Outcome::Updated {
        if let Some(parent) = target.path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                log::warn!(
                    "[cli_install] failed to create {}: {error}",
                    parent.display()
                );
                return TargetResult {
                    path: target.path.clone(),
                    outcome: Outcome::Failed,
                };
            }
        }
        if let Err(error) = std::fs::write(&target.path, &updated) {
            log::warn!("[cli_install] failed to write {}: {error}", target.path.display());
            return TargetResult {
                path: target.path.clone(),
                outcome: Outcome::Failed,
            };
        }
    }

    TargetResult {
        path: target.path.clone(),
        outcome,
    }
}

/// Write or update the command in every known shell configuration file.
pub fn install() -> ChangeReport {
    let Some(binary) = resolve_cli_binary() else {
        return ChangeReport {
            operation: Operation::Install,
            results: Vec::new(),
            fallback_binary: true,
        };
    };
    let fallback_binary = !binary
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case(COMMAND_NAME));
    let results = shell_targets()
        .iter()
        .map(|target| apply(target, &binary, Operation::Install))
        .collect();
    ChangeReport {
        operation: Operation::Install,
        results,
        fallback_binary,
    }
}

/// Remove the command from every known shell configuration file.
pub fn remove() -> ChangeReport {
    let Some(binary) = resolve_cli_binary() else {
        return ChangeReport {
            operation: Operation::Remove,
            results: Vec::new(),
            fallback_binary: true,
        };
    };
    let results = shell_targets()
        .iter()
        .map(|target| apply(target, &binary, Operation::Remove))
        .collect();
    ChangeReport {
        operation: Operation::Remove,
        results,
        fallback_binary: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_markers_are_distinct_from_the_gpuui_installer() {
        assert_ne!(BLOCK_BEGIN, "# >>> augur-git cli >>>");
        assert_ne!(COMMAND_NAME, "augurgit");
    }

    #[test]
    fn install_appends_a_marked_block() {
        let block = build_block(ShellKind::Posix, Path::new("/apps/augur-git-tauri"));
        let updated = upsert_block("export PATH=/usr/bin\n", &block).unwrap();
        assert!(updated.starts_with("export PATH=/usr/bin\n\n"));
        assert!(updated.contains("augurgit-tauri() { '/apps/augur-git-tauri' \"$@\"; }"));
        assert!(updated.ends_with('\n'));
    }

    #[test]
    fn repeated_install_updates_in_place() {
        let first = build_block(ShellKind::Posix, Path::new("/old/path"));
        let second = build_block(ShellKind::Posix, Path::new("/new/path"));
        let installed = upsert_block("", &first).unwrap();
        let updated = upsert_block(&installed, &second).unwrap();
        assert!(!updated.contains("/old/path"));
        assert!(updated.contains("/new/path"));
        assert_eq!(updated.matches(BLOCK_BEGIN).count(), 1);
    }

    #[test]
    fn removal_restores_the_original_bytes() {
        let original = "export PATH=/usr/bin\n";
        let block = build_block(ShellKind::Posix, Path::new("/apps/augur-git-tauri"));
        let installed = upsert_block(original, &block).unwrap();
        assert_eq!(strip_block(&installed).unwrap(), original);
    }

    #[test]
    fn removing_without_a_block_is_a_no_op() {
        assert_eq!(strip_block("nothing here\n").unwrap(), "nothing here\n");
    }

    #[test]
    fn a_half_installed_block_is_rejected() {
        let partial = format!("{BLOCK_BEGIN}\nfunction x {{ }}\n");
        assert!(upsert_block(&partial, "new").is_err());
        assert!(strip_block(&partial).is_err());
    }

    #[test]
    fn quoting_survives_awkward_paths() {
        let block = build_block(ShellKind::Posix, Path::new("/apps/it's here/bin"));
        assert!(block.contains(r"'/apps/it'\''s here/bin'"));
        let fish = build_block(ShellKind::Fish, Path::new(r"C:\Program Files\app"));
        assert!(fish.contains(r"'C:\\Program Files\\app'"));
    }

    #[test]
    fn a_missing_optional_target_reports_not_installed() {
        let target = RcTarget {
            kind: ShellKind::Fish,
            path: PathBuf::from("/definitely/missing/config.fish"),
            create: false,
        };
        let result = apply(&target, Path::new("/apps/augur-git-tauri"), Operation::Install);
        assert_eq!(result.outcome, Outcome::NotInstalled);
    }
}
