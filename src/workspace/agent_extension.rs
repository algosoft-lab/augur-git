//! Extension-initiated interactive Agent sessions.
//!
//! Lua extensions run Agent operations through the same visible
//! `TerminalBackend` windows as manual operations: the extension worker sends
//! an [`AgentSessionRequest`](crate::extension::agent_session::AgentSessionRequest)
//! and blocks, this module opens the matching `AgentSessionWindow` on the UI
//! thread, and the window reports exactly one
//! [`AgentSessionOutcome`](crate::extension::agent_session::AgentSessionOutcome)
//! back through the reply channel. Extension sessions register in the normal
//! session registry, so app-close guards, the per-repository deduplication
//! key, and the Stop button all behave like manual sessions.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};

use gpui::{
    App, AppContext, Context, WeakEntity, Window, WindowBounds,
    WindowDecorations, WindowKind, WindowOptions, px, size,
};
use gpui_component::TitleBar;

use crate::agent::{
    AgentOperation, AgentOperationChallenge, AgentPromptChallenge,
    ResolvedAgentProfile,
};
use crate::core::git::agent_operation::{
    probe_agent_merge, probe_agent_rebase,
};
use crate::core::i18n::Locale;
use crate::extension::{
    AgentSessionOperation, AgentSessionOutcome, AgentSessionRequest,
};

use super::Workspace;
use super::agent_commit::AgentCommitOutcome;
use super::agent_connectivity::{AgentSessionWindow, commit_key};
use super::agent_connectivity::{launch_for_profile, next_session_id};
use super::agent_merge::{AgentMergeMode, AgentMergeOutcome};
use super::agent_rebase::{AgentRebaseMode, AgentRebaseOutcome};

/// Channel from an extension-started session window back to the blocked
/// extension worker thread.
pub(super) struct ExtensionChannel {
    workspace: WeakEntity<Workspace>,
    session_id: u64,
    reply: Sender<Result<AgentSessionOutcome, String>>,
    cancelled: Arc<AtomicBool>,
}

impl ExtensionChannel {
    fn new(
        workspace: WeakEntity<Workspace>,
        session_id: u64,
        reply: Sender<Result<AgentSessionOutcome, String>>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            workspace,
            session_id,
            reply,
            cancelled,
        }
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(super) fn report(
        &self,
        outcome: AgentSessionOutcome,
    ) -> Result<(), mpsc::SendError<Result<AgentSessionOutcome, String>>> {
        self.reply.send(Ok(outcome))
    }

    pub(super) fn report_error(&self, summary: String) {
        let _ = self.reply.send(Err(summary));
    }

    /// The workspace entity and session id used to close the window after a
    /// confirmed outcome.
    pub(super) fn close_target(&self) -> (WeakEntity<Workspace>, u64) {
        (self.workspace.clone(), self.session_id)
    }
}

/// Handle one extension Agent session request on the UI thread.
///
/// Configuration problems and busy repositories are reported through the
/// reply channel without opening a window; launch errors detected by
/// `launch_for_profile` behave the same way.
pub(super) fn open_extension_session(
    workspace: &mut Workspace,
    request: AgentSessionRequest,
    cx: &mut Context<Workspace>,
) {
    let AgentSessionRequest {
        extension_id,
        operation,
        repository_path,
        reply,
        cancelled,
    } = request;
    workspace
        .agent_sessions
        .retain(|(_, handle)| handle.update(cx, |_, _, _| ()).is_ok());
    let profile_id = workspace.config.agent.default_profile_id();
    let Some(profile) = workspace.config.agent.profile(&profile_id) else {
        let _ = reply.send(Err(format!(
            "configured Agent profile is unavailable: {profile_id}"
        )));
        return;
    };
    let locale = workspace.locale;
    log::info!(
        "[agent_operation] extension Agent session accepted: extension={extension_id}"
    );
    match operation {
        AgentSessionOperation::Repository { operation, hint } => {
            let Some(path) = repository_path else {
                let _ = reply.send(Err(
                    "a repository Agent operation requires a repository".into(),
                ));
                return;
            };
            let key = commit_key(&path.to_string_lossy());
            if running_session_exists(workspace, &key, cx) {
                let _ = reply.send(Err(
                    "an Agent session is already active for this repository"
                        .into(),
                ));
                return;
            }
            match operation {
                AgentOperation::Commit => start_commit_session(
                    workspace, key, locale, profile, path, hint, reply,
                    cancelled, cx,
                ),
                AgentOperation::Merge { target_oid, .. } => {
                    start_merge_session(
                        key,
                        locale,
                        profile,
                        path,
                        AgentMergeMode::Start {
                            target_oid: target_oid.clone(),
                        },
                        reply,
                        cancelled,
                        cx,
                    )
                }
                AgentOperation::ResolveMerge { merge_head_oid, .. } => {
                    start_merge_session(
                        key,
                        locale,
                        profile,
                        path,
                        AgentMergeMode::Resolve {
                            merge_head_oid: merge_head_oid.clone(),
                        },
                        reply,
                        cancelled,
                        cx,
                    )
                }
                AgentOperation::Rebase { upstream_oid, .. } => {
                    start_rebase_session(
                        key,
                        locale,
                        profile,
                        path,
                        AgentRebaseMode::Start {
                            upstream_oid: upstream_oid.clone(),
                        },
                        reply,
                        cancelled,
                        cx,
                    )
                }
                AgentOperation::ResolveRebase {
                    rebase_head_oid,
                    upstream_oid,
                    ..
                } => start_rebase_session(
                    key,
                    locale,
                    profile,
                    path,
                    AgentRebaseMode::Resolve {
                        upstream_oid: upstream_oid.clone(),
                        rebase_head_oid: rebase_head_oid.clone(),
                    },
                    reply,
                    cancelled,
                    cx,
                ),
            }
        }
        AgentSessionOperation::Prompt { prompt } => start_prompt_session(
            workspace,
            locale,
            profile,
            repository_path,
            prompt,
            reply,
            cancelled,
            cx,
        ),
    }
}

