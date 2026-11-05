//! End-to-end behaviour of the askpass bridge against the credential vault.
//!
//! The test plays Git's part: it speaks the same framed protocol the askpass
//! helper uses, so a wrong frame order or a prompt the bridge no longer answers
//! fails here rather than in a push.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use augur_service::askpass::{AskpassServer, CredentialLoginAnswer};
use augur_service::credentials::model::{CredentialInput, CredentialScope, SecretStorage};
use augur_service::credentials::secret::SecretStores;
use augur_service::credentials::vault::CredentialVault;
use augur_service::sink::{EventSink, EventTarget};
use augur_service::store::JsonFileStore;

struct CapturingSink(Mutex<Vec<serde_json::Value>>);

impl EventSink for CapturingSink {
    fn emit(&self, _target: EventTarget, _event: &str, payload: serde_json::Value) {
        self.0.lock().unwrap().push(payload);
    }
}

fn temp_vault() -> (Arc<CredentialVault>, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "augur-askpass-it-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let store = Arc::new(JsonFileStore::open_in(&directory, "settings.json").unwrap());
    let (vault, warnings) = CredentialVault::load(store, SecretStores::memory_only());
    assert!(warnings.is_empty());
    (Arc::new(vault), directory)
}

fn write_frame(stream: &mut TcpStream, bytes: &[u8]) {
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(bytes).unwrap();
}

fn read_frame(stream: &mut TcpStream) -> Vec<u8> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).unwrap();
    let length = u32::from_be_bytes(length) as usize;
    let mut frame = vec![0; length];
    stream.read_exact(&mut frame).unwrap();
    frame
}

fn prompt(address: &str, token: &str, operation: &str, repository: &str, text: &str) -> TcpStream {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame(&mut stream, token.as_bytes());
    write_frame(&mut stream, operation.as_bytes());
    write_frame(&mut stream, repository.as_bytes());
    write_frame(&mut stream, text.as_bytes());
    stream
}

