//! Repository location abstraction: local directories and WSL distributions.
//!
//! A [`GitRepo`] bundles where a repository physically lives with the
//! repository path as seen from inside that location, and knows how to build
//! the `std::process::Command` that reaches the location's own `git`
//! executable. Every Git invocation in the worker flows through
//! [`GitRepo::command`], so supporting a new location means implementing this
//! one construction site.
//!
//! WSL repositories are reached with
//! `wsl.exe -d <distro> --cd <path> --exec git`: `--exec` bypasses the login
//! shell, so arguments are forwarded to the Linux process as structured argv
//! on both sides of the boundary. No shell is involved anywhere.
//!
//! This module only depends on `std` so its platform-specific code can be
//! type-checked for the Windows target without a full cross toolchain.

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Where a repository physically lives.
///
/// The enum exists on every platform so a configuration file written on
/// Windows parses everywhere; executing against a WSL location is only
/// possible on Windows and is rejected explicitly elsewhere. The WSL variant
/// is only constructed on Windows, which the dead-code analysis cannot see
/// from the platform-gated conversion sites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoLocation {
    /// A plain directory the platform's `git` executable reaches directly.
    Local,
    /// A repository inside a Windows Subsystem for Linux distribution,
    /// operated through the distro's own `git` via `wsl.exe`.
    #[cfg_attr(not(windows), allow(dead_code))]
    Wsl { distro: String },
}

impl RepoLocation {
    /// The WSL distribution name, or `None` for a local repository.
    pub fn distro(&self) -> Option<&str> {
        match self {
            Self::Local => None,
            Self::Wsl { distro } => Some(distro),
        }
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Everything required to talk to one repository: its location plus the
/// canonical path as seen from inside that location (`C:\dev\repo` for a
/// local Windows repository, `/home/u/repo` inside a distribution).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRepo {
    location: RepoLocation,
    path: String,
}

impl GitRepo {
    /// Construct a repository handle. The worker and configuration layer are
    /// the intended constructors; tests may also use [`GitRepo::local`].
    pub(crate) fn new(location: RepoLocation, path: impl Into<String>) -> Self {
        Self {
            location,
            path: path.into(),
        }
    }

    /// A repository in a plain local directory.
    pub fn local(path: impl Into<String>) -> Self {
        Self::new(RepoLocation::Local, path)
    }

    pub fn location(&self) -> &RepoLocation {
        &self.location
    }

    /// The repository path as seen from inside its location.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn is_wsl(&self) -> bool {
        matches!(self.location, RepoLocation::Wsl { .. })
    }

    /// Build the base command for one Git invocation.
    ///
    /// Callers append their regular arguments afterwards, including the
    /// `-C <path>` pair. For WSL the `-C` value duplicates `--cd` and is
    /// redundant but harmless; keeping it spares every argument builder from
    /// location awareness.
    pub fn command(&self) -> Command {
        #[allow(unused_mut)] // mutated only behind `cfg(windows)`
        let mut command = match &self.location {
            RepoLocation::Local => Command::new("git"),
            RepoLocation::Wsl { distro } => {
                #[cfg(windows)]
                {
                    wsl_base_command(distro, &self.path, "git")
                }
                #[cfg(not(windows))]
                {
                    // Unreachable through supported flows: repository opening
                    // rejects WSL locations on non-Windows platforms before
                    // any command is built. Fall back to the platform git so
                    // the state can never launch a foreign program silently.
                    let _ = distro;
                    Command::new("git")
                }
            }
        };

        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        command
    }

    /// Build a command that runs `program` (resolved on the location's own
    /// PATH) with the repository as its working directory. Git cannot answer
    /// every location-specific question — for example reading an untracked
    /// file inside a WSL distribution — so a small number of best-effort
    /// helpers run through this constructor. No shell is involved on either
    /// side.
    pub(crate) fn command_in_location(&self, program: &str) -> Command {
        #[allow(unused_mut)] // mutated only behind `cfg(windows)`
        let mut command = match &self.location {
            RepoLocation::Local => {
                let mut command = Command::new(program);
                command.current_dir(&self.path);
                command
            }
            RepoLocation::Wsl { distro } => {
                #[cfg(windows)]
                {
                    wsl_base_command(distro, &self.path, program)
                }
                #[cfg(not(windows))]
                {
                    // Unreachable through supported flows; see `command`.
                    let _ = (distro, program);
                    Command::new(program)
                }
            }
        };

        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        command
    }

    /// Short display label combining the location with the directory name,
    /// for example `repo` or `Ubuntu · repo`.
    pub fn label(&self) -> String {
        match &self.location {
            RepoLocation::Local => dir_name(&self.path).to_string(),
            RepoLocation::Wsl { distro } => {
                format!("{distro} · {}", dir_name(&self.path))
            }
        }
    }
}

