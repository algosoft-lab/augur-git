//! Loopback-only Git askpass bridge.
//!
//! The bridge is shared by every frontend: Git subprocesses talk to a loopback
//! TCP listener, and each prompt is answered from the credential vault, from a
//! form the user fills in, or from the one-shot channel the previous
//! implementation used for prompts this module does not understand.
//!
//! Three rules keep it safe:
//!
//! - a prompt is answered from storage at most once per Git invocation, so a
//!   rejected password cannot be retried in a loop;
//! - a second password prompt for the same invocation marks the entry that was
//!   used as rejected, because Git asks again after the server refused it;
//! - anything the prompt parser does not recognise keeps the plain
//!   "show one field, remember nothing" behaviour.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use augur_core::config::LocationConfig;
use augur_core::git::location::{GitAskpassEnvironment, configure_git_askpass};

use crate::credentials::RepositoryIdentity;
use crate::credentials::model::{CredentialInput, CredentialScope, SecretStorage};
use crate::credentials::prompt::{PromptContext, PromptKind, RemoteTarget, parse_prompt};
use crate::credentials::vault::{CredentialVault, now_seconds};
use crate::events::LFS_CREDENTIAL_REQUEST_EVENT;
use crate::sink::{EventSink, EventTarget, emit};

const MAX_FRAME_BYTES: usize = 16 * 1024;
/// How long a login form may stay open before its streams are dropped.
const SESSION_TIMEOUT: Duration = Duration::from_secs(300);
/// How long an answered credential is remembered for rejection detection.
const SERVED_TIMEOUT: Duration = Duration::from_secs(600);

static PENDING_RESPONSES: OnceLock<Mutex<HashMap<String, mpsc::Sender<Option<String>>>>> =
    OnceLock::new();
static LOGIN_SESSIONS: OnceLock<Mutex<HashMap<String, LoginSession>>> = OnceLock::new();
/// Credentials already handed to one Git invocation, keyed by its operation id.
static SERVED: OnceLock<Mutex<HashMap<String, ServedCredential>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRequestEvent {
    pub request_id: String,
    /// `login` for a username/password form, `prompt` for a raw Git prompt.
    pub kind: String,
    /// The original Git prompt, always present for diagnostics.
    pub prompt: String,
    pub protocol: Option<String>,
    pub host: Option<String>,
    /// Username Git already named in the URL, when it did.
    pub username: Option<String>,
    /// Repository path the Git command runs in.
    pub repository: Option<String>,
    /// Whether at least one persistent backend can take the secret.
    pub can_persist: bool,
    pub keychain_available: bool,
    pub plaintext_available: bool,
    /// Storage the dialog should preselect.
    pub default_storage: String,
    /// Whether an entry already exists for this host.
    pub stored: bool,
}

/// One answer to a username/password form.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialLoginAnswer {
    pub username: String,
    pub password: String,
    /// `None` keeps nothing. `Session` keeps it until the process exits.
    pub persist: Option<SecretStorage>,
    /// Store the entry for this repository only.
    pub repository_only: bool,
    pub cancel: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialLoginResult {
    /// Whether Git received an answer.
    pub delivered: bool,
    /// Whether the credential was written to a persistent backend.
    pub stored: bool,
    /// Why the credential could not be stored, when it could not.
    pub error: Option<String>,
}

struct ServedCredential {
    entry_id: String,
    answered_password: bool,
    recorded_at: Instant,
}

struct LoginSession {
    request_id: String,
    target: RemoteTarget,
    repository: Option<String>,
    username_waiter: Option<TcpStream>,
    password_waiter: Option<TcpStream>,
    reply: Option<CredentialLoginAnswer>,
    emitted: bool,
    answered_username: bool,
    answered_password: bool,
    opened_at: Instant,
}

impl LoginSession {
    fn new(request_id: String, target: RemoteTarget, repository: Option<String>) -> Self {
        Self {
            request_id,
            target,
            repository,
            username_waiter: None,
            password_waiter: None,
            reply: None,
            emitted: false,
            answered_username: false,
            answered_password: false,
            opened_at: Instant::now(),
        }
    }

    fn attach(&mut self, kind: PromptKind, stream: TcpStream) {
        match kind {
            PromptKind::Username => self.username_waiter = Some(stream),
            PromptKind::Password => self.password_waiter = Some(stream),
            PromptKind::Other => {}
        }
    }

