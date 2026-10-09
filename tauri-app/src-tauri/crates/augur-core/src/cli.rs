//! Command-line parsing for the desktop binary.
//!
//! Repository paths are validated and canonicalized before the window opens so
//! a typo is reported instead of producing a broken tab. Only local
//! directories are reachable from the command line; WSL locations must be
//! opened from the interface.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::build_info;
use crate::paths::normalize_extended_path;

/// A parsed command line: canonicalized repository paths to open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliInvocation {
    pub paths: Vec<String>,
}

/// Outcome of parsing arguments without process-level side effects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(CliInvocation),
    Help,
    Version,
    UsageError(String),
}

/// Parse `std::env::args_os`, printing help or version and exiting when the
/// arguments request it. This is the desktop binary's entry point.
pub fn parse_from_env() -> CliInvocation {
    let mut args = std::env::args_os();
    args.next();
    match parse(&args.collect::<Vec<_>>()) {
        Parsed::Run(invocation) => invocation,
        Parsed::Help => {
            print_help();
            std::process::exit(0);
        }
        Parsed::Version => {
            println!("{}", build_info::version_line());
            std::process::exit(0);
        }
        Parsed::UsageError(message) => {
            eprintln!("{}: {message}", build_info::APP_BINARY);
            eprintln!(
                "Try '{} --help' for more information.",
                build_info::APP_BINARY
            );
            std::process::exit(2);
        }
    }
}

/// Parse an argument list. Pure apart from the filesystem access used to
/// validate paths, so it is unit-testable.
pub fn parse(args: &[OsString]) -> Parsed {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    parse_at(args, &cwd)
}

pub fn parse_launcher(args: &[OsString]) -> Parsed {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    parse_launcher_at(args, &cwd)
}

pub fn parse_launcher_at(args: &[OsString], cwd: &Path) -> Parsed {
    if args.is_empty() {
        return parse_at(&[OsString::from(".")], cwd);
    }
    parse_at(args, cwd)
}

pub fn parse_at(args: &[OsString], cwd: &Path) -> Parsed {
    let mut requested: Vec<OsString> = Vec::new();
    let mut options = true;
    for arg in args {
        let lossy = arg.to_string_lossy();
        if options && lossy == "--" {
            options = false;
            continue;
        }
        if options && lossy.starts_with('-') && lossy != "-" {
            match lossy.as_ref() {
                "-h" | "--help" => return Parsed::Help,
                "-V" | "--version" => return Parsed::Version,
                _ => {
                    return Parsed::UsageError(format!("unrecognized option '{lossy}'"));
                }
            }
        }
        if lossy.is_empty() {
            return Parsed::UsageError("empty path argument".to_string());
        }
        requested.push(arg.clone());
    }

    // The desktop application launched from a dock or file manager carries no
    // arguments and must not grow a tab for whatever its working directory
    // happens to be.
    if requested.is_empty() {
        return Parsed::Run(CliInvocation { paths: Vec::new() });
    }

    match resolve_paths(&requested, cwd) {
        Ok(paths) => Parsed::Run(CliInvocation { paths }),
        Err(message) => Parsed::UsageError(message),
    }
}

/// Canonicalize and validate every requested path. Each argument must be an
/// existing directory.
fn resolve_paths(requested: &[OsString], cwd: &Path) -> Result<Vec<String>, String> {
    let mut resolved = Vec::with_capacity(requested.len());
    for request in requested {
        let root = resolve_path(request, cwd)?;
        if !resolved.contains(&root) {
            resolved.push(root);
        }
    }
    Ok(resolved)
}

fn resolve_path(request: &OsString, cwd: &Path) -> Result<String, String> {
    let candidate = Path::new(request);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        cwd.join(candidate)
    };
    let canonical = std::fs::canonicalize(&absolute)
        .map_err(|_| format!("path '{}' does not exist", candidate.to_string_lossy()))?;
    if !canonical.is_dir() {
        return Err(format!(
            "path '{}' is not a directory",
            candidate.to_string_lossy()
        ));
    }
    let output = crate::git::GitRepo::local("")
        .command()
        .arg("-C")
        .arg(&canonical)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| format!("could not run Git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "path '{}' is not a Git working tree: {}",
            candidate.to_string_lossy(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let root = String::from_utf8(output.stdout)
        .map_err(|_| "Git returned a non-UTF-8 repository path".to_string())?;
    let root = root.strip_suffix("\n").unwrap_or(&root);
    #[cfg(windows)]
    let root = root.strip_suffix("\r").unwrap_or(root);
    let root = std::fs::canonicalize(root).map_err(|error| error.to_string())?;
    normalize_extended_path(&root)
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "repository path is not valid UTF-8".to_string())
}

/// Resolve paths received from a second launch. The secondary process already
/// validated its arguments, so failures are reported to the caller instead of
/// terminating it.
pub fn resolve_forwarded(raw: &[String], cwd: &Path) -> Result<Vec<String>, String> {
    let mut resolved = Vec::with_capacity(raw.len());
    for request in raw {
        let root = resolve_path(&OsString::from(request), cwd)?;
        if !resolved.contains(&root) {
            resolved.push(root);
        }
    }
    Ok(resolved)
}

pub fn print_help() {
    print_help_for(build_info::APP_BINARY);
}

pub fn print_help_for(binary: &str) {
    println!(
        "{} - Git GUI client

Usage:
  {binary} [OPTIONS] [PATH]...

Opens the Git working tree containing each PATH as a repository tab. When an application window is already
running, the paths are forwarded to it; otherwise a new window opens with
them.

Options:
  -h, --help     Print this help and exit
  -V, --version  Print version information and exit
  --             Treat following arguments as paths

Examples:
  {binary} .
  {binary} ~/projects/repo-a ~/projects/repo-b",
        build_info::version_line(),
        binary = binary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(value: &str) -> OsString {
        OsString::from(value)
    }

    #[test]
    fn explicit_existing_directory_is_canonicalized() {
        let output = crate::git::GitRepo::local("")
            .command()
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .unwrap();
        let assets =
            std::fs::canonicalize(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
        let parsed = parse(&[os("i18n")]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(
            invocation.paths,
            vec![assets.to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn a_bare_invocation_opens_no_paths() {
        let parsed = parse(&[]);
        assert_eq!(parsed, Parsed::Run(CliInvocation::default()));
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert_eq!(parse(&[os("--help")]), Parsed::Help);
        assert_eq!(parse(&[os("-h")]), Parsed::Help);
        assert_eq!(parse(&[os("--version")]), Parsed::Version);
        assert_eq!(parse(&[os("-V")]), Parsed::Version);
    }

    #[test]
    fn unknown_options_and_missing_paths_are_usage_errors() {
        assert!(matches!(parse(&[os("--nope")]), Parsed::UsageError(_)));
        assert!(matches!(
            parse(&[os("definitely-missing-directory")]),
            Parsed::UsageError(_)
        ));
        assert!(matches!(parse(&[os("")]), Parsed::UsageError(_)));
    }

    #[test]
    fn multiple_paths_are_accepted() {
        let parsed = parse(&[os("i18n"), os("src")]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(invocation.paths.len(), 1);
    }

    #[test]
    fn a_file_argument_is_rejected() {
        assert!(matches!(
            parse(&[os("keymap.default.json")]),
            Parsed::UsageError(_)
        ));
    }
}