fn running_session_exists(workspace: &Workspace, key: &str, cx: &App) -> bool {
    workspace.agent_sessions.iter().any(|(entry, handle)| {
        entry == key
            && handle.read(cx).is_ok_and(|session| session.is_running())
    })
}

#[allow(clippy::too_many_arguments)]
fn start_commit_session(
    workspace: &mut Workspace,
    key: String,
    locale: Locale,
    profile: ResolvedAgentProfile,
    path: PathBuf,
    hint: Option<String>,
    reply: Sender<Result<AgentSessionOutcome, String>>,
    cancelled: Arc<AtomicBool>,
    cx: &mut Context<Workspace>,
) {
    let session_id = next_session_id();
    let challenge = AgentOperationChallenge::new();
    let prompt = match AgentOperation::Commit
        .prompt_with_challenge(hint.as_deref(), &challenge)
    {
        Ok(prompt) => prompt,
        Err(error) => {
            let _ = reply.send(Err(error.to_string()));
            return;
        }
    };
    let extension = ExtensionChannel::new(
        cx.entity().downgrade(),
        session_id,
        reply,
        cancelled,
    );
    let (spec, startup_error) =
        launch_for_profile(workspace, &profile, &prompt, cx);
    if let Some(error) = startup_error {
        extension.report_error(error);
        return;
    }
    open_extension_session_window(
        workspace,
        key,
        extension,
        session_id,
        move |window, cx| {
            AgentSessionWindow::new_commit(
                locale,
                profile,
                spec,
                prompt,
                path,
                None,
                challenge,
                None,
                window.window_handle().window_id().as_u64(),
                cx,
            )
        },
        cx,
    );
}

#[allow(clippy::too_many_arguments)]
fn start_merge_session(
    key: String,
    locale: Locale,
    profile: ResolvedAgentProfile,
    path: PathBuf,
    mode: AgentMergeMode,
    reply: Sender<Result<AgentSessionOutcome, String>>,
    cancelled: Arc<AtomicBool>,
    cx: &mut Context<Workspace>,
) {
    let session_id = next_session_id();
    let challenge = AgentOperationChallenge::new();
    let extension = ExtensionChannel::new(
        cx.entity().downgrade(),
        session_id,
        reply.clone(),
        cancelled,
    );
    let entity = cx.entity().downgrade();
    let probe_path = path.clone();
    let target = mode.target_oid().to_string();
    cx.spawn(async move |_, cx| {
        let baseline = cx
            .background_executor()
            .spawn(async move { probe_agent_merge(&probe_path, &target) })
            .await;
        let _ = entity.update(cx, |workspace, cx| {
            let baseline = match baseline {
                Ok(baseline) => baseline,
                Err(error) => {
                    extension.report_error(first_line(&error).to_string());
                    return;
                }
            };
            // Mirror `start_merge_session` in agent_connectivity: the prompt
            // embeds the probe baseline captured immediately before launch.
            let operation = match &mode {
                AgentMergeMode::Start { target_oid } => AgentOperation::Merge {
                    target_oid: target_oid.clone(),
                    baseline_head: baseline.head.clone(),
                },
                AgentMergeMode::Resolve { merge_head_oid } => {
                    AgentOperation::ResolveMerge {
                        merge_head_oid: merge_head_oid.clone(),
                        baseline_head: baseline.head.clone(),
                    }
                }
            };
            let prompt = match operation.prompt_with_challenge(None, &challenge)
            {
                Ok(prompt) => prompt,
                Err(error) => {
                    extension.report_error(error.to_string());
                    return;
                }
            };
            let (spec, startup_error) =
                launch_for_profile(workspace, &profile, &prompt, cx);
            if let Some(error) = startup_error {
                extension.report_error(error);
                return;
            }
            open_extension_session_window(
                workspace,
                key,
                extension,
                session_id,
                move |window, cx| {
                    AgentSessionWindow::new_merge(
                        locale,
                        profile,
                        spec,
                        prompt,
                        path,
                        None,
                        challenge,
                        mode,
                        baseline,
                        None,
                        window.window_handle().window_id().as_u64(),
                        cx,
                    )
                },
                cx,
            );
        });
    })
    .detach();
}

