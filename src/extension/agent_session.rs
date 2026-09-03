//! Session protocol between the extension host and the visible interactive
//! Agent window.
//!
//! Extension Agent operations no longer spawn a headless process. The host
//! thread sends an [`AgentSessionRequest`] to the UI layer, which opens the
//! same visible `TerminalBackend` session used by manual Agent operations.
//! The window streams the `AUGUR_GIT_DONE:` completion marker from the live
//! terminal grid, classifies the repository outcome with the standard probes,
//! and reports one [`AgentSessionOutcome`] back through the reply channel.
//! The user can watch and intervene in the terminal at any time, and the
//! window Stop button or an extension cancellation stops the session.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use std::sync::mpsc::{self, Receiver, Sender};

use crate::agent::AgentOperation;

use super::api::HostResponse;

/// How often the host re-checks cancellation and the reply channel while an
/// Agent session is running.
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The Agent work an extension asks the UI layer to run in a visible session.
#[derive(Clone, Debug)]
pub(crate) enum AgentSessionOperation {
    /// A repository operation. The UI layer builds the fixed operation prompt
    /// with a per-session challenge, opens the matching session window, and
    /// reports the probe-classified outcome.
    Repository {
        operation: AgentOperation,
        hint: Option<String>,
    },
    /// A free-form prompt. The UI layer appends the completion-marker
    /// instruction and reports marker-based completion.
    Prompt { prompt: String },
}

/// A request to open one visible interactive Agent session.
pub(crate) struct AgentSessionRequest {
    pub(crate) extension_id: String,
    pub(crate) operation: AgentSessionOperation,
    /// Repository working directory; `None` falls back to the home directory.
    pub(crate) repository_path: Option<PathBuf>,
    /// Receives exactly one result. An `Err` means the session could not be
    /// opened at all (invalid profile, busy repository, launch failure).
    pub(crate) reply: Sender<Result<AgentSessionOutcome, String>>,
    /// Set by the host on cancellation or timeout; the session window polls
    /// it and stops itself exactly like a user-initiated Stop.
    pub(crate) cancelled: Arc<AtomicBool>,
}

/// The definitive result of one interactive Agent session.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AgentSessionOutcome {
    /// The session completed its work: the per-session marker was observed
    /// and, for repository operations, the repository probe classified a
    /// definitive result.
    Confirmed {
        summary: String,
    },
    /// The Agent or its terminal ended without the completion marker.
    Unconfirmed {
        exit_code: Option<i32>,
        summary: String,
    },
    Cancelled,
    TimedOut,
}

/// Extension-facing summary of one finished Agent session.
pub(crate) struct AgentResult {
    pub(crate) completed: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) cancelled: bool,
    pub(crate) timed_out: bool,
    pub(crate) summary: String,
}

impl AgentResult {
    pub(crate) fn from_outcome(
        outcome: AgentSessionOutcome,
        elapsed: Duration,
        timeout: Duration,
    ) -> Self {
        match outcome {
            AgentSessionOutcome::Confirmed { summary } => Self {
                completed: true,
                exit_code: None,
                cancelled: false,
                timed_out: false,
                summary,
            },
            AgentSessionOutcome::Unconfirmed { exit_code, summary } => Self {
                completed: false,
                exit_code,
                cancelled: false,
                timed_out: false,
                summary,
            },
            AgentSessionOutcome::Cancelled => Self {
                completed: false,
                exit_code: None,
                cancelled: true,
                timed_out: false,
                summary: "the Agent session was cancelled".into(),
            },
            AgentSessionOutcome::TimedOut => Self {
                completed: false,
                exit_code: None,
                cancelled: false,
                timed_out: true,
                summary: format!(
                    "the Agent session timed out after {}s",
                    timeout.as_secs().max(elapsed.as_secs())
                ),
            },
        }
    }
}

/// Build the structured Lua result for one finished Agent session.
///
/// `verified` reflects host-side repository verification; `verified_summary`
/// replaces the session summary when verification succeeded.
pub(crate) fn agent_response(
    result: AgentResult,
    verified: bool,
    verified_summary: &str,
) -> HostResponse {
    HostResponse::Json(serde_json::json!({
        "ok": verified,
        "verified": verified,
        "completed": result.completed,
        "exit_code": result.exit_code,
        "cancelled": result.cancelled,
        "timed_out": result.timed_out,
        "summary": if verified {
            verified_summary.to_string()
        } else {
            result.summary
        },
        "side_effects_verified": verified,
    }))
}

