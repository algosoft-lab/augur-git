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
    eprintln!("agit installation is currently supported on macOS and Linux.");
    std::process::exit(1);
}
