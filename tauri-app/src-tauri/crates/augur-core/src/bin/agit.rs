//! A terminal launcher that does not link the desktop runtime.
#[cfg(unix)]
fn main() {
    use augur_core::{build_info, cli, cli_install};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let invocation = match cli::parse_launcher(&args) {
        cli::Parsed::Help => {
            cli::print_help_for("agit");
            return;
        }
        cli::Parsed::Version => {
            println!("{}", build_info::version_line());
            return;
        }
        cli::Parsed::UsageError(error) => {
            eprintln!("agit: {error}\nTry 'agit --help' for more information.");
            std::process::exit(2);
        }
        cli::Parsed::Run(invocation) => invocation,
    };
    let result = (|| -> Result<(), String> {
        let executable = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(|error| error.to_string())?;
        let target = cli_install::launch_target(&executable)?;
        let mut command = Command::new(target);
        command
            .arg("--")
            .args(invocation.paths)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // A new session survives terminal closure and does not receive its Ctrl-C.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command
            .spawn()
            .map_err(|error| format!("could not launch Augur Git: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("agit: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(unix))]
fn main() {
    #[cfg(windows)]
    windows_main();
    #[cfg(not(windows))]
    {
        eprintln!("agit installation is currently supported on macOS and Linux.");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn windows_main() {
    use augur_core::{build_info, cli, cli_install};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [flag] if flag == "--internal-cli-register" => {
            if let Err(error) = cli_install::install_from_launcher() {
                eprintln!("agit: could not register the command: {error}");
                std::process::exit(1);
            }
            return;
        }
        [flag] if flag == "--internal-cli-unregister" => {
            if let Err(error) = cli_install::uninstall_from_launcher() {
                eprintln!("agit: could not unregister the command: {error}");
                std::process::exit(1);
            }
            return;
        }
        [flag] if flag == "--internal-cli-uninstall-package" => {
            if let Err(error) = cli_install::uninstall_package_from_launcher() {
                eprintln!("agit: could not clean up the command: {error}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }

    let invocation = match cli::parse_launcher(&args) {
        cli::Parsed::Help => {
            cli::print_help_for("agit");
            return;
        }
        cli::Parsed::Version => {
            println!("{}", build_info::version_line());
            return;
        }
        cli::Parsed::UsageError(error) => {
            eprintln!("agit: {error}\nTry 'agit --help' for more information.");
            std::process::exit(2);
        }
        cli::Parsed::Run(invocation) => invocation,
    };

    let result = (|| -> Result<(), String> {
        let context = cli_install::Context::from_env()?;
        let target = context.launch_target()?;
        let mut command = Command::new(target);
        command
            .arg("--")
            .args(invocation.paths)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        command
            .spawn()
            .map_err(|error| format!("could not launch Augur Git: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("agit: {error}");
        std::process::exit(1);
    }
}