/// Block the extension worker thread until the interactive session reports a
/// result, the run is cancelled, or the timeout elapses.
///
/// Cancellation and timeout set `session_cancelled` so the window stops
/// itself; the host does not wait for that shutdown to finish.
pub(crate) fn wait_for_agent_session(
    reply: Receiver<Result<AgentSessionOutcome, String>>,
    run_cancelled: &AtomicBool,
    session_cancelled: &Arc<AtomicBool>,
    timeout: Duration,
) -> Result<AgentSessionOutcome, String> {
    let stop_session = || session_cancelled.store(true, Ordering::Release);
    let started = Instant::now();
    loop {
        if run_cancelled.load(Ordering::Acquire) {
            stop_session();
            return Ok(AgentSessionOutcome::Cancelled);
        }
        let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
            stop_session();
            return Ok(AgentSessionOutcome::TimedOut);
        };
        match reply.recv_timeout(remaining.min(WAIT_POLL_INTERVAL)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(
                    "the Agent session window closed without reporting a result"
                        .into(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> (
        Sender<Result<AgentSessionOutcome, String>>,
        Receiver<Result<AgentSessionOutcome, String>>,
    ) {
        mpsc::channel()
    }

    #[test]
    fn wait_returns_the_reported_outcome() {
        let (tx, rx) = channel();
        let cancelled = AtomicBool::new(false);
        let session_cancelled = Arc::new(AtomicBool::new(false));
        tx.send(Ok(AgentSessionOutcome::Confirmed {
            summary: "committed".into(),
        }))
        .expect("send outcome");
        let outcome = wait_for_agent_session(
            rx,
            &cancelled,
            &session_cancelled,
            Duration::from_secs(5),
        )
        .expect("session result");
        assert_eq!(
            outcome,
            AgentSessionOutcome::Confirmed {
                summary: "committed".into()
            }
        );
        assert!(!session_cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn wait_reports_launch_failure_from_the_session() {
        let (tx, rx) = channel();
        let cancelled = AtomicBool::new(false);
        let session_cancelled = Arc::new(AtomicBool::new(false));
        tx.send(Err("an Agent session is already active".into()))
            .expect("send failure");
        let error = wait_for_agent_session(
            rx,
            &cancelled,
            &session_cancelled,
            Duration::from_secs(5),
        )
        .expect_err("session failure must surface");
        assert!(error.contains("already active"));
    }

    #[test]
    fn wait_honours_run_cancellation_and_stops_the_session() {
        let (_tx, rx) = channel();
        let cancelled = AtomicBool::new(true);
        let session_cancelled = Arc::new(AtomicBool::new(false));
        let outcome = wait_for_agent_session(
            rx,
            &cancelled,
            &session_cancelled,
            Duration::from_secs(30),
        )
        .expect("cancelled result");
        assert_eq!(outcome, AgentSessionOutcome::Cancelled);
        assert!(session_cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn wait_times_out_and_stops_the_session() {
        let (_tx, rx) = channel();
        let cancelled = AtomicBool::new(false);
        let session_cancelled = Arc::new(AtomicBool::new(false));
        let outcome = wait_for_agent_session(
            rx,
            &cancelled,
            &session_cancelled,
            Duration::from_millis(30),
        )
        .expect("timeout result");
        assert_eq!(outcome, AgentSessionOutcome::TimedOut);
        assert!(session_cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn wait_reports_a_dropped_reply_channel() {
        let (tx, rx) = channel();
        drop(tx);
        let cancelled = AtomicBool::new(false);
        let session_cancelled = Arc::new(AtomicBool::new(false));
        let error = wait_for_agent_session(
            rx,
            &cancelled,
            &session_cancelled,
            Duration::from_secs(5),
        )
        .expect_err("a dropped channel must fail");
        assert!(error.contains("without reporting a result"));
    }

    #[test]
    fn result_mapping_reflects_the_outcome() {
        let confirmed = AgentResult::from_outcome(
            AgentSessionOutcome::Confirmed {
                summary: "merged".into(),
            },
            Duration::from_secs(2),
            Duration::from_secs(60),
        );
        assert!(confirmed.completed);
        assert!(confirmed.summary.contains("merged"));

        let unconfirmed = AgentResult::from_outcome(
            AgentSessionOutcome::Unconfirmed {
                exit_code: Some(3),
                summary: "exited".into(),
            },
            Duration::from_secs(2),
            Duration::from_secs(60),
        );
        assert!(!unconfirmed.completed);
        assert_eq!(unconfirmed.exit_code, Some(3));

        let timed_out = AgentResult::from_outcome(
            AgentSessionOutcome::TimedOut,
            Duration::from_secs(30),
            Duration::from_secs(30),
        );
        assert!(timed_out.timed_out);
        assert!(timed_out.summary.contains("30s"));
    }

    #[test]
    fn response_payload_keeps_the_documented_shape() {
        let result = AgentResult::from_outcome(
            AgentSessionOutcome::Confirmed {
                summary: "committed abc".into(),
            },
            Duration::from_secs(1),
            Duration::from_secs(60),
        );
        let response = agent_response(result, true, "verified commit");
        let HostResponse::Json(value) = response else {
            panic!("agent responses must be JSON");
        };
        assert_eq!(value["ok"], serde_json::json!(true));
        assert_eq!(value["verified"], serde_json::json!(true));
        assert_eq!(value["completed"], serde_json::json!(true));
        assert_eq!(value["summary"], serde_json::json!("verified commit"));
        assert_eq!(value["side_effects_verified"], serde_json::json!(true));
        assert!(value.get("transcript").is_none());
    }
}
