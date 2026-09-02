//! Background Agent process controller used by extension host calls.

use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use crate::agent::AgentLaunchSpec;

use super::api::{HostResponse, MAX_AGENT_TRANSCRIPT_BYTES};
use super::host::HostEvent;

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const AGENT_ATTENTION_IDLE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct AgentResult {
    pub(super) ok: bool,
    pub(super) completed: bool,
    pub(super) exit_code: Option<i32>,
    pub(super) cancelled: bool,
    pub(super) timed_out: bool,
    pub(super) summary: String,
    pub(super) transcript: String,
}

pub(super) fn agent_response(
    result: AgentResult,
    verified: bool,
    summary: &str,
) -> HostResponse {
    HostResponse::Json(serde_json::json!({
        "ok": verified,
        "verified": verified,
        "completed": result.completed,
        "exit_code": result.exit_code,
        "cancelled": result.cancelled,
        "timed_out": result.timed_out,
        "summary": if verified { summary } else { result.summary.as_str() },
        // Keep the transcript in the in-memory Lua result only. The manager
        // stores a bounded run summary and never serializes this field.
        "transcript": result.transcript,
    }))
}

pub(super) fn run_agent_process(
    extension_id: &str,
    spec: AgentLaunchSpec,
    cwd: Option<&Path>,
    timeout: Duration,
    cancelled: &AtomicBool,
    event_tx: &Sender<HostEvent>,
) -> Result<AgentResult, String> {
    let mut command = std::process::Command::new(&spec.executable);
    command
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start Agent: {error}"))?;
    let started = Instant::now();
    let last_activity = Arc::new(AtomicU64::new(0));
    let stdout_activity = last_activity.clone();
    let stderr_activity = last_activity.clone();
    let stdout = child.stdout.take().map(|stdout| {
        thread::spawn(move || {
            collect_agent_output(stdout, stdout_activity, started)
        })
    });
    let stderr = child.stderr.take().map(|stderr| {
        thread::spawn(move || {
            collect_agent_output(stderr, stderr_activity, started)
        })
    });
    let mut was_cancelled = false;
    let mut timed_out = false;
    let mut attention_sent = false;
    loop {
        if cancelled.load(Ordering::Acquire) {
            was_cancelled = true;
            let _ = child.kill();
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        let activity_at =
            Duration::from_millis(last_activity.load(Ordering::Acquire));
        if !attention_sent
            && started.elapsed().saturating_sub(activity_at)
                >= AGENT_ATTENTION_IDLE
        {
            attention_sent = true;
            let _ = event_tx.send(HostEvent::Notify {
                extension_id: extension_id.to_string(),
                level: "warning".into(),
                title: "Agent needs attention".into(),
                body: "An extension Agent has produced no output for 60 seconds; review the repository or cancel the run.".into(),
            });
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                return Err(error.to_string());
            }
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("failed to collect Agent output: {error}"))?;
    let stdout = stdout
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr = stderr
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let transcript = bounded_transcript(&stdout, &stderr);
    let completed = transcript
        .lines()
        .any(|line| line.trim().starts_with("AUGUR_GIT_DONE:"));
    let result = AgentResult {
        ok: status.success() && completed && !was_cancelled && !timed_out,
        completed,
        exit_code: status.code(),
        cancelled: was_cancelled,
        timed_out,
        summary: if status.success() {
            "Agent exited without a verified completion marker".into()
        } else {
            summarize_agent_output(&stderr, &stdout, status.code())
        },
        transcript,
    };
    log::info!(
        "[agent_operation] extension Agent finished: ok={}, completed={}, code={:?}, cancelled={}, timed_out={}",
        result.ok,
        result.completed,
        result.exit_code,
        result.cancelled,
        result.timed_out
    );
    Ok(result)
}

fn collect_agent_output<R: Read>(
    mut reader: R,
    activity: Arc<AtomicU64>,
    started: Instant,
) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                activity.store(
                    started.elapsed().as_millis() as u64,
                    Ordering::Release,
                );
                if output.len() < MAX_AGENT_TRANSCRIPT_BYTES {
                    let remaining = MAX_AGENT_TRANSCRIPT_BYTES - output.len();
                    output.extend_from_slice(&buffer[..read.min(remaining)]);
                }
            }
            Err(_) => break,
        }
    }
    output
}

fn bounded_transcript(stdout: &[u8], stderr: &[u8]) -> String {
    let mut bytes = Vec::with_capacity(
        (stdout.len() + stderr.len()).min(MAX_AGENT_TRANSCRIPT_BYTES),
    );
    bytes.extend_from_slice(stdout);
    if !stderr.is_empty() {
        bytes.extend_from_slice(b"\n[stderr]\n");
        bytes.extend_from_slice(stderr);
    }
    bytes.truncate(MAX_AGENT_TRANSCRIPT_BYTES);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn summarize_agent_output(
    stderr: &[u8],
    stdout: &[u8],
    code: Option<i32>,
) -> String {
    let bytes = if !stderr.is_empty() { stderr } else { stdout };
    let output = String::from_utf8_lossy(bytes).trim().to_string();
    if output.is_empty() {
        code.map(|code| format!("Agent exited with status {code}"))
            .unwrap_or_else(|| "Agent terminated unexpectedly".into())
    } else {
        output.chars().take(2000).collect()
    }
}