#[test]
fn a_saved_password_answers_git_and_a_refusal_is_not_repeated() {
    let (vault, directory) = temp_vault();
    vault
        .save(
            CredentialInput {
                id: None,
                protocol: "https".into(),
                host: "github.com".into(),
                path_prefix: None,
                username: "dev".into(),
                password: Some("s3cret".into()),
                scope: CredentialScope::Global,
                storage: SecretStorage::Session,
            },
            1,
        )
        .unwrap();
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let server = AskpassServer::start(sink.clone(), vault.clone()).unwrap();
    let (address, token) = server.endpoint();
    let address = address.to_string();
    let token = token.to_string();

    // First password prompt: answered from storage without a dialog.
    let mut stream = prompt(
        &address,
        &token,
        "operation-1",
        "/home/dev/repo",
        "Password for 'https://dev@github.com': ",
    );
    assert_eq!(read_frame(&mut stream), b"s3cret");
    drop(stream);

    // The same operation asks again, which means the server refused it.
    let mut stream = prompt(
        &address,
        &token,
        "operation-1",
        "/home/dev/repo",
        "Password for 'https://dev@github.com': ",
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while sink.0.lock().unwrap().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let events = sink.0.lock().unwrap().clone();
    assert_eq!(events.len(), 1, "the refusal shows one form");
    assert_eq!(events[0]["kind"], "login");
    assert_eq!(events[0]["host"], "github.com");
    assert_eq!(events[0]["stored"], true);
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let mut buffer = [0u8; 4];
    assert!(
        stream.read_exact(&mut buffer).is_err(),
        "a refused password must not be answered again"
    );

    // The entry is now marked refused, so the next operation does not reuse it
    // either.
    assert!(
        vault
            .snapshot()
            .entries
            .first()
            .is_some_and(|entry| entry.failing)
    );

    // Answering the form delivers the typed password to the waiting stream.
    let request_id = events[0]["requestId"].as_str().unwrap().to_string();
    let result = augur_service::askpass::respond_login(
        &request_id,
        CredentialLoginAnswer {
            username: "dev".into(),
            password: "fresh".into(),
            persist: Some(SecretStorage::Session),
            repository_only: false,
            cancel: false,
        },
        &vault,
    );
    assert!(result.delivered);
    assert!(result.stored);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(read_frame(&mut stream), b"fresh");
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_fresh_host_shows_one_form_and_answers_both_prompts() {
    let (vault, directory) = temp_vault();
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let server = AskpassServer::start(sink.clone(), vault.clone()).unwrap();
    let (address, token) = server.endpoint();
    let address = address.to_string();
    let token = token.to_string();

    // Git asks for the username first and waits for the form.
    let mut username_stream = prompt(
        &address,
        &token,
        "operation-3",
        "/home/dev/repo",
        "Username for 'https://git.example.com': ",
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while sink.0.lock().unwrap().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let events = sink.0.lock().unwrap().clone();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "login");
    assert_eq!(events[0]["canPersist"], true);
    let request_id = events[0]["requestId"].as_str().unwrap().to_string();

    // Answering the form before the password prompt exists still delivers both.
    let result = augur_service::askpass::respond_login(
        &request_id,
        CredentialLoginAnswer {
            username: "dev".into(),
            password: "s3cret".into(),
            persist: None,
            repository_only: false,
            cancel: false,
        },
        &vault,
    );
    assert!(result.delivered);
    assert!(!result.stored);
    assert_eq!(read_frame(&mut username_stream), b"dev");

    // The password prompt belongs to the same form, not to a second one.
    let mut password_stream = prompt(
        &address,
        &token,
        "operation-3",
        "/home/dev/repo",
        "Password for 'https://dev@git.example.com': ",
    );
    assert_eq!(read_frame(&mut password_stream), b"s3cret");
    assert_eq!(sink.0.lock().unwrap().len(), 1, "one form, not two");
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn an_ssh_passphrase_prompt_is_not_stored_or_guessed() {
    let (vault, directory) = temp_vault();
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let server = AskpassServer::start(sink.clone(), vault).unwrap();
    let (address, token) = server.endpoint();
    let mut stream = prompt(
        address,
        token,
        "operation-2",
        "/home/dev/repo",
        "Enter passphrase for key '/home/dev/.ssh/id_ed25519': ",
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while sink.0.lock().unwrap().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let events = sink.0.lock().unwrap().clone();
    assert_eq!(events[0]["kind"], "prompt");
    assert_eq!(events[0]["canPersist"], false);
    let request_id = events[0]["requestId"].as_str().unwrap().to_string();
    assert!(augur_service::askpass::respond(
        &request_id,
        Some("phrase".into())
    ));
    assert_eq!(read_frame(&mut stream), b"phrase");
    let _ = std::fs::remove_dir_all(directory);
}

/// Git executes `GIT_ASKPASS` itself and appends the prompt as its only
/// argument: the value cannot carry a flag of its own. The helper recognises
/// that shape, so the test pins the shape rather than assuming it.
#[test]
fn git_runs_the_askpass_program_with_the_prompt_as_its_only_argument() {
    let directory = std::env::temp_dir().join(format!(
        "augur-askpass-shape-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let script = directory.join("askpass");
    let record = directory.join("arguments.txt");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$#\" \"$@\" > '{}'\nprintf 'answered\\n'\n",
            record.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = std::process::Command::new("git")
        .env("GIT_ASKPASS", &script)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .args(["credential", "fill"])
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"url=https://example.invalid/repo.git\n")?;
            child.wait_with_output()
        })
        .expect("git runs");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recorded = std::fs::read_to_string(&record).unwrap_or_default();
    let mut lines = recorded.lines();
    let count = lines.next().unwrap_or_default();
    let argument = lines.next().unwrap_or_default();
    assert_eq!(count, "1", "git passed {recorded:?}");
    assert!(
        argument.contains("example.invalid"),
        "the prompt is the only argument: {argument:?}"
    );
}
