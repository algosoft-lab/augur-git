//! Loopback-only Git askpass bridge for HTTPS prompts.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use augur_core::git::location::{GitAskpassEnvironment, configure_git_askpass};

use crate::events::LFS_CREDENTIAL_REQUEST_EVENT;

const MAX_FRAME_BYTES: usize = 16 * 1024;
static PENDING_RESPONSES: OnceLock<Mutex<HashMap<String, mpsc::Sender<Option<String>>>>> =
    OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRequestEvent {
    pub request_id: String,
    pub prompt: String,
}

pub struct AskpassServer {
    running: Arc<AtomicBool>,
    listener_thread: Mutex<Option<JoinHandle<()>>>,
}

impl AskpassServer {
    pub fn start(app: AppHandle) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .to_string();
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let thread_token = token.clone();
        let listener_thread = thread::Builder::new()
            .name("augur-git-askpass".into())
            .spawn(move || {
                while thread_running.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let app = app.clone();
                            let token = thread_token.clone();
                            let running = thread_running.clone();
                            thread::spawn(move || handle_request(app, token, running, stream));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(30));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        configure_git_askpass(Some(GitAskpassEnvironment {
            executable: executable.to_string_lossy().into_owned(),
            address,
            token,
        }));
        Ok(Self {
            running,
            listener_thread: Mutex::new(Some(listener_thread)),
        })
    }
}

impl Drop for AskpassServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        configure_git_askpass(None);
        if let Some(thread) = self
            .listener_thread
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = thread.join();
        }
    }
}

fn handle_request(app: AppHandle, token: String, running: Arc<AtomicBool>, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(received_token) = read_frame(&mut stream) else {
        return;
    };
    if received_token != token.as_bytes() {
        return;
    }
    let Ok(prompt) = read_frame(&mut stream) else {
        return;
    };
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(180)));
    let prompt = String::from_utf8_lossy(&prompt).into_owned();
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let (sender, receiver) = mpsc::channel();
    let pending = PENDING_RESPONSES.get_or_init(|| Mutex::new(HashMap::new()));
    pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(request_id.clone(), sender);
    let _ = app.emit(
        LFS_CREDENTIAL_REQUEST_EVENT,
        CredentialRequestEvent {
            request_id: request_id.clone(),
            prompt,
        },
    );
    let response = loop {
        if !running.load(Ordering::Acquire) {
            break None;
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => break value,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break None,
        }
    };
    pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&request_id);
    match response {
        Some(value) if value.len() <= MAX_FRAME_BYTES && !value.contains('\0') => {
            let _ = write_frame(&mut stream, value.as_bytes());
        }
        _ => {
            let _ = write_frame(&mut stream, &[]);
        }
    }
}

pub fn respond(request_id: &str, response: Option<String>) -> bool {
    let Some(pending) = PENDING_RESPONSES.get() else {
        return false;
    };
    let sender = pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(request_id);
    sender.is_some_and(|sender| sender.send(response).is_ok())
}

/// Entry point used when Git launches this application as its askpass helper.
pub fn run_helper() -> Result<(), String> {
    let address = std::env::var("AUGUR_GIT_ASKPASS_ADDR")
        .map_err(|_| "Git askpass is not configured".to_string())?;
    let token = std::env::var("AUGUR_GIT_ASKPASS_TOKEN")
        .map_err(|_| "Git askpass is not configured".to_string())?;
    let prompt = std::env::args_os()
        .nth(1)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    write_frame(&mut stream, token.as_bytes())?;
    write_frame(&mut stream, prompt.as_bytes())?;
    let response = read_frame(&mut stream)?;
    if response.is_empty() {
        return Err("Git credential prompt was cancelled".into());
    }
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    output
        .write_all(&response)
        .and_then(|()| output.write_all(b"\n"))
        .map_err(|error| error.to_string())
}

fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|error| error.to_string())?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME_BYTES {
        return Err("Git askpass message exceeds the size limit".into());
    }
    let mut frame = vec![0; length];
    stream
        .read_exact(&mut frame)
        .map_err(|error| error.to_string())?;
    Ok(frame)
}

fn write_frame(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), String> {
    let length = u32::try_from(bytes.len()).map_err(|error| error.to_string())?;
    stream
        .write_all(&length.to_be_bytes())
        .and_then(|()| stream.write_all(bytes))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_without_interpreting_credential_contents() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_frame(&mut stream, b"secret with spaces").unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        assert_eq!(read_frame(&mut client).unwrap(), b"secret with spaces");
        handle.join().unwrap();
    }
}