/// Final path segment of a Windows or POSIX path, for display purposes.
fn dir_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Build the `wsl.exe` argument prefix that positions a Linux-side process
/// inside `path` and executes `program` without a shell. Exposed as a pure
/// function so the argument shape stays unit-testable on every platform.
#[cfg(windows)]
fn wsl_command_args(distro: &str, path: &str, program: &str) -> Vec<String> {
    vec![
        "-d".to_string(),
        distro.to_string(),
        "--cd".to_string(),
        path.to_string(),
        "--exec".to_string(),
        program.to_string(),
    ]
}

#[cfg(windows)]
fn wsl_base_command(distro: &str, path: &str, program: &str) -> Command {
    let mut command = Command::new("wsl.exe");
    command.args(wsl_command_args(distro, path, program));
    command
}

/// Build a `wsl.exe` management command for host-level queries that do not
/// target one repository, such as listing the installed distributions. The
/// only caller runs on Windows.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn wsl_host_command(args: &[&str]) -> Command {
    #[allow(unused_mut)] // mutated only behind `cfg(windows)`
    let mut command = Command::new("wsl.exe");
    command.args(args);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Decode `wsl.exe` output, which is UTF-16LE whenever stdout is redirected
/// (with or without a BOM) and plain UTF-8 on interactive consoles. Only
/// reachable on Windows; kept platform-independent so the fixtures run
/// everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn decode_wsl_output(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16le(&bytes[2..]);
    }
    // Redirected wsl.exe output reliably interleaves NUL bytes; real UTF-8
    // output essentially never contains one.
    if bytes.contains(&0) {
        return decode_utf16le(bytes);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

/// Parse the distro names printed by `wsl -l -q` (already decoded). Only
/// reachable on Windows; kept platform-independent so the fixtures run
/// everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_wsl_distro_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                // `wsl -l` can print guidance footers; real distro names
                // never start with an option or parenthesis.
                && !line.starts_with('(')
                && !line.starts_with('-')
        })
        .map(str::to_string)
        .collect()
}

/// Recognize `\\wsl$\<distro>\...` and `\\wsl.localhost\<distro>\...` inputs.
/// Only reachable on Windows (WSL open dialog); fixtures run everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_wsl_unc_path(input: &str) -> bool {
    parse_unc_path(input).is_some()
}

/// Split a WSL UNC path into `(distro, linux path)` by direct component
/// mapping, so a pasted `\\wsl$\Ubuntu\home\u\repo` becomes
/// `("Ubuntu", "/home/u/repo")`. Returns `None` for anything else. Only
/// reachable on Windows; fixtures run everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_unc_path(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim().trim_matches(['"', '\'']);
    let mut parts = trimmed.split(['\\', '/']).filter(|part| !part.is_empty());
    let prefix = parts.next()?;
    if !prefix.eq_ignore_ascii_case("wsl$")
        && !prefix.eq_ignore_ascii_case("wsl.localhost")
    {
        return None;
    }
    let distro = parts.next()?.to_string();
    if distro.is_empty() {
        return None;
    }
    let rest = parts.collect::<Vec<_>>().join("/");
    let path = if rest.is_empty() {
        "/".to_string()
    } else {
        format!("/{rest}")
    };
    Some((distro, path))
}

/// Validate a user-entered Linux path for the WSL open dialog.
/// Returns the trimmed path or a static rejection reason. Only reachable on
/// Windows; fixtures run everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn validate_linux_path(input: &str) -> Result<String, &'static str> {
    let path = input.trim();
    if path.is_empty() {
        return Err("empty");
    }
    if !path.starts_with('/') {
        // `--exec` performs no shell expansion, so `~` and relative paths
        // cannot be resolved.
        return Err("not-absolute");
    }
    if path.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
        return Err("control-characters");
    }
    Ok(path.to_string())
}

