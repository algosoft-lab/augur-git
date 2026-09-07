//! Command-line interface parsing.
//!
//! Both shipped binaries (`augur-git` and `augurgit`) accept repository
//! paths as arguments. Paths are validated and canonicalized before the
//! application starts so a typo cannot spawn the GUI. Only local directories
//! are supported; WSL locations are not reachable from the command line.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::core::build_info;
use crate::core::paths::normalize_extended_path;

/// Alias binary name whose bare invocation opens the current directory.
pub const CLI_COMMAND_NAME: &str = "augurgit";

/// A parsed command line: canonicalized repository paths to open.
pub struct CliInvocation {
    pub paths: Vec<String>,
}

/// Outcome of parsing arguments without process-level side effects.
enum Parsed {
    Run(CliInvocation),
    Help,
    Version,
    UsageError(String),
}

/// Parse `std::env::args_os`, exiting the process for `--help`, `--version`,
/// and usage errors. This is the single entry point used by both binaries.
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
            println!("{}", version_line());
            std::process::exit(0);
        }
        Parsed::UsageError(message) => {
            eprintln!("{CLI_COMMAND_NAME}: {message}");
            eprintln!("Try '{CLI_COMMAND_NAME} --help' for more information.");
            std::process::exit(2);
        }
    }
}

/// Parse a program name and argument list. Pure apart from filesystem access
/// for path validation, so it is unit-testable.
fn parse(program: Option<&OsStr>, args: &[OsString]) -> Parsed {
    let mut requested = Vec::new();
    for arg in args {
        let lossy = arg.to_string_lossy();
        if lossy.starts_with('-') && lossy != "-" {
            match lossy.as_ref() {
                "-h" | "--help" => return Parsed::Help,
                "-V" | "--version" => return Parsed::Version,
                _ => {
                    return Parsed::UsageError(format!(
                        "unrecognized option '{lossy}'"
                    ));
                }
            }
        }
        if lossy.is_empty() {
            return Parsed::UsageError("empty path argument".to_string());
        }
        requested.push(arg.clone());
    }

    // A bare `augurgit` invocation means "open the directory I am in". The
    // GUI binaries launched from a dock or file manager carry no arguments
    // and must not grow a repository tab for whatever their working directory
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
            _ => Parsed::UsageError(
                "current directory is not accessible".to_string(),
            ),
        },
        Err(_) => Parsed::UsageError(
            "current directory is not accessible".to_string(),
        ),
    }
}

fn program_is_cli_alias(program: Option<&OsStr>) -> bool {
    program
        .map(Path::new)
        .and_then(Path::file_stem)
        .is_some_and(|stem| stem.eq_ignore_ascii_case(CLI_COMMAND_NAME))
}

/// Canonicalize and validate every requested path. Every argument must be an
/// existing directory so a typo is reported before the GUI starts.
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
    let canonical = std::fs::canonicalize(&absolute).map_err(|_| {
        format!("path '{}' does not exist", candidate.to_string_lossy())
    })?;
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

fn version_line() -> String {
    let commit = crate::core::build_info::GIT_COMMIT;
    if commit == "unknown" {
        format!("{} {}", build_info::APP_NAME, build_info::APP_VERSION)
    } else {
        format!(
            "{} {} ({commit})",
            build_info::APP_NAME,
            build_info::APP_VERSION
        )
    }
}

fn print_help() {
    println!(
        "{} - Git GUI client

Usage:
  {CLI_COMMAND_NAME} [OPTIONS] [PATH]...
  augur-git [OPTIONS] [PATH]...

Opens each PATH as a repository tab. When an application window is already
running, the paths are forwarded to it; otherwise a new window opens with
them. Running `{CLI_COMMAND_NAME}` without arguments opens the current
directory.

Options:
  -h, --help     Print this help and exit
  -V, --version  Print version information and exit",
        version_line()
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
        let assets = cwd.join("assets");
        let parsed = parse(Some(&os("augurgit")), &[os("assets")]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(
            invocation.paths,
            vec![assets.to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn bare_alias_defaults_to_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let parsed = parse(Some(&os("/usr/local/bin/augurgit")), &[]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(invocation.paths, vec![cwd.to_string_lossy().into_owned()]);
    }

    #[test]
    fn bare_alias_is_case_insensitive() {
        let parsed = parse(Some(&os("AUGURGIT.EXE")), &[]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert!(!invocation.paths.is_empty());
    }

    #[test]
    fn bare_gui_binary_opens_nothing() {
        let parsed = parse(Some(&os("augur-git")), &[]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert!(invocation.paths.is_empty());
    }

    #[test]
    fn missing_path_is_a_usage_error() {
        let parsed = parse(Some(&os("augurgit")), &[os("does/not/exist")]);
        assert!(
            matches!(parsed, Parsed::UsageError(message) if message.contains("does not exist"))
        );
    }

    #[test]
    fn file_path_is_rejected() {
        let parsed = parse(Some(&os("augurgit")), &[os("Cargo.toml")]);
        assert!(
            matches!(parsed, Parsed::UsageError(message) if message.contains("is not a directory"))
        );
    }

    #[test]
    fn unknown_option_is_a_usage_error() {
        let parsed = parse(Some(&os("augurgit")), &[os("--bogus")]);
        assert!(
            matches!(parsed, Parsed::UsageError(message) if message.contains("--bogus"))
        );
    }

    #[test]
    fn help_and_version_flags_are_recognized() {
        let help = parse(Some(&os("augurgit")), &[os("--help")]);
        assert!(matches!(help, Parsed::Help));
        let short_help = parse(Some(&os("augurgit")), &[os("-h")]);
        assert!(matches!(short_help, Parsed::Help));
        let version = parse(Some(&os("augurgit")), &[os("-V")]);
        assert!(matches!(version, Parsed::Version));
    }

    #[test]
    fn multiple_paths_are_all_resolved() {
        let parsed = parse(Some(&os("augurgit")), &[os("assets"), os("src")]);
        let Parsed::Run(invocation) = parsed else {
            panic!("expected a run invocation");
        };
        assert_eq!(invocation.paths.len(), 2);
    }
}