#[allow(clippy::too_many_arguments)]
fn start_rebase_session(
    key: String,
    locale: Locale,
    profile: ResolvedAgentProfile,
    path: PathBuf,
    mode: AgentRebaseMode,
    reply: Sender<Result<AgentSessionOutcome, String>>,
    cancelled: Arc<AtomicBool>,
    cx: &mut Context<Workspace>,
) {
    let session_id = next_session_id();
    let challenge = AgentOperationChallenge::new();
    let extension = ExtensionChannel::new(
        cx.entity().downgrade(),
        session_id,
        reply.clone(),
        cancelled,
    );
    let entity = cx.entity().downgrade();
    let probe_path = path.clone();
    let upstream = mode.upstream_oid().map(ToOwned::to_owned);
    cx.spawn(async move |_, cx| {
        let baseline = cx
            .background_executor()
            .spawn(async move {
                probe_agent_rebase(&probe_path, upstream.as_deref())
            })
            .await;
        let _ = entity.update(cx, |workspace, cx| {
            let baseline = match baseline {
                Ok(baseline) => baseline,
                Err(error) => {
                    extension.report_error(first_line(&error).to_string());
                    return;
                }
            };
            // Mirror the rebase prompt construction in agent_connectivity.
            let operation = match &mode {
                AgentRebaseMode::Start { upstream_oid } => {
                    AgentOperation::Rebase {
                        upstream_oid: upstream_oid.clone(),
                        baseline_head: baseline.head.clone(),
                    }
                }
                AgentRebaseMode::Resolve {
                    upstream_oid,
                    rebase_head_oid,
                } => AgentOperation::ResolveRebase {
                    rebase_head_oid: rebase_head_oid.clone(),
                    upstream_oid: upstream_oid.clone(),
                    baseline_head: baseline.head.clone(),
                },
            };
            let prompt = match operation.prompt_with_challenge(None, &challenge)
            {
                Ok(prompt) => prompt,
                Err(error) => {
                    extension.report_error(error.to_string());
                    return;
                }
            };
            let (spec, startup_error) =
                launch_for_profile(workspace, &profile, &prompt, cx);
            if let Some(error) = startup_error {
                extension.report_error(error);
                return;
            }
            open_extension_session_window(
                workspace,
                key,
                extension,
                session_id,
                move |window, cx| {
                    AgentSessionWindow::new_rebase(
                        locale,
                        profile,
                        spec,
                        prompt,
                        path,
                        None,
                        challenge,
                        mode,
                        baseline,
                        None,
                        window.window_handle().window_id().as_u64(),
                        cx,
                    )
                },
                cx,
            );
        });
    })
    .detach();
}

#[allow(clippy::too_many_arguments)]
fn start_prompt_session(
    workspace: &mut Workspace,
    locale: Locale,
    profile: ResolvedAgentProfile,
    repository_path: Option<PathBuf>,
    prompt: String,
    reply: Sender<Result<AgentSessionOutcome, String>>,
    cancelled: Arc<AtomicBool>,
    cx: &mut Context<Workspace>,
) {
    let session_id = next_session_id();
    let challenge = AgentPromptChallenge::new();
    let full_prompt = format!("{prompt}\n\n{}", challenge.prompt);
    let extension = ExtensionChannel::new(
        cx.entity().downgrade(),
        session_id,
        reply,
        cancelled,
    );
    let working_directory = repository_path
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let (spec, startup_error) =
        launch_for_profile(workspace, &profile, &full_prompt, cx);
    if let Some(error) = startup_error {
        extension.report_error(error);
        return;
    }
    let key = format!("extension-prompt:{session_id}");
    open_extension_session_window(
        workspace,
        key,
        extension,
        session_id,
        move |window, cx| {
            AgentSessionWindow::new_prompt(
                locale,
                profile,
                spec,
                full_prompt,
                working_directory,
                challenge.expected_marker,
                None,
                window.window_handle().window_id().as_u64(),
                cx,
            )
        },
        cx,
    );
}