    /// Deliver whatever the user already typed, and report whether the session
    /// still waits for a prompt.
    fn drain(&mut self) {
        let Some(reply) = self.reply.clone() else {
            return;
        };
        let username = if reply.cancel {
            String::new()
        } else {
            reply.username.clone()
        };
        let password = if reply.cancel {
            String::new()
        } else {
            reply.password.clone()
        };
        if let Some(mut stream) = self.username_waiter.take() {
            let _ = write_frame(&mut stream, username.as_bytes());
            let _ = stream.flush();
            self.answered_username = true;
        }
        if let Some(mut stream) = self.password_waiter.take() {
            let _ = write_frame(&mut stream, password.as_bytes());
            let _ = stream.flush();
            self.answered_password = true;
        }
    }

    /// Whether the session is done.
    ///
    /// A username answer does not finish it: Git asks for the password from a
    /// second helper process right after the username arrives, and that prompt
    /// has to be answered from the same reply rather than with a new form.
    fn is_finished(&self) -> bool {
        let drained = self.username_waiter.is_none() && self.password_waiter.is_none();
        self.reply.is_some() && drained && (self.answered_password || !self.answered_username)
    }
}

pub struct AskpassServer {
    running: Arc<AtomicBool>,
    listener_thread: Mutex<Option<JoinHandle<()>>>,
    address: String,
    token: String,
}

impl AskpassServer {
    pub fn start(sink: Arc<dyn EventSink>, vault: Arc<CredentialVault>) -> Result<Self, String> {
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
                            let sink = sink.clone();
                            let token = thread_token.clone();
                            let running = thread_running.clone();
                            let vault = vault.clone();
                            thread::spawn(move || {
                                handle_request(sink, token, running, vault, stream)
                            });
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
            address: address.clone(),
            token: token.clone(),
        }));
        Ok(Self {
            running,
            listener_thread: Mutex::new(Some(listener_thread)),
            address,
            token,
        })
    }
}

