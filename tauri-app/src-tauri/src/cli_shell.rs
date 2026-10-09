use std::ffi::{OsStr, OsString};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const FRAME: &[u8] = b"\x1eAGIT_PATH\x1f";
const MAX_OUTPUT: usize = 32 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);

use augur_core::cli_install::ShellDetection;

fn unknown(reason: &'static str) -> ShellDetection {
    ShellDetection {
        state: "unknown",
        shell: None,
        path: None,
        reason: Some(reason),
    }
}

pub fn detect(expected: &Path) -> ShellDetection {
    let Some(shell) = configured_shell() else {
        return unknown("shell-unavailable");
    };
    let Some(name) = shell.file_name().and_then(OsStr::to_str) else {
        return unknown("shell-unsupported");
    };
    let kind = match name {
        "bash" => "bash",
        "zsh" => "zsh",
        "fish" => "fish",
        _ => return unknown("shell-unsupported"),
    };
    match probe(
        &shell,
        kind,
        &std::env::var_os("PATH").unwrap_or_default(),
        TIMEOUT,
    ) {
        Ok(Some(path)) => ShellDetection {
            state: if same(&path, expected) {
                "available"
            } else {
                "conflict"
            },
            shell: Some(kind.to_owned()),
            path: Some(path),
            reason: None,
        },
        Ok(None) => ShellDetection {
            state: "not-found",
            shell: Some(kind.to_owned()),
            path: None,
            reason: None,
        },
        Err(reason) => ShellDetection {
            state: "unknown",
            shell: Some(kind.to_owned()),
            path: None,
            reason: Some(reason),
        },
    }
}

fn configured_shell() -> Option<PathBuf> {
    let shell = std::env::var_os("SHELL")
        .filter(|value| Path::new(value).is_absolute())
        .map(PathBuf::from)
        .filter(|path| executable(path));
    shell.or_else(default_shell)
}

fn default_shell() -> Option<PathBuf> {
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 16 * 1024];
    let code = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if code != 0 || result.is_null() || entry.pw_shell.is_null() {
        return None;
    }
    let path = PathBuf::from(OsString::from_vec(unsafe {
        std::ffi::CStr::from_ptr(entry.pw_shell).to_bytes().to_vec()
    }));
    (path.is_absolute() && executable(&path)).then_some(path)
}

fn executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn same(left: &Path, right: &Path) -> bool {
    std::fs::canonicalize(left)
        .ok()
        .zip(std::fs::canonicalize(right).ok())
        .is_some_and(|(left, right)| left == right)
}

fn probe(
    shell: &Path,
    kind: &str,
    inherited_path: &OsStr,
    timeout: Duration,
) -> Result<Option<PathBuf>, &'static str> {
    let mut command = Command::new(shell);
    let script = match kind {
        "bash" => {
            command.args(["--login", "--interactive", "-c"]);
            "printf '\\036AGIT_PATH\\037%s\\000' \"$PATH\""
        }
        "zsh" => {
            command.args(["-l", "-i", "-c"]);
            "printf '\\036AGIT_PATH\\037%s\\000' \"$PATH\""
        }
        "fish" => {
            command.args(["--login", "--interactive", "--command"]);
            "printf '\\036AGIT_PATH\\037%s\\000' (string join : $PATH)"
        }
        _ => return Err("shell-unsupported"),
    };
    command
        .arg(script)
        .env("PATH", inherited_path)
        .current_dir(
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute())
                .unwrap_or_else(|| PathBuf::from("/")),
        )
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().map_err(|_| "shell-start-failed")?;
    let pid = child.id() as libc::pid_t;
    let stdout = child.stdout.take().ok_or("shell-output-unavailable")?;
    let fd = stdout.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        terminate_group(pid);
        let _ = child.wait();
        return Err("shell-output-unavailable");
    }
    let mut output = Vec::with_capacity(MAX_OUTPUT.min(4096));
    let started = Instant::now();
    let status = loop {
        if let Err(reason) = read_available(fd, &mut output) {
            terminate_group(pid);
            let _ = child.wait();
            return Err(reason);
        }
        if let Some(status) = child.try_wait().map_err(|_| "shell-wait-failed")? {
            let _ = read_available(fd, &mut output);
            break status;
        }
        if started.elapsed() >= timeout {
            terminate_group(pid);
            let _ = child.wait();
            return Err("shell-timeout");
        }
        thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err("shell-exited-with-error");
    }
    let path = parse_frame(&output)?;
    let Some(path) = path else {
        return Ok(None);
    };
    Ok(std::env::split_paths(&path)
        .map(|directory| directory.join("agit"))
        .find(|candidate| executable(candidate)))
}

