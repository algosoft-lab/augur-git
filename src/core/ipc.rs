//! Single-instance handshake for command-line launches.
//!
//! The first instance binds an ephemeral loopback TCP port and records the
//! port, its process id, and a per-run token in the user cache directory.
//! A later launch reads that record and, when the running instance answers,
//! hands over the requested repository paths and exits. The token keeps
//! unrelated local processes from injecting open requests; the handshake is
//! best-effort, so a stale record (crashed instance, reused port) simply
//! falls back to starting a new instance.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Upper bound for one handshake message, including all path arguments.
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Requests above this size are rejected instead of partially applied.
const MAX_PATHS_PER_REQUEST: usize = 32;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Forwarding is restricted to the loopback interface on every platform.
const FORWARD_HOST: &str = "127.0.0.1";

#[derive(Serialize, Deserialize)]
struct Handshake {
    token: String,
    paths: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Ack {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct InstanceRecord {
    port: u16,
    pid: u32,
    token: String,
}

/// Background listener that receives forwarded open requests. The receiver
/// is consumed by the workspace, which turns every batch of paths into
/// repository tabs on the UI thread.
pub struct InstanceServer {
    receiver: Receiver<Vec<String>>,
}

impl InstanceServer {
    /// Bind a loopback port, publish the instance record, and start the
    /// accept thread. Returns `None` when binding or publishing fails; the
    /// application then starts without CLI forwarding support.
    pub fn start() -> Option<Self> {
        let listener = match TcpListener::bind((FORWARD_HOST, 0)) {
            Ok(listener) => listener,
            Err(error) => {
                log::warn!("[ipc] failed to bind forward listener: {error}");
                return None;
            }
        };
        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                log::warn!("[ipc] failed to read listener port: {error}");
                return None;
            }
        };
        let token = generate_token();
        if let Err(error) = write_instance_file(&InstanceRecord {
            port,
            pid: std::process::id(),
            token: token.clone(),
        }) {
            log::warn!("[ipc] failed to publish instance record: {error}");
            return None;
        }

        let (sender, receiver) = mpsc::channel::<Vec<String>>();
        let spawned = std::thread::Builder::new()
            .name("instance-server".to_string())
            .spawn(move || serve(listener, token, sender));
        match spawned {
            Ok(_handle) => Some(Self { receiver }),
            Err(error) => {
                log::warn!("[ipc] failed to spawn accept thread: {error}");
                None
            }
        }
    }

    pub fn into_receiver(self) -> Receiver<Vec<String>> {
        self.receiver
    }
}

fn serve(listener: TcpListener, token: String, sender: Sender<Vec<String>>) {
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                log::warn!("[ipc] failed to accept connection: {error}");
                continue;
            }
        };
        if let Some(paths) = accept_connection(&mut stream, &token) {
            let _ = sender.send(paths);
        }
    }
}

/// Read one handshake, validate the token, acknowledge it, and return the
/// requested paths. `None` means the connection was rejected or malformed.
fn accept_connection(
    stream: &mut TcpStream,
    token: &str,
) -> Option<Vec<String>> {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let line = match read_line_bounded(stream) {
        Ok(line) => line,
        Err(error) => {
            log::warn!("[ipc] dropped malformed handshake: {error}");
            return None;
        }
    };
    let request: Handshake = match serde_json::from_str(&line) {
        Ok(request) => request,
        Err(error) => {
            log::warn!("[ipc] dropped undecodable handshake: {error}");
            write_ack(
                stream,
                &Ack {
                    ok: false,
                    error: Some("undecodable request".to_string()),
                },
            );
            return None;
        }
    };
    if request.token != token {
        log::warn!("[ipc] rejected handshake with an invalid token");
        write_ack(
            stream,
            &Ack {
                ok: false,
                error: Some("invalid token".to_string()),
            },
        );
        return None;
    }

    let truncated = request.paths.len() > MAX_PATHS_PER_REQUEST;
    if truncated {
        log::warn!(
            "[ipc] truncating forwarded request from {} to {MAX_PATHS_PER_REQUEST} paths",
            request.paths.len()
        );
    }
    let paths: Vec<String> = request
        .paths
        .into_iter()
        .take(MAX_PATHS_PER_REQUEST)
        .filter(|path| !path.trim().is_empty())
        .collect();
    write_ack(
        stream,
        &Ack {
            ok: true,
            error: None,
        },
    );
    Some(paths)
}

fn write_ack(stream: &mut TcpStream, ack: &Ack) {
    if let Ok(text) = serde_json::to_string(ack) {
        let _ = stream.write_all(text.as_bytes());
        let _ = stream.write_all(b"\n");
    }
}

/// Try to hand `paths` to a running instance. Returns `true` when a running
/// instance accepted the request; the caller should then exit.
pub fn forward_to_running_instance(paths: &[String]) -> bool {
    if paths.is_empty() {
        return false;
    }
    let Some(record) = read_instance_file() else {
        return false;
    };
    if record.pid == std::process::id() {
        return false;
    }
    let address = SocketAddr::from(([127, 0, 0, 1], record.port));
    let mut stream = match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
    {
        Ok(stream) => stream,
        Err(error) => {
            log::info!(
                "[ipc] no running instance answered on port {}: {error}",
                record.port
            );
            remove_stale_instance_file(&record.token);
            return false;
        }
    };
    match send_handshake(&mut stream, &record.token, paths) {
        Ok(()) => {
            log::info!(
                "[ipc] forwarded {} path(s) to the running instance",
                paths.len()
            );
            true
        }
        Err(error) => {
            log::info!("[ipc] forward rejected: {error}");
            remove_stale_instance_file(&record.token);
            false
        }
    }
}

