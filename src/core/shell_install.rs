//! Installing and removing the `augurgit` command in user shell
//! configuration files.
//!
//! The installer writes a clearly marked block into the rc files of common
//! shells: bash/zsh and fish on Unix, Windows PowerShell profiles on
//! Windows. The markers make repeated installs update the block in place
//! (for example after the application moved) and make removal exact; every
//! other byte of the file is preserved. Both operations are user-initiated
//! from the File menu and report per-file results.

use std::path::{Path, PathBuf};

use crate::core::config::write_atomically;

/// Begin marker of the installer block. Also the anchor for in-place
/// updates; do not change without a migration story for existing installs.
pub(crate) const BLOCK_BEGIN: &str = "# >>> augur-git cli >>>";
pub(crate) const BLOCK_END: &str = "# <<< augur-git cli <<<";

/// The command name exposed by the block.
const COMMAND_NAME: &str = "augurgit";

#[derive(Clone, Copy, PartialEq)]
enum ShellKind {
    /// POSIX-style shells (bash, zsh) using a shell function.
    Posix,
    /// fish using its own function syntax.
    Fish,
    /// Windows PowerShell (both editions) using a PowerShell function.
    #[cfg_attr(not(windows), allow(dead_code))]
    PowerShell,
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
#[derive(Clone, Debug)]
pub struct TargetResult {
    pub path: PathBuf,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The block was added or its path was updated.
    Updated,
    /// The block was already present with the current content.
    Unchanged,
    /// The block was removed.
    Removed,
    /// No block was present (uninstall on a clean file).
    NotInstalled,
    Failed(String),
}

/// Whether a report came from installing or uninstalling.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    Install,
    Uninstall,
}

/// Per-file results of an install or uninstall run.
#[derive(Clone)]
pub struct ChangeReport {
    pub operation: Operation,
    pub results: Vec<TargetResult>,
    /// True when no sibling `augurgit` binary exists next to the running
    /// executable, so the installed command points at the application
    /// binary itself (bare `augurgit` then cannot use the current
    /// directory).
    pub fallback_binary: bool,
}