impl AskpassServer {
    /// The loopback endpoint Git is pointed at, for diagnostics and tests.
    pub fn endpoint(&self) -> (&str, &str) {
        (&self.address, &self.token)
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

fn handle_request(
    sink: Arc<dyn EventSink>,
    token: String,
    running: Arc<AtomicBool>,
    vault: Arc<CredentialVault>,
    mut stream: TcpStream,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(received_token) = read_frame(&mut stream) else {
        return;
    };
    if received_token != token.as_bytes() {
        return;
    }
    let Ok(operation_id) = read_frame(&mut stream) else {
        return;
    };
    let Ok(repository) = read_frame(&mut stream) else {
        return;
    };
    let Ok(prompt) = read_frame(&mut stream) else {
        return;
    };
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let prompt = String::from_utf8_lossy(&prompt).into_owned();
    let operation_id = String::from_utf8_lossy(&operation_id).into_owned();
    let repository = String::from_utf8_lossy(&repository).into_owned();
    let repository = if repository.is_empty() {
        None
    } else {
        Some(repository)
    };

    let context = parse_prompt(&prompt);
    if !context.is_remote() {
        // Unknown prompts keep the one-field, remember-nothing behaviour.
        let request_id = uuid::Uuid::new_v4().simple().to_string();
        let (sender, receiver) = mpsc::channel();
        pending()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(request_id.clone(), sender);
        emit(
            sink.as_ref(),
            EventTarget::All,
            LFS_CREDENTIAL_REQUEST_EVENT,
            &CredentialRequestEvent {
                request_id: request_id.clone(),
                kind: "prompt".into(),
                prompt: prompt.clone(),
                protocol: context.protocol.clone(),
                host: context.host.clone(),
                username: context.username.clone(),
                repository: repository.clone(),
                can_persist: false,
                keychain_available: false,
                plaintext_available: false,
                default_storage: SecretStorage::Session.as_str().into(),
                stored: false,
            },
        );
        let response = wait_for_answer(&running, receiver);
        pending()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&request_id);
        let _ = write_frame(&mut stream, response.unwrap_or_default().as_bytes());
        return;
    }

    let target = RemoteTarget::from_prompt(
        &context,
        repository.as_ref().map(|path| RepositoryIdentity {
            location: LocationConfig::Local,
            path: path.clone(),
        }),
    );
    let Some(target) = target else {
        let _ = write_frame(&mut stream, b"");
        return;
    };
    let kind = context.kind();
    log::debug!(
        "[git_askpass] prompt: kind={} target={}://{}{} repository={} op={}",
        kind.as_str(),
        target.protocol,
        target.host,
        target
            .path
            .as_deref()
            .map(|path| format!("/{path}"))
            .unwrap_or_default(),
        repository.as_deref().unwrap_or("-"),
        operation_id
    );

    // A password prompt for an invocation whose stored password was already
    // used means the server refused it: mark the entry and stop offering it.
    let served = {
        let mut served = SERVED
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Every entry is one Git invocation that answered from storage. Most
        // never ask twice, so the map is pruned instead of only drained.
        served.retain(|_, value| value.recorded_at.elapsed() < SERVED_TIMEOUT);
        served.remove(&operation_id)
    };
    if let Some(served) = served
        && served.answered_password
        && kind != PromptKind::Username
    {
        log::info!(
            "[git_askpass] a password was asked again for op={operation_id}, so credential {} is marked failed",
            served.entry_id
        );
        vault.mark_failed(&served.entry_id, now_seconds());
    }

    if vault.auto_fill() {
        if let Some(resolved) = vault.resolve(&target, now_seconds()) {
            let answer = match kind {
                PromptKind::Username => Some(resolved.entry.username.clone()),
                PromptKind::Password => resolved.secret.clone(),
                PromptKind::Other => None,
            };
            if let Some(answer) = answer {
                log::debug!(
                    "[git_askpass] answered {} from credential {}",
                    kind.as_str(),
                    resolved.entry.id
                );
                let _ = write_frame(&mut stream, answer.as_bytes());
                SERVED
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(
                        operation_id,
                        ServedCredential {
                            entry_id: resolved.entry.id.clone(),
                            answered_password: kind == PromptKind::Password,
                            recorded_at: Instant::now(),
                        },
                    );
                return;
            }
        }
    }

    log::info!(
        "[git_askpass] asking the interface for {}://{}{}",
        target.protocol,
        target.host,
        target
            .path
            .as_deref()
            .map(|path| format!("/{path}"))
            .unwrap_or_default()
    );
    attach_login(
        sink,
        &vault,
        operation_id,
        context,
        target,
        repository,
        stream,
    );
}

fn wait_for_answer(
    running: &Arc<AtomicBool>,
    receiver: mpsc::Receiver<Option<String>>,
) -> Option<String> {
    loop {
        if !running.load(Ordering::Acquire) {
            return None;
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => return value,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn attach_login(
    sink: Arc<dyn EventSink>,
    vault: &Arc<CredentialVault>,
    operation_id: String,
    context: PromptContext,
    target: RemoteTarget,
    repository: Option<String>,
    stream: TcpStream,
) {
    let mut sessions = sessions_lock();
    sessions.retain(|_, session| session.opened_at.elapsed() < SESSION_TIMEOUT);
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let session = sessions
        .entry(operation_id)
        .or_insert_with(|| LoginSession::new(request_id, target.clone(), repository.clone()));
    session.attach(context.kind(), stream);
    if session.reply.is_some() {
        session.drain();
        if session.is_finished() {
            let request_id = session.request_id.clone();
            sessions.retain(|_, entry| entry.request_id != request_id);
        }
        return;
    }
    if session.emitted {
        return;
    }
    session.emitted = true;
    let request = CredentialRequestEvent {
        request_id: session.request_id.clone(),
        kind: "login".into(),
        prompt: String::new(),
        protocol: Some(target.protocol.clone()),
        host: Some(target.host.clone()),
        username: session
            .reply
            .as_ref()
            .map(|reply| reply.username.clone())
            .or_else(|| target.username.clone())
            .filter(|username| !username.is_empty()),
        repository: repository.clone(),
        can_persist: can_persist(vault),
        keychain_available: vault.secrets().available(SecretStorage::Keychain),
        plaintext_available: vault.settings().allow_plaintext
            && vault.secrets().available(SecretStorage::Plaintext),
        default_storage: default_storage(vault).as_str().into(),
        stored: vault.has_entry(&target),
    };
    drop(sessions);
    emit(
        sink.as_ref(),
        EventTarget::All,
        LFS_CREDENTIAL_REQUEST_EVENT,
        &request,
    );
}

fn can_persist(vault: &CredentialVault) -> bool {
    vault.secrets().available(SecretStorage::Keychain)
        || (vault.settings().allow_plaintext && vault.secrets().available(SecretStorage::Plaintext))
}

fn default_storage(vault: &CredentialVault) -> SecretStorage {
    let settings = vault.settings();
    match settings.default_storage {
        SecretStorage::Plaintext
            if !settings.allow_plaintext
                || !vault.secrets().available(SecretStorage::Plaintext) =>
        {
            SecretStorage::Session
        }
        SecretStorage::Keychain if !vault.secrets().available(SecretStorage::Keychain) => {
            SecretStorage::Session
        }
        other => other,
    }
}

fn sessions_lock() -> std::sync::MutexGuard<'static, HashMap<String, LoginSession>> {
    LOGIN_SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn pending() -> &'static Mutex<HashMap<String, mpsc::Sender<Option<String>>>> {
    PENDING_RESPONSES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Answer a raw prompt shown by the interface.
pub fn respond(request_id: &str, response: Option<String>) -> bool {
    let sender = pending()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(request_id);
    sender.is_some_and(|sender| sender.send(response).is_ok())
}

/// Answer a username/password form and, when asked, remember it.
pub fn respond_login(
    request_id: &str,
    answer: CredentialLoginAnswer,
    vault: &CredentialVault,
) -> CredentialLoginResult {
    let mut sessions = sessions_lock();
    let Some(operation_id) = sessions
        .iter()
        .find(|(_, session)| session.request_id == request_id)
        .map(|(operation_id, _)| operation_id.clone())
    else {
        return CredentialLoginResult {
            delivered: false,
            stored: false,
            error: Some("The credential prompt is no longer waiting".into()),
        };
    };
    let Some(session) = sessions.get_mut(&operation_id) else {
        return CredentialLoginResult {
            delivered: false,
            stored: false,
            error: None,
        };
    };
    let target = session.target.clone();
    let repository = session.repository.clone();
    session.reply = Some(answer.clone());
    session.drain();
    if session.is_finished() {
        sessions.remove(&operation_id);
    }
    drop(sessions);

    let delivered = !answer.cancel;
    if !delivered {
        return CredentialLoginResult {
            delivered: true,
            stored: false,
            error: None,
        };
    }
    let mut stored = false;
    let mut error = None;
    if let Some(persist) = answer.persist
        && !answer.password.is_empty()
    {
        let scope = match (&repository, answer.repository_only) {
            (Some(path), true) => CredentialScope::Repository {
                location: LocationConfig::Local,
                path: path.clone(),
            },
            _ => CredentialScope::Global,
        };
        let input = CredentialInput {
            id: None,
            protocol: target.protocol.clone(),
            host: target.host.clone(),
            path_prefix: None,
            username: answer.username.clone(),
            password: Some(answer.password.clone()),
            scope,
            storage: persist,
        };
        match vault.save(input, now_seconds()) {
            Ok(_) => stored = true,
            Err(reason) => {
                log::warn!("[git_askpass] the credential could not be stored: {reason}");
                error = Some(reason);
            }
        }
    }
    CredentialLoginResult {
        delivered,
        stored,
        error,
    }
}

/// Whether this process was started by Git as its askpass helper.
///
/// Git executes `GIT_ASKPASS` directly — no shell — and appends the prompt as
/// the only argument, so the value cannot carry a mode flag. The mode is
/// instead recognised from the bridge variables every spawned Git process
/// inherits. `--git-askpass` is still accepted, for a wrapper script that
/// names this binary with the flag.
pub fn is_helper_invocation() -> bool {
    let address = std::env::var("AUGUR_GIT_ASKPASS_ADDR").ok();
    let token = std::env::var("AUGUR_GIT_ASKPASS_TOKEN").ok();
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    helper_invocation(address.as_deref(), token.as_deref(), &arguments)
}

fn helper_invocation(address: Option<&str>, token: Option<&str>, arguments: &[OsString]) -> bool {
    let configured = address.is_some_and(|value| !value.trim().is_empty())
        && token.is_some_and(|value| !value.trim().is_empty());
    if !configured {
        return false;
    }
    match arguments {
        // Git passed the prompt. A flag here means this is a normal launch.
        [prompt] => prompt
            .to_str()
            .is_some_and(|value| !value.is_empty() && !value.starts_with('-')),
        [flag, prompt] => {
            flag == OsStr::new("--git-askpass")
                && prompt
                    .to_str()
                    .is_some_and(|value| !value.trim().is_empty())
        }
        _ => false,
    }
}

/// The Git prompt, with any legacy mode flag removed.
fn prompt_from_arguments(mut arguments: Vec<OsString>) -> String {
    arguments
        .iter()
        .position(|value| value == OsStr::new("--git-askpass"))
        .map(|index| arguments.remove(index + 1))
        .unwrap_or_else(|| arguments.into_iter().next().unwrap_or_default())
        .to_string_lossy()
        .into_owned()
}

/// Entry point used when Git launches this application as its askpass helper.
pub fn run_helper() -> Result<(), String> {
    let address = std::env::var("AUGUR_GIT_ASKPASS_ADDR")
        .map_err(|_| "Git askpass is not configured".to_string())?;
    let token = std::env::var("AUGUR_GIT_ASKPASS_TOKEN")
        .map_err(|_| "Git askpass is not configured".to_string())?;
    let operation_id = std::env::var("AUGUR_GIT_OP_ID").unwrap_or_default();
    let repository = std::env::var("AUGUR_GIT_REPO_PATH").unwrap_or_default();
    let prompt = prompt_from_arguments(std::env::args_os().skip(1).collect());
    let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    write_frame(&mut stream, token.as_bytes())?;
    write_frame(&mut stream, operation_id.as_bytes())?;
    write_frame(&mut stream, repository.as_bytes())?;
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
    use crate::credentials::SecretStores;

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

    #[test]
    fn an_askpass_launch_is_recognised_by_the_prompt_git_appends() {
        let arguments = vec![OsString::from("Username for 'https://example.com': ")];
        assert!(helper_invocation(
            Some("127.0.0.1:5000"),
            Some("token"),
            &arguments
        ));
    }

    #[test]
    fn a_wrapper_that_passes_the_flag_is_still_recognised() {
        let arguments = vec![
            OsString::from("--git-askpass"),
            OsString::from("Password for 'https://example.com': "),
        ];
        assert!(helper_invocation(
            Some("127.0.0.1:5000"),
            Some("token"),
            &arguments
        ));
    }

    #[test]
    fn a_normal_launch_is_not_mistaken_for_the_helper() {
        // No bridge variables: Finder, a shell launch, and the second instance
        // a user opens with a repository path all take the normal path.
        assert!(!helper_invocation(None, None, &[OsString::from("repo")]));
        // A flag or an extra argument is not what Git produces.
        assert!(!helper_invocation(
            Some("127.0.0.1:5000"),
            Some("token"),
            &[OsString::from("--logo"), OsString::from("x")]
        ));
        assert!(!helper_invocation(
            Some("127.0.0.1:5000"),
            Some("token"),
            &[]
        ));
        // A half-configured bridge cannot answer a prompt either.
        assert!(!helper_invocation(
            Some(""),
            Some("token"),
            &[OsString::from("Username")]
        ));
    }

    #[test]
    fn the_prompt_survives_both_argument_layouts() {
        assert_eq!(
            prompt_from_arguments(vec![OsString::from("Password for 'x': ")]),
            "Password for 'x': "
        );
        assert_eq!(
            prompt_from_arguments(vec![
                OsString::from("--git-askpass"),
                OsString::from("Password for 'x': "),
            ]),
            "Password for 'x': "
        );
    }

    #[test]
    fn an_unknown_request_id_is_reported_instead_of_panicking() {
        let vault = Arc::new(
            CredentialVault::load(
                Arc::new(
                    crate::store::JsonFileStore::open_in(
                        &std::env::temp_dir()
                            .join(format!("augur-askpass-{}", uuid::Uuid::new_v4().simple())),
                        "settings.json",
                    )
                    .unwrap(),
                ),
                SecretStores::memory_only(),
            )
            .0,
        );
        let result = respond_login(
            "missing",
            CredentialLoginAnswer {
                username: "dev".into(),
                password: "s3cret".into(),
                persist: Some(SecretStorage::Session),
                repository_only: false,
                cancel: false,
            },
            &vault,
        );
        assert!(!result.delivered);
        assert!(result.error.is_some());
    }
}
