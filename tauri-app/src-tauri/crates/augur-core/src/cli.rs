//! Command-line parsing shared by the desktop binary and its CLI companion.
//!
//! The desktop application and the `augurgit-tauri` command share this parser.
//! Repository paths are validated and canonicalized before the window opens so
//! a typo is reported instead of producing a broken tab. Only local
//! directories are reachable from the command line; WSL locations must be
//! opened from the interface.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::build_info;
use crate::paths::normalize_extended_path;

/// Command exposed to users by the shell installer.
pub const CLI_COMMAND_NAME: &str = build_info::CLI_COMMAND_NAME;

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
/// arguments request it. This is the single entry point used by both binaries.
pub fn parse_from_env() -> CliInvocation {
    let mut args = std::env::args_os();
    let program = args.next();
    match parse(program.as_deref(), &args.collect::<Vec<_>>()) {
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
            eprintln!("{CLI_COMMAND_NAME}: {message}");
            eprintln!("Try '{CLI_COMMAND_NAME} --help' for more information.");
            std::process::exit(2);
        }
    }
}

/// Parse a program name and argument list. Pure apart from the filesystem
/// access used to validate paths, so it is unit-testable.
pub fn parse(program: Option<&OsStr>, args: &[OsString]) -> Parsed {
    let mut requested: Vec<OsString> = Vec::new();
    for arg in args {
        let lossy = arg.to_string_lossy();
        if lossy.starts_with('-') && lossy != "-" {
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

    // A bare `augurgit-tauri` invocation means "open the directory I am in".
    // The desktop application launched from a dock or file manager carries no
    // arguments and must not grow a tab for whatever its working directory
    // happens to be, so the default is keyed to the executable name.
    if requested.is_empty() {
        if program_is_cli_alias(program) {
            return current_directory_invocation();
        }
        return Parsed::Run(CliInvocation { paths: Vec::new() });
    }

    match resolve_paths(&requested) {
        Ok(paths) => Parsed::Run(CliInvocation { paths }),
        Err(message) => Parsed::UsageError(message),
    }
}

fn current_directory_invocation() -> Parsed {
    match std::env::current_dir() {
        Ok(cwd) => match std::fs::canonicalize(&cwd) {
            Ok(canonical) if canonical.is_dir() => Parsed::Run(CliInvocation {
                paths: vec![
                    normalize_extended_path(&canonical)
                        .to_string_lossy()
                        .into_owned(),
                ],
            }),
            _ => Parsed::UsageError("current directory is not accessible".to_string()),
        },
        Err(_) => Parsed::UsageError("current directory is not accessible".to_string()),
    }
}

fn program_is_cli_alias(program: Option<&OsStr>) -> bool {
    program
        .map(Path::new)
        .and_then(Path::file_stem)
        .is_some_and(|stem| stem.eq_ignore_ascii_case(CLI_COMMAND_NAME))
}

/// Canonicalize and validate every requested path. Each argument must be an
/// existing directory.
fn resolve_paths(requested: &[OsString]) -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut resolved = Vec::with_capacity(requested.len());
    for request in requested {
        resolved.push(resolve_path(request, &cwd)?);
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
    Ok(normalize_extended_path(&canonical)
        .to_string_lossy()
        .into_owned())
}

/// Resolve paths received from a second launch. The secondary process already
/// validated its arguments, so failures are reported to the caller instead of
/// terminating it.
pub fn resolve_forwarded(raw: &[String], cwd: &Path) -> Result<Vec<String>, String> {
    let mut resolved = Vec::with_capacity(raw.len());
    for request in raw {
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
        resolved.push(
            normalize_extended_path(&canonical)
                .to_string_lossy()
                .into_owned(),
        );
    }
    Ok(resolved)
}

pub fn print_help() {
    println!(
        "{} - Git GUI client

Usage:
  {CLI_COMMAND_NAME} [OPTIONS] [PATH]...
  {binary} [OPTIONS] [PATH]...

Opens each PATH as a repository tab. When an application window is already
running, the paths are forwarded to it; otherwise a new window opens with
them. Running `{CLI_COMMAND_NAME}` without arguments opens the current
directory.

Options:
  -h, --help     Print this help and exit
  -V, --version  Print version information and exit",
        build_info::version_line(),
        binary = build_info::APP_BINARY,
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
        let cwd = std::env::current_dir().unwrap();
        let assets = cwd.join("i18n");
        let parsed = parse(Some(&os(CLI_COMMAND_NAME)), &[os("i18n")]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(invocation.paths, vec![assets.to_string_lossy().into_owned()]);
    }

    #[test]
    fn the_desktop_binary_does_not_open_its_working_directory() {
        let parsed = parse(Some(&os(build_info::APP_BINARY)), &[]);
        assert_eq!(parsed, Parsed::Run(CliInvocation::default()));
    }

    #[test]
    fn the_alias_opens_the_current_directory() {
        let parsed = parse(Some(&os(CLI_COMMAND_NAME)), &[]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(invocation.paths.len(), 1);
        assert!(std::path::Path::new(&invocation.paths[0]).is_dir());
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert_eq!(parse(Some(&os("x")), &[os("--help")]), Parsed::Help);
        assert_eq!(parse(Some(&os("x")), &[os("-h")]), Parsed::Help);
        assert_eq!(parse(Some(&os("x")), &[os("--version")]), Parsed::Version);
        assert_eq!(parse(Some(&os("x")), &[os("-V")]), Parsed::Version);
    }

    #[test]
    fn unknown_options_and_missing_paths_are_usage_errors() {
        assert!(matches!(
            parse(Some(&os("x")), &[os("--nope")]),
            Parsed::UsageError(_)
        ));
        assert!(matches!(
            parse(Some(&os("x")), &[os("definitely-missing-directory")]),
            Parsed::UsageError(_)
        ));
        assert!(matches!(
            parse(Some(&os("x")), &[os("")]),
            Parsed::UsageError(_)
        ));
    }

    #[test]
    fn multiple_paths_are_accepted() {
        let parsed = parse(Some(&os("x")), &[os("i18n"), os("src")]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(invocation.paths.len(), 2);
    }

    #[test]
    fn a_file_argument_is_rejected() {
        assert!(matches!(
            parse(Some(&os("x")), &[os("keymap.default.json")]),
            Parsed::UsageError(_)
        ));
    }
}