/// Locate the binary the installed command should invoke: the `augurgit`
/// executable next to the running one. Falls back to the running executable
/// itself when the sibling is missing (single-binary layouts).
pub(crate) fn resolve_cli_binary() -> Option<PathBuf> {
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

/// Render the full marked block for one shell kind.
fn build_block(kind: ShellKind, binary: &Path) -> String {
    let body = match kind {
        ShellKind::Posix => {
            format!("{COMMAND_NAME}() {{ {} \"$@\"; }}", quote_posix(binary))
        }
        ShellKind::Fish => format!(
            "function {COMMAND_NAME}; {} $argv; end",
            quote_fish(binary)
        ),
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

/// Insert `block` into `existing`, replacing any previously installed block.
/// Errors when the file contains a half-installed block (begin marker
/// without the end marker) instead of guessing what to keep.
fn upsert_block(existing: &str, block: &str) -> Result<String, String> {
    match locate_block(existing) {
        BlockLocation::None => {
            let mut out =
                String::with_capacity(existing.len() + block.len() + 2);
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

/// Remove the installed block, leaving the rest of the file untouched. A
/// file without a block is returned unchanged so the caller can report
/// "not installed".
fn strip_block(existing: &str) -> Result<String, String> {
    match locate_block(existing) {
        BlockLocation::None => Ok(existing.to_string()),
        BlockLocation::Whole { start, end } => {
            // Drop one newline on each side: the blank separator before the
            // block and the line break after it. A file containing only the
            // block becomes empty.
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

enum BlockLocation {
    None,
    Whole { start: usize, end: usize },
    Malformed,
}

fn locate_block(text: &str) -> BlockLocation {
    let Some(start) = text.find(BLOCK_BEGIN) else {
        return if text.contains(BLOCK_END) {
            BlockLocation::Malformed
        } else {
            BlockLocation::None
        };
    };
    let Some(relative_end) = text[start..].find(BLOCK_END) else {
        return BlockLocation::Malformed;
    };
    BlockLocation::Whole {
        start,
        end: start + relative_end + BLOCK_END.len(),
    }
}

/// Add or update the `augurgit` command in every supported shell
/// configuration. Runs on a background thread; caller shows the report.
pub fn install() -> ChangeReport {
    log::info!("[cli_install] install requested");
    let Some(binary) = resolve_cli_binary() else {
        return ChangeReport {
            operation: Operation::Install,
            results: vec![TargetResult {
                path: PathBuf::new(),
                outcome: Outcome::Failed(
                    "cannot locate the running executable".to_string(),
                ),
            }],
            fallback_binary: false,
        };
    };
    install_with_targets(shell_targets(), &binary)
}

fn install_with_targets(targets: Vec<RcTarget>, binary: &Path) -> ChangeReport {
    let fallback_binary = {
        let name = if cfg!(windows) {
            format!("{COMMAND_NAME}.exe")
        } else {
            COMMAND_NAME.to_string()
        };
        !binary.parent().is_some_and(|dir| dir.join(name).is_file())
    };

    let results = targets
        .into_iter()
        .map(|target| {
            let block = build_block(target.kind, binary);
            apply_to_target(&target, Mode::Install, &block)
        })
        .collect();
    ChangeReport {
        operation: Operation::Install,
        results,
        fallback_binary,
    }
}

/// Remove the `augurgit` command from every supported shell configuration.
pub fn uninstall() -> ChangeReport {
    log::info!("[cli_install] uninstall requested");
    uninstall_with_targets(shell_targets())
}

fn uninstall_with_targets(targets: Vec<RcTarget>) -> ChangeReport {
    let results = targets
        .into_iter()
        .map(|target| apply_to_target(&target, Mode::Uninstall, ""))
        .collect();
    ChangeReport {
        operation: Operation::Uninstall,
        results,
        fallback_binary: false,
    }
}

enum Mode {
    Install,
    Uninstall,
}

fn apply_to_target(target: &RcTarget, mode: Mode, block: &str) -> TargetResult {
    let path = target.path.clone();
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !target.create {
                return TargetResult {
                    path,
                    outcome: Outcome::NotInstalled,
                };
            }
            String::new()
        }
        Err(error) => {
            return TargetResult {
                path,
                outcome: Outcome::Failed(format!("read: {error}")),
            };
        }
    };

    let updated = match mode {
        Mode::Install => upsert_block(&existing, block),
        Mode::Uninstall => strip_block(&existing),
    };
    let updated = match updated {
        Ok(text) => text,
        Err(message) => {
            return TargetResult {
                path,
                outcome: Outcome::Failed(message),
            };
        }
    };

    if updated == existing {
        let outcome = match mode {
            Mode::Install => Outcome::Unchanged,
            Mode::Uninstall => Outcome::NotInstalled,
        };
        return TargetResult { path, outcome };
    }

    if target.create {
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                return TargetResult {
                    path,
                    outcome: Outcome::Failed(format!(
                        "create directory: {error}"
                    )),
                };
            }
        }
    }
    match write_atomically(&path, &updated) {
        Ok(()) => {
            log::info!("[cli_install] updated {}", path.display());
            TargetResult {
                path,
                outcome: match mode {
                    Mode::Install => Outcome::Updated,
                    Mode::Uninstall => Outcome::Removed,
                },
            }
        }
        Err(error) => TargetResult {
            path,
            outcome: Outcome::Failed(format!("write: {error}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_is_appended_to_plain_file() {
        let installed = build_block(
            ShellKind::Posix,
            Path::new("/opt/Augur Git.app/Contents/MacOS/augurgit"),
        );
        let result =
            upsert_block("export EDITOR=vim\nalias ll='ls -l'\n", &installed)
                .expect("append succeeds");
        assert!(result.starts_with("export EDITOR=vim\nalias ll='ls -l'\n\n"));
        assert!(result.contains(&installed));
        assert!(result.ends_with(&format!("{installed}\n")));
    }

    #[test]
    fn upsert_replaces_an_existing_block() {
        let first = build_block(ShellKind::Posix, Path::new("/old/path"));
        let second = build_block(ShellKind::Posix, Path::new("/new/path"));
        let existing = format!("keep=me\n\n{first}\nafter=1\n");
        let result =
            upsert_block(&existing, &second).expect("replace succeeds");
        assert!(!result.contains("/old/path"));
        assert!(result.contains("/new/path"));
        assert!(result.starts_with("keep=me\n"));
        assert!(result.ends_with("after=1\n"));
    }

    #[test]
    fn upsert_is_idempotent() {
        let block =
            build_block(ShellKind::Posix, Path::new("/opt/bin/augurgit"));
        let once = upsert_block("", &block).expect("first install");
        let twice = upsert_block(&once, &block).expect("second install");
        assert_eq!(once, twice);
    }

    #[test]
    fn empty_file_receives_only_the_block() {
        let block = build_block(ShellKind::Posix, Path::new("/bin/augurgit"));
        let result = upsert_block("", &block).expect("install into empty file");
        assert_eq!(result, format!("{block}\n"));
    }

    #[test]
    fn missing_end_marker_is_an_error() {
        let block = build_block(ShellKind::Posix, Path::new("/bin/augurgit"));
        let truncated = block.split(BLOCK_END).next().unwrap().to_string();
        assert!(upsert_block(&truncated, &block).is_err());
        assert!(strip_block(&truncated).is_err());
    }

    #[test]
    fn strip_removes_block_and_keeps_surroundings() {
        let block =
            build_block(ShellKind::Fish, Path::new("/usr/bin/augurgit"));
        let existing =
            format!("set -x EDITOR vim\n\n{block}\n\nset -x FOO 1\n");
        let result = strip_block(&existing).expect("strip succeeds");
        assert_eq!(result, "set -x EDITOR vim\n\nset -x FOO 1\n");
    }

    #[test]
    fn strip_of_block_only_file_becomes_empty() {
        let block = build_block(
            ShellKind::PowerShell,
            Path::new(r"C:\App\augurgit.exe"),
        );
        let existing = format!("{block}\n");
        let result = strip_block(&existing).expect("strip succeeds");
        assert_eq!(result, "");
    }

    #[test]
    fn strip_without_block_is_a_no_op() {
        let text = "export PATH=$HOME/bin\n".to_string();
        let result = strip_block(&text).expect("strip succeeds");
        assert_eq!(result, text);
    }

    #[test]
    fn posix_block_wraps_the_path_in_quotes() {
        let block = build_block(
            ShellKind::Posix,
            Path::new("/Applications/Augur Git.app/Contents/MacOS/augurgit"),
        );
        assert!(block.contains(
            "augurgit() { '/Applications/Augur Git.app/Contents/MacOS/augurgit' \"$@\"; }"
        ));
        assert!(block.starts_with(BLOCK_BEGIN));
        assert!(block.ends_with(BLOCK_END));
    }

    #[test]
    fn posix_quotes_are_escaped() {
        let quoted = quote_posix(Path::new("/weird'name/bin"));
        assert_eq!(quoted, "'/weird'\\''name/bin'");
    }

    #[test]
    fn fish_block_uses_fish_function_syntax() {
        let block =
            build_block(ShellKind::Fish, Path::new("/usr/local/bin/augurgit"));
        assert!(block.contains(
            "function augurgit; '/usr/local/bin/augurgit' $argv; end"
        ));
    }

    #[test]
    fn powershell_block_uses_powershell_function_syntax() {
        let block = build_block(
            ShellKind::PowerShell,
            Path::new(r"C:\Program Files\Augur Git\augurgit.exe"),
        );
        assert!(block.contains(
            r"function augurgit { & 'C:\Program Files\Augur Git\augurgit.exe' @args }"
        ));
    }

    /// End-to-end round trip against real files inside a temporary
    /// directory: install writes and preserves the original content,
    /// reinstalling is a no-op, and uninstalling restores the file.
    #[test]
    fn install_round_trip_preserves_and_restores_files() {
        let root = std::env::temp_dir().join(format!(
            "augur-git-cli-install-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("create temp directory");
        let original = "export EDITOR=vim\nalias ll='ls -l'\n";
        let rc = root.join("zshrc");
        std::fs::write(&rc, original).expect("seed rc file");

        let targets = vec![
            RcTarget {
                kind: ShellKind::Posix,
                path: rc.clone(),
                create: true,
            },
            // Nested directory that does not exist yet must be created.
            RcTarget {
                kind: ShellKind::Fish,
                path: root.join("fresh").join("config.fish"),
                create: true,
            },
        ];
        let binary = Path::new("/opt/Augur Git/augurgit");

        let report = install_with_targets(targets.clone(), binary);
        assert_eq!(report.results.len(), 2);
        assert!(
            report
                .results
                .iter()
                .all(|result| result.outcome == Outcome::Updated),
            "first install must report Updated: {:?}",
            report.results
        );
        let installed =
            std::fs::read_to_string(&rc).expect("read installed rc");
        assert!(installed.starts_with(original));
        assert!(
            installed
                .contains("augurgit() { '/opt/Augur Git/augurgit' \"$@\"; }")
        );
        let fish_installed =
            std::fs::read_to_string(root.join("fresh").join("config.fish"))
                .expect("read created fish config");
        assert!(fish_installed.contains("function augurgit;"));

        let again = install_with_targets(targets.clone(), binary);
        assert!(
            again
                .results
                .iter()
                .all(|result| result.outcome == Outcome::Unchanged),
            "second install must report Unchanged: {:?}",
            again.results
        );

        let removal = uninstall_with_targets(targets);
        assert!(
            removal
                .results
                .iter()
                .all(|result| result.outcome == Outcome::Removed),
            "uninstall must report Removed: {:?}",
            removal.results
        );
        assert_eq!(
            std::fs::read_to_string(&rc).expect("read restored rc"),
            original,
            "uninstall must restore the original file content"
        );

        std::fs::remove_dir_all(&root).expect("clean temp directory");
    }
}