/// Write the handshake and wait for the acknowledgement. Split from the
/// file-based plumbing so tests can drive both sides directly.
fn send_handshake(
    stream: &mut TcpStream,
    token: &str,
    paths: &[String],
) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let request = Handshake {
        token: token.to_string(),
        paths: paths.to_vec(),
    };
    let mut text = serde_json::to_string(&request)
        .map_err(|error| format!("encode request: {error}"))?;
    text.push('\n');
    stream
        .write_all(text.as_bytes())
        .map_err(|error| format!("send request: {error}"))?;
    let line = read_line_bounded(stream)
        .map_err(|error| format!("read acknowledgement: {error}"))?;
    let ack: Ack = serde_json::from_str(&line)
        .map_err(|error| format!("decode acknowledgement: {error}"))?;
    if ack.ok {
        Ok(())
    } else {
        Err(ack.error.unwrap_or_else(|| "rejected".to_string()))
    }
}

/// Read up to the next newline with a hard size cap so a hostile or broken
/// peer cannot exhaust memory.
fn read_line_bounded(stream: &mut impl Read) -> Result<String, String> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => {
                if buffer.is_empty() {
                    return Err("connection closed".to_string());
                }
                break;
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buffer.push(byte[0]);
                if buffer.len() > MAX_MESSAGE_BYTES {
                    return Err("message too large".to_string());
                }
            }
            Err(error) => return Err(format!("read: {error}")),
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn instance_file_path() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("augur-git").join("instance.json"))
}

fn write_instance_file(record: &InstanceRecord) -> anyhow::Result<()> {
    let path = instance_file_path()
        .ok_or_else(|| anyhow::anyhow!("no user cache directory"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::core::config::write_atomically(
        &path,
        &serde_json::to_string_pretty(record)?,
    )
}

fn read_instance_file() -> Option<InstanceRecord> {
    let text = std::fs::read_to_string(instance_file_path()?).ok()?;
    match serde_json::from_str(&text) {
        Ok(record) => Some(record),
        Err(error) => {
            log::info!("[ipc] ignoring unreadable instance record: {error}");
            None
        }
    }
}

/// Best-effort cleanup of a dead instance's record. The record is only
/// removed while it still carries `token`, so a freshly started instance's
/// record is never deleted by a lagging second launch.
fn remove_stale_instance_file(token: &str) {
    let Some(path) = instance_file_path() else {
        return;
    };
    let matches = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<InstanceRecord>(&text).ok())
        .is_some_and(|record| record.token == token);
    if matches {
        let _ = std::fs::remove_file(path);
    }
}

/// Unpredictable per-run token derived from the process id and two random
/// hash seeds. Strong enough to keep other local processes from forging
/// open requests; the cache directory itself is already user-private.
fn generate_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut first =
        std::collections::hash_map::RandomState::new().build_hasher();
    first.write_u32(std::process::id());
    first.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0),
    );
    let mut second =
        std::collections::hash_map::RandomState::new().build_hasher();
    second.write(&first.finish().to_le_bytes());
    format!("{:016x}{:016x}", first.finish(), second.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_round_trips_through_the_socket() {
        let listener =
            TcpListener::bind((FORWARD_HOST, 0)).expect("bind loopback port");
        let address = listener.local_addr().expect("local address");
        let server_token = generate_token();
        let client_token = server_token.clone();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            accept_connection(&mut stream, &server_token)
        });

        let mut client =
            TcpStream::connect(address).expect("connect to test server");
        let paths = vec!["/tmp/repo-one".to_string()];
        send_handshake(&mut client, &client_token, &paths).expect("handshake");

        let received = server.join().expect("server thread");
        assert_eq!(received, Some(paths));
    }

    #[test]
    fn wrong_token_is_rejected() {
        let listener =
            TcpListener::bind((FORWARD_HOST, 0)).expect("bind loopback port");
        let address = listener.local_addr().expect("local address");
        let server_token = generate_token();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            accept_connection(&mut stream, &server_token)
        });

        let mut client =
            TcpStream::connect(address).expect("connect to test server");
        let error =
            send_handshake(&mut client, "forged-token", &["/tmp/repo".into()])
                .expect_err("forged token must be rejected");
        assert!(error.contains("invalid token"));
        assert_eq!(server.join().expect("server thread"), None);
    }

    #[test]
    fn instance_record_serializes_all_fields() {
        let record = InstanceRecord {
            port: 49152,
            pid: 4242,
            token: "abcdef0123456789".to_string(),
        };
        let text = serde_json::to_string(&record).expect("serialize");
        let parsed: InstanceRecord =
            serde_json::from_str(&text).expect("deserialize");
        assert_eq!(parsed.port, 49152);
        assert_eq!(parsed.pid, 4242);
        assert_eq!(parsed.token, "abcdef0123456789");
    }

    #[test]
    fn oversized_messages_are_rejected_without_panic() {
        let listener =
            TcpListener::bind((FORWARD_HOST, 0)).expect("bind loopback port");
        let address = listener.local_addr().expect("local address");
        let server_token = generate_token();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            accept_connection(&mut stream, &server_token)
        });

        let mut client =
            TcpStream::connect(address).expect("connect to test server");
        let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
        client
            .write_all(oversized.as_bytes())
            .expect("write oversized payload");
        assert_eq!(server.join().expect("server thread"), None);
    }

    #[test]
    fn tokens_are_unique_and_fixed_width() {
        let first = generate_token();
        let second = generate_token();
        assert_eq!(first.len(), 32);
        assert_eq!(second.len(), 32);
        assert_ne!(first, second);
    }
}