/// Map a failed WSL repository-open probe to an error key by inspecting the
/// captured stderr. Pure so the decision table stays testable everywhere.
pub fn classify_open_failure(stderr: &str) -> &'static str {
    let text = stderr.to_ascii_lowercase();
    if text.contains("no distribution with the supplied name") {
        return "err-wsl-distro-not-found";
    }
    if text.contains("git")
        && (text.contains("not found") || text.contains("not installed"))
    {
        return "err-wsl-git-missing";
    }
    if text.contains("not a git repository") {
        return "err-not-a-repo";
    }
    if text.contains("cannot change to")
        || text.contains("no such file or directory")
        || text.contains("does not exist")
    {
        return "err-path-not-exist";
    }
    "err-not-a-repo"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_command_targets_git() {
        let repo = GitRepo::local("/tmp/repo");
        assert_eq!(repo.command().get_program(), "git");
        assert_eq!(repo.path(), "/tmp/repo");
        assert!(!repo.is_wsl());
    }

    #[test]
    #[cfg(windows)]
    fn wsl_command_uses_exec_without_shell() {
        let args = wsl_command_args("Ubuntu-22.04", "/home/u/my repo", "git");
        assert_eq!(
            args,
            vec![
                "-d",
                "Ubuntu-22.04",
                "--cd",
                "/home/u/my repo",
                "--exec",
                "git"
            ]
        );
        let repo = GitRepo::new(
            RepoLocation::Wsl {
                distro: "Ubuntu".into(),
            },
            "/home/u/repo",
        );
        let command = repo.command();
        assert_eq!(command.get_program(), "wsl.exe");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["-d", "Ubuntu", "--cd", "/home/u/repo", "--exec", "git"]
        );
        let cat = repo.command_in_location("cat");
        assert_eq!(
            cat.get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["-d", "Ubuntu", "--cd", "/home/u/repo", "--exec", "cat"]
        );
    }

    #[test]
    fn labels_combine_distro_and_directory() {
        let local = GitRepo::local(r"C:\dev\augur-git");
        assert_eq!(local.label(), "augur-git");
        let wsl = GitRepo::new(
            RepoLocation::Wsl {
                distro: "Ubuntu".into(),
            },
            "/home/u/augur-git",
        );
        assert_eq!(wsl.label(), "Ubuntu · augur-git");
        assert!(wsl.is_wsl());
    }

    #[test]
    fn decode_wsl_output_accepts_utf16_and_utf8() {
        let utf16 = encode_utf16le("Ubuntu-22.04\n");
        assert_eq!(decode_wsl_output(&utf16), "Ubuntu-22.04\n");
        let mut bom = vec![0xFF, 0xFE];
        bom.extend(encode_utf16le("Debian"));
        assert_eq!(decode_wsl_output(&bom), "Debian");
        assert_eq!(decode_wsl_output(b"Ubuntu\n"), "Ubuntu\n");
        assert_eq!(decode_wsl_output(b""), "");
    }

    fn encode_utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect()
    }

    #[test]
    fn distro_list_parser_trims_and_drops_footers() {
        let text = "Ubuntu-22.04\r\nDebian\r\n\r\n docker-desktop \n(older builds print guidance)\n";
        assert_eq!(
            parse_wsl_distro_list(text),
            vec![
                "Ubuntu-22.04".to_string(),
                "Debian".to_string(),
                "docker-desktop".to_string(),
            ]
        );
        assert!(parse_wsl_distro_list("").is_empty());
    }

    #[test]
    fn distro_list_parser_decodes_multiple_utf16_distros() {
        let output = encode_utf16le("Ubuntu\r\narchlinux\r\n");
        let decoded = decode_wsl_output(&output);

        assert_eq!(
            parse_wsl_distro_list(&decoded),
            vec!["Ubuntu".to_string(), "archlinux".to_string()]
        );
    }

    #[test]
    fn unc_paths_split_into_distro_and_linux_path() {
        assert_eq!(
            parse_unc_path(r"\\wsl$\Ubuntu\home\u\repo"),
            Some(("Ubuntu".to_string(), "/home/u/repo".to_string()))
        );
        assert_eq!(
            parse_unc_path(r"\\wsl.localhost\Debian-12\srv\git\proj\"),
            Some(("Debian-12".to_string(), "/srv/git/proj".to_string()))
        );
        // Forward-slash paste of the same share resolves identically.
        assert_eq!(
            parse_unc_path("//wsl$/Ubuntu/home/u/repo"),
            Some(("Ubuntu".to_string(), "/home/u/repo".to_string()))
        );
        assert_eq!(
            parse_unc_path(r"\\wsl$\Ubuntu"),
            Some(("Ubuntu".into(), "/".into()))
        );
        assert!(parse_unc_path(r"C:\dev\repo").is_none());
        assert!(parse_unc_path(r"\\server\share\repo").is_none());
        assert!(is_wsl_unc_path(r"\\wsl$\Ubuntu\home"));
        assert!(!is_wsl_unc_path("/home/u/repo"));
    }

    #[test]
    fn linux_path_validation_rejects_relative_and_control_input() {
        assert_eq!(
            validate_linux_path("  /home/u/repo \t"),
            Ok("/home/u/repo".to_string())
        );
        assert!(matches!(validate_linux_path("~/repo"), Err("not-absolute")));
        assert!(matches!(validate_linux_path("repo"), Err("not-absolute")));
        assert!(matches!(validate_linux_path("  "), Err("empty")));
        assert!(matches!(
            validate_linux_path("/bad\npath"),
            Err("control-characters")
        ));
    }

    #[test]
    fn open_failure_classification_prefers_specific_wsl_causes() {
        assert_eq!(
            classify_open_failure(
                "There is no distribution with the supplied name."
            ),
            "err-wsl-distro-not-found"
        );
        assert_eq!(
            classify_open_failure("/usr/bin/git: not found"),
            "err-wsl-git-missing"
        );
        assert_eq!(
            classify_open_failure(
                "fatal: not a git repository (or any of the parent directories)"
            ),
            "err-not-a-repo"
        );
        assert_eq!(
            classify_open_failure(
                "fatal: cannot change to '/no/repo': No such file or directory"
            ),
            "err-path-not-exist"
        );
        assert_eq!(classify_open_failure("unknown failure"), "err-not-a-repo");
    }
}