fn read_available(fd: libc::c_int, output: &mut Vec<u8>) -> Result<(), &'static str> {
    let mut buffer = [0; 4096];
    loop {
        let remaining = MAX_OUTPUT + 1 - output.len();
        let limit = buffer.len().min(remaining);
        let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), limit) };
        if count > 0 {
            output.extend_from_slice(&buffer[..count as usize]);
            if output.len() > MAX_OUTPUT {
                return Err("shell-output-too-large");
            }
            continue;
        }
        if count == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EAGAIN) => return Ok(()),
            _ => return Err("shell-output-failed"),
        }
    }
}

fn terminate_group(pid: libc::pid_t) {
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

fn parse_frame(output: &[u8]) -> Result<Option<OsString>, &'static str> {
    let Some(start) = output
        .windows(FRAME.len())
        .rposition(|window| window == FRAME)
    else {
        return Err("shell-output-invalid");
    };
    let value = &output[start + FRAME.len()..];
    let Some(end) = value.iter().position(|byte| *byte == 0) else {
        return Err("shell-output-invalid");
    };
    if value[end + 1..]
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return Err("shell-output-invalid");
    }
    if end == 0 {
        return Ok(None);
    }
    Ok(Some(OsString::from_vec(value[..end].to_vec())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn parses_a_framed_path_after_shell_startup_noise() {
        let mut output = b"shell startup noise\n".to_vec();
        output.extend_from_slice(FRAME);
        output.extend_from_slice(b"/bin:/usr/bin\0");
        assert_eq!(
            parse_frame(&output).unwrap(),
            Some(OsString::from("/bin:/usr/bin"))
        );
    }

    #[test]
    fn rejects_missing_or_ambiguous_frames() {
        assert_eq!(parse_frame(b"noise"), Err("shell-output-invalid"));
        let mut output = FRAME.to_vec();
        output.extend_from_slice(b"/bin\0unexpected");
        assert_eq!(parse_frame(&output), Err("shell-output-invalid"));
    }

    #[test]
    fn probes_an_isolated_shell_and_finds_the_first_executable_command() {
        let root = fixture();
        let shell = root.join("zsh");
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        write_executable(
            &shell,
            r#"#!/bin/sh
printf '\036AGIT_PATH\037%s\000' "$PATH"
"#,
        );
        write_executable(&first.join("agit"), "#!/bin/sh\nexit 0\n");
        write_executable(&second.join("agit"), "#!/bin/sh\nexit 0\n");
        let path = std::env::join_paths([&first, &second]).unwrap();
        assert_eq!(
            probe(&shell, "zsh", &path, Duration::from_secs(1)).unwrap(),
            Some(first.join("agit"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_shell_output_over_the_limit() {
        let root = fixture();
        std::fs::create_dir_all(&root).unwrap();
        let shell = root.join("bash");
        write_executable(&shell, "#!/bin/sh\nprintf '%40000s' x\n");
        assert_eq!(
            probe(
                &shell,
                "bash",
                OsStr::new("/bin:/usr/bin"),
                Duration::from_secs(1)
            ),
            Err("shell-output-too-large")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn times_out_and_kills_the_shell_process_group() {
        let root = fixture();
        std::fs::create_dir_all(&root).unwrap();
        let shell = root.join("bash");
        write_executable(&shell, "#!/bin/sh\nsleep 10\n");
        let started = Instant::now();
        assert_eq!(
            probe(
                &shell,
                "bash",
                OsStr::new("/bin:/usr/bin"),
                Duration::from_millis(100)
            ),
            Err("shell-timeout")
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(root);
    }

    fn fixture() -> PathBuf {
        std::env::temp_dir().join(format!(
            "augur-shell-probe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn write_executable(path: &Path, contents: &str) {
        std::fs::write(path, contents).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