/// Open one extension session window and register it in the workspace.
fn open_extension_session_window(
    workspace: &mut Workspace,
    key: String,
    extension: ExtensionChannel,
    session_id: u64,
    build: impl FnOnce(
        &mut Window,
        &mut Context<AgentSessionWindow>,
    ) -> AgentSessionWindow,
    cx: &mut Context<Workspace>,
) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            size(px(1120.), px(760.)),
            cx,
        )),
        is_resizable: true,
        kind: WindowKind::Normal,
        window_decorations: Some(WindowDecorations::Client),
        window_min_size: Some(size(px(760.), px(520.))),
        ..TitleBar::window_options()
    };
    match cx.open_window(options, move |window, cx| {
        let session = cx.new(|cx| {
            let mut session = build(window, cx);
            session.set_extension_channel(session_id, extension);
            session
        });
        let weak_session = session.downgrade();
        window.on_window_should_close(cx, move |_window, app| {
            let _ = weak_session.update(app, |session, cx| session.stop(cx));
            true
        });
        window.activate_window();
        session
    }) {
        Ok(handle) => workspace.agent_sessions.push((key, handle)),
        Err(_error) => {
            log::error!(
                "[agent_terminal] failed to open extension agent session window"
            );
        }
    }
}

/// Map a window commit outcome onto the extension session protocol.
pub(super) fn commit_outcome(
    outcome: &AgentCommitOutcome,
) -> AgentSessionOutcome {
    match outcome {
        AgentCommitOutcome::Committed { .. } => {
            AgentSessionOutcome::Confirmed {
                summary: "the Agent reported the commit complete".into(),
            }
        }
        AgentCommitOutcome::NoChanges => AgentSessionOutcome::Confirmed {
            summary: "the working tree had no changes to commit".into(),
        },
        AgentCommitOutcome::Conflict => AgentSessionOutcome::Confirmed {
            summary: "the commit ended with unresolved conflicts".into(),
        },
        AgentCommitOutcome::Failed => AgentSessionOutcome::Unconfirmed {
            exit_code: None,
            summary: "the Agent did not complete the commit".into(),
        },
        AgentCommitOutcome::Cancelled => AgentSessionOutcome::Cancelled,
        AgentCommitOutcome::ExitedUnverified { code } => {
            AgentSessionOutcome::Unconfirmed {
                exit_code: *code,
                summary: "the Agent exited without the completion marker"
                    .into(),
            }
        }
    }
}

/// Map a window merge outcome onto the extension session protocol.
pub(super) fn merge_outcome(
    outcome: &AgentMergeOutcome,
) -> AgentSessionOutcome {
    match outcome {
        AgentMergeOutcome::Merged { .. }
        | AgentMergeOutcome::AlreadyUpToDate => {
            AgentSessionOutcome::Confirmed {
                summary: "the Agent reported the merge complete".into(),
            }
        }
        AgentMergeOutcome::Conflict => AgentSessionOutcome::Confirmed {
            summary: "the merge ended with unresolved conflicts".into(),
        },
        AgentMergeOutcome::Failed => AgentSessionOutcome::Unconfirmed {
            exit_code: None,
            summary: "the Agent did not complete the merge".into(),
        },
        AgentMergeOutcome::Cancelled => AgentSessionOutcome::Cancelled,
        AgentMergeOutcome::ExitedUnverified { code } => {
            AgentSessionOutcome::Unconfirmed {
                exit_code: *code,
                summary: "the Agent exited without the completion marker"
                    .into(),
            }
        }
    }
}

/// Map a window rebase outcome onto the extension session protocol.
pub(super) fn rebase_outcome(
    outcome: &AgentRebaseOutcome,
) -> AgentSessionOutcome {
    match outcome {
        AgentRebaseOutcome::Rebased { .. }
        | AgentRebaseOutcome::AlreadyUpToDate => {
            AgentSessionOutcome::Confirmed {
                summary: "the Agent reported the rebase complete".into(),
            }
        }
        AgentRebaseOutcome::Conflict => AgentSessionOutcome::Confirmed {
            summary: "the rebase ended with unresolved conflicts".into(),
        },
        AgentRebaseOutcome::Failed => AgentSessionOutcome::Unconfirmed {
            exit_code: None,
            summary: "the Agent did not complete the rebase".into(),
        },
        AgentRebaseOutcome::Cancelled => AgentSessionOutcome::Cancelled,
        AgentRebaseOutcome::ExitedUnverified { code } => {
            AgentSessionOutcome::Unconfirmed {
                exit_code: *code,
                summary: "the Agent exited without the completion marker"
                    .into(),
            }
        }
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}
