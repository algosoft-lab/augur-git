//! Workspace-side lifecycle, scheduling, and event routing for extensions.

use std::time::{Duration, Instant};

use chrono::Local;
use gpui::*;

use crate::core::extension::{self, ExtensionSettings, SettingValue};
use crate::extension::{
    ExtensionEvent, ExtensionEventPayload, ExtensionRunRequest,
    ExtensionTrigger, HostEvent, discover_definitions,
};

use super::extensions::ExtensionsPanelEvent;
use super::{TabContent, Workspace};

pub(super) struct PendingEventBatch {
    pub(super) due_at: Instant,
    pub(super) events: Vec<ExtensionEventPayload>,
}

impl Workspace {
    pub(super) fn open_extensions(&mut self, cx: &mut Context<Self>) {
        self.show_settings = false;
        self.show_extensions = true;
        self.sync_extension_repositories(cx);
        self.extensions_panel.update(cx, |panel, cx| {
            panel.set_status("sync-open-tabs", "Ready", cx);
        });
        cx.notify();
    }

    pub(super) fn close_extensions(&mut self, cx: &mut Context<Self>) {
        self.show_extensions = false;
        cx.notify();
    }

    pub(super) fn sync_extension_repositories(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let snapshots = self
            .tabs
            .iter()
            .filter_map(|entry| match &entry.content {
                TabContent::Repo(tab) => {
                    Some(tab.read(cx).extension_snapshot())
                }
                TabContent::Welcome => None,
            })
            .collect::<Vec<_>>();
        let current = snapshots
            .iter()
            .cloned()
            .map(|snapshot| (snapshot.tab_id, snapshot))
            .collect::<std::collections::BTreeMap<_, _>>();
        let previous = std::mem::replace(
            &mut self.extension_observed_repositories,
            current.clone(),
        );
        self.extension_host.set_repositories(snapshots);
        self.extension_host
            .set_agent_settings(self.config.agent.clone());
        let now = Local::now();
        for (tab_id, snapshot) in &current {
            match previous.get(tab_id) {
                None => self.emit_repository_event(ExtensionEventPayload {
                    trigger_id: String::new(),
                    event_type: "workspace.repository_opened".into(),
                    occurred_at: now,
                    repository: Some(snapshot.clone()),
                    previous: None,
                    current: Some(snapshot.clone()),
                    origin_extension_id: None,
                    origin_run_id: None,
                }),
                Some(old) if repository_state_changed(old, snapshot) => {
                    let origin = self.extension_pending_origins.remove(tab_id);
                    let event_type = if old.branch != snapshot.branch {
                        "repository.branch_changed"
                    } else {
                        "repository.status_changed"
                    };
                    let origin_extension_id =
                        origin.as_ref().map(|value| value.0.clone());
                    let origin_run_id = origin.as_ref().map(|value| value.1);
                    self.emit_repository_event(ExtensionEventPayload {
                        trigger_id: String::new(),
                        event_type: event_type.into(),
                        occurred_at: now,
                        repository: Some(snapshot.clone()),
                        previous: Some(old.clone()),
                        current: Some(snapshot.clone()),
                        origin_extension_id: origin_extension_id.clone(),
                        origin_run_id,
                    });
                    if old.branch != snapshot.branch {
                        self.emit_repository_event(ExtensionEventPayload {
                            trigger_id: String::new(),
                            event_type: "repository.status_changed".into(),
                            occurred_at: now,
                            repository: Some(snapshot.clone()),
                            previous: Some(old.clone()),
                            current: Some(snapshot.clone()),
                            origin_extension_id: origin_extension_id.clone(),
                            origin_run_id,
                        });
                    }
                }
                _ => {}
            }
        }
        for (tab_id, snapshot) in previous {
            if !current.contains_key(&tab_id) {
                self.emit_repository_event(ExtensionEventPayload {
                    trigger_id: String::new(),
                    event_type: "workspace.repository_closed".into(),
                    occurred_at: now,
                    repository: Some(snapshot.clone()),
                    previous: Some(snapshot),
                    current: None,
                    origin_extension_id: None,
                    origin_run_id: None,
                });
            }
        }
    }

    pub(super) fn start_extension_polling(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let Some(workspace) = entity.upgrade() else {
                    break;
                };
                workspace
                    .update(cx, |workspace, cx| workspace.poll_extensions(cx));
            }
        })
        .detach();
    }

    fn poll_extensions(&mut self, cx: &mut Context<Self>) {
        while let Ok(event) = self.host_events.try_recv() {
            self.handle_host_event(event, cx);
        }
        self.sync_extension_repositories(cx);
        while let Ok(event) = self.extension_events.try_recv() {
            self.handle_extension_event(event, cx);
        }
        self.flush_pending_extension_events(cx);
        let now = Local::now();
        let previous = self.last_extension_tick;
        self.last_extension_tick = now;
        self.schedule_event_extensions(previous, now, cx);
    }

    fn emit_repository_event(&mut self, mut event: ExtensionEventPayload) {
        let now = Instant::now();
        for definition in self.extension_definitions.clone() {
            let extension_id = definition.package.manifest.id.clone();
            let settings = self
                .config
                .extensions
                .get(&extension_id)
                .cloned()
                .unwrap_or_else(|| {
                    ExtensionSettings::with_defaults(
                        &definition.package.manifest,
                    )
                })
                .normalized_for(&definition.package.manifest);
            if !settings.trusted
                || event.origin_extension_id.as_deref()
                    == Some(extension_id.as_str())
            {
                if event.origin_extension_id.as_deref()
                    == Some(extension_id.as_str())
                {
                    log::debug!(
                        "[extension_events] suppressed self-origin event: id={extension_id}, type={}",
                        event.event_type
                    );
                }
                continue;
            }
            for trigger in definition.package.manifest.event_triggers() {
                if trigger.event_type != event.event_type
                    || !settings.is_subscribed(&trigger.id)
                {
                    continue;
                }
                event.trigger_id = trigger.id.clone();
                let key = (extension_id.clone(), trigger.id.clone());
                let entry = self
                    .extension_pending_events
                    .entry(key)
                    .or_insert_with(|| PendingEventBatch {
                        due_at: now
                            + Duration::from_millis(
                                trigger.debounce_duration_ms(),
                            ),
                        events: Vec::new(),
                    });
                if let Some(tab_id) = event
                    .repository
                    .as_ref()
                    .map(|repository| repository.tab_id)
                {
                    entry.events.retain(|candidate| {
                        candidate
                            .repository
                            .as_ref()
                            .map(|repository| repository.tab_id)
                            != Some(tab_id)
                    });
                }
                entry.events.push(event.clone());
            }
        }
    }

    fn flush_pending_extension_events(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let due = self
            .extension_pending_events
            .iter()
            .filter(|(_, batch)| batch.due_at <= now)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for (extension_id, trigger_id) in due {
            let Some(batch) = self
                .extension_pending_events
                .remove(&(extension_id.clone(), trigger_id.clone()))
            else {
                continue;
            };
            let Some(definition) = self
                .extension_definitions
                .iter()
                .find(|definition| {
                    definition.package.manifest.id == extension_id
                })
                .cloned()
            else {
                continue;
            };
            let Some(trigger) = definition
                .package
                .manifest
                .event_triggers()
                .into_iter()
                .find(|trigger| trigger.id == trigger_id)
            else {
                continue;
            };
            let request = match self.extension_request(
                &extension_id,
                ExtensionTrigger::Repository {
                    trigger_id: trigger.id.clone(),
                    event_type: trigger.event_type.clone(),
                },
                None,
                batch.events,
                trigger.handler,
                cx,
            ) {
                Ok(request) => request,
                Err(error) => {
                    log::warn!(
                        "[extension_events] failed to build event request: id={extension_id}, error={error}"
                    );
                    continue;
                }
            };
            let Some(manager) = &self.extension_manager else {
                continue;
            };
            match manager.run(request) {
                Ok(Some(run_id)) => log::info!(
                    "[extension_events] queued repository event: id={extension_id}, trigger={trigger_id}, run_id={run_id}"
                ),
                Ok(None) => log::debug!(
                    "[extension_events] repository event coalesced: id={extension_id}, trigger={trigger_id}"
                ),
                Err(error) => log::warn!(
                    "[extension_events] repository event rejected: id={extension_id}, error={error}"
                ),
            }
        }
    }

    fn schedule_event_extensions(
        &mut self,
        previous: chrono::DateTime<Local>,
        now: chrono::DateTime<Local>,
        cx: &mut Context<Self>,
    ) {
        if self.extension_manager.is_none() {
            return;
        }
        let definitions = self.extension_definitions.clone();
        for definition in definitions {
            let id = definition.package.manifest.id.clone();
            let settings = self
                .config
                .extensions
                .get(&id)
                .cloned()
                .unwrap_or_else(|| {
                    ExtensionSettings::with_defaults(
                        &definition.package.manifest,
                    )
                })
                .normalized_for(&definition.package.manifest);
            if settings.subscribed_triggers.is_empty() || !settings.trusted {
                continue;
            }
            for trigger in definition.package.manifest.event_triggers() {
                if !settings.is_subscribed(&trigger.id) {
                    continue;
                }
                let (occurred_at, occurrence_key) =
                    match trigger.event_type.as_str() {
                        "schedule.daily" => {
                            let Some(SettingValue::Time(time)) = trigger
                                .time_setting
                                .as_deref()
                                .and_then(|key| settings.values.get(key))
                            else {
                                continue;
                            };
                            let Ok(time) = extension::parse_daily_time(time)
                            else {
                                continue;
                            };
                            let Some(occurrence) =
                                extension::daily_occurrence_between(
                                    previous, now, time,
                                )
                            else {
                                continue;
                            };
                            let occurrence_key =
                                extension::local_date_string(occurrence);
                            if settings
                                .last_event_occurrences
                                .get(&trigger.id)
                                .is_some_and(|value| value == &occurrence_key)
                            {
                                continue;
                            }
                            (occurrence, occurrence_key)
                        }
                        "schedule.interval" => {
                            let Some(SettingValue::Integer(minutes)) = trigger
                                .interval_setting
                                .as_deref()
                                .and_then(|key| settings.values.get(key))
                            else {
                                continue;
                            };
                            let key = (id.clone(), trigger.id.clone());
                            let last = self
                                .extension_interval_ticks
                                .entry(key)
                                .or_insert(previous);
                            if now.signed_duration_since(*last)
                                < chrono::Duration::minutes((*minutes).max(1))
                            {
                                continue;
                            }
                            *last = now;
                            (now, now.to_rfc3339())
                        }
                        _ => continue,
                    };
                let request = self.extension_request(
                    &id,
                    ExtensionTrigger::Schedule {
                        trigger_id: trigger.id.clone(),
                        event_type: trigger.event_type.clone(),
                    },
                    Some(occurred_at),
                    Vec::new(),
                    trigger.handler.clone(),
                    cx,
                );
                if let Ok(request) = request {
                    if let Some(manager) = &self.extension_manager {
                        match manager.run(request) {
                            Ok(result) => {
                                if let Some(entry) =
                                    self.config.extensions.get_mut(&id)
                                {
                                    entry.last_event_occurrences.insert(
                                        trigger.id.clone(),
                                        occurrence_key.clone(),
                                    );
                                    entry.last_scheduled_date =
                                        Some(occurrence_key.clone());
                                }
                                self.persist_config();
                                log::info!(
                                    "[extension_runtime] scheduled event queued: id={id}, trigger={}, result={result:?}",
                                    trigger.id
                                );
                            }
                            Err(error) => log::warn!(
                                "[extension_runtime] scheduled event rejected: id={id}, trigger={}, error={error}",
                                trigger.id
                            ),
                        }
                    }
                } else if let Err(error) = request {
                    log::warn!(
                        "[extension_runtime] failed to build scheduled event: id={id}, error={error}"
                    );
                }
            }
        }
    }

    fn extension_request(
        &self,
        extension_id: &str,
        trigger: ExtensionTrigger,
        scheduled_at: Option<chrono::DateTime<Local>>,
        events: Vec<ExtensionEventPayload>,
        handler: String,
        cx: &Context<Self>,
    ) -> Result<ExtensionRunRequest, String> {
        let definition = self
            .extension_definitions
            .iter()
            .find(|definition| definition.package.manifest.id == extension_id)
            .ok_or_else(|| "extension definition is unavailable".to_string())?;
        let settings = self
            .config
            .extensions
            .get(extension_id)
            .cloned()
            .unwrap_or_else(|| {
                ExtensionSettings::with_defaults(&definition.package.manifest)
            })
            .normalized_for(&definition.package.manifest);
        let repositories = self
            .tabs
            .iter()
            .filter_map(|entry| match &entry.content {
                TabContent::Repo(tab) => {
                    Some(tab.read(cx).extension_snapshot())
                }
                TabContent::Welcome => None,
            })
            .collect::<Vec<_>>();
        Ok(ExtensionRunRequest {
            extension_id: extension_id.to_string(),
            trigger,
            scheduled_at,
            settings: settings.values,
            repositories,
            events,
            handler,
        })
    }

    pub(super) fn handle_extensions_panel_event(
        &mut self,
        event: &ExtensionsPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExtensionsPanelEvent::Close => self.close_extensions(cx),
            ExtensionsPanelEvent::Reload => self.reload_extensions(window, cx),
            ExtensionsPanelEvent::InstallDirectory => {
                self.install_extension_directory(window, cx);
            }
            ExtensionsPanelEvent::Uninstall(extension_id) => {
                if self.extension_definitions.iter().any(|definition| {
                    definition.package.manifest.id == *extension_id
                        && definition.package.bundled
                }) {
                    return;
                }
                match extension::uninstall_local_package(extension_id) {
                    Ok(()) => {
                        self.reload_extensions(window, cx);
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(extension_id, "Uninstalled", cx);
                        });
                        self.config.extensions.remove(extension_id);
                        self.persist_config();
                    }
                    Err(error) => {
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(
                                extension_id,
                                error.to_string(),
                                cx,
                            );
                        })
                    }
                }
            }
            ExtensionsPanelEvent::EnabledChanged {
                extension_id,
                enabled,
            } => {
                let event_ids = self
                    .extension_definitions
                    .iter()
                    .find(|definition| {
                        definition.package.manifest.id == *extension_id
                    })
                    .map(|definition| {
                        definition
                            .package
                            .manifest
                            .event_triggers()
                            .into_iter()
                            .map(|trigger| trigger.id)
                            .collect::<std::collections::BTreeSet<_>>()
                    })
                    .unwrap_or_default();
                let settings = self
                    .config
                    .extensions
                    .entry(extension_id.clone())
                    .or_default();
                if *enabled && !settings.trusted {
                    self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(
                            extension_id,
                            "Trust this extension before enabling it",
                            cx,
                        );
                    });
                    return;
                }
                settings.subscribed_triggers = if *enabled {
                    event_ids
                } else {
                    Default::default()
                };
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.update_flags(
                        extension_id,
                        *enabled,
                        settings.trusted,
                        cx,
                    );
                });
                self.persist_config();
            }
            ExtensionsPanelEvent::TrustedChanged {
                extension_id,
                trusted,
            } => {
                let settings = self
                    .config
                    .extensions
                    .entry(extension_id.clone())
                    .or_default();
                settings.trusted = *trusted;
                if !*trusted {
                    settings.subscribed_triggers.clear();
                }
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.update_flags(
                        extension_id,
                        !settings.subscribed_triggers.is_empty(),
                        *trusted,
                        cx,
                    );
                });
                self.persist_config();
            }
            ExtensionsPanelEvent::SettingChanged {
                extension_id,
                key,
                value,
            } => {
                let Some(definition) =
                    self.extension_definitions.iter().find(|definition| {
                        definition.package.manifest.id == *extension_id
                    })
                else {
                    return;
                };
                let Some(setting) =
                    definition.package.manifest.settings.get(key)
                else {
                    return;
                };
                if let Err(error) = setting.validate_value(value) {
                    self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(extension_id, error, cx);
                    });
                    return;
                }
                let settings = self
                    .config
                    .extensions
                    .entry(extension_id.clone())
                    .or_default();
                settings.values.insert(key.clone(), value.clone());
                self.persist_config();
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.update_setting(extension_id, key, value.clone(), cx);
                    panel.set_status(extension_id, "Setting saved", cx);
                });
            }
            ExtensionsPanelEvent::RunNow(extension_id) => {
                let Some(manager) = &self.extension_manager else {
                    return;
                };
                let Some(definition) =
                    self.extension_definitions.iter().find(|definition| {
                        definition.package.manifest.id == *extension_id
                    })
                else {
                    return;
                };
                let settings = self
                    .config
                    .extensions
                    .entry(extension_id.clone())
                    .or_insert_with(|| {
                        ExtensionSettings::with_defaults(
                            &definition.package.manifest,
                        )
                    });
                if !settings.trusted {
                    self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(
                            extension_id,
                            "Enable and trust this extension before running it",
                            cx,
                        );
                    });
                    return;
                }
                let Some(handler) =
                    definition.package.manifest.manual_handler.clone()
                else {
                    self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(
                            extension_id,
                            "No manual handler declared",
                            cx,
                        )
                    });
                    return;
                };
                let request = match self.extension_request(
                    extension_id,
                    ExtensionTrigger::Manual,
                    None,
                    Vec::new(),
                    handler,
                    cx,
                ) {
                    Ok(request) => request,
                    Err(error) => {
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(extension_id, error, cx)
                        });
                        return;
                    }
                };
                match manager.run(request) {
                    Ok(Some(run_id)) => {
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(
                                extension_id,
                                format!("Queued run {run_id}"),
                                cx,
                            )
                        })
                    }
                    Ok(None) => {
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(
                                extension_id,
                                "Already queued or running",
                                cx,
                            )
                        })
                    }
                    Err(error) => {
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(extension_id, error, cx)
                        })
                    }
                }
            }
        }
    }

    fn install_extension_directory(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from("Select an extension directory")),
        });
        let entity = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.first().cloned(),
                _ => None,
            };
            let Some(path) = path else { return };
            let result = cx
                .background_executor()
                .spawn(async move { extension::install_local_package(&path) })
                .await;
            let _ = cx.update(|window, app| {
                entity.update(app, |workspace, cx| match result {
                    Ok(package) => {
                        let id = package.manifest.id.clone();
                        workspace.reload_extensions(window, cx);
                        workspace.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(&id, "Installed and reloaded", cx)
                        });
                    }
                    Err(error) => {
                        workspace.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(
                                "sync-open-tabs",
                                error.to_string(),
                                cx,
                            )
                        });
                    }
                })
            });
        })
        .detach();
    }

    fn reload_extensions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let definitions = discover_definitions();
        let reload_result = self
            .extension_manager
            .as_ref()
            .ok_or_else(|| "extension runtime is unavailable".to_string())
            .and_then(|manager| manager.reload(definitions.clone()));
        if let Err(error) = reload_result {
            self.extensions_panel.update(cx, |panel, cx| {
                panel.set_status("sync-open-tabs", error, cx)
            });
            return;
        }
        for definition in &definitions {
            let id = definition.package.manifest.id.clone();
            let entry = self.config.extensions.entry(id).or_insert_with(|| {
                ExtensionSettings::with_defaults(&definition.package.manifest)
            });
            *entry = entry.normalized_for(&definition.package.manifest);
            entry.last_seen_fingerprint =
                Some(definition.package.fingerprint.clone());
        }
        self.extension_definitions = definitions.clone();
        self.extensions_panel.update(cx, |panel, cx| {
            panel.replace_definitions(definitions, &self.config, window, cx);
            panel.set_status("sync-open-tabs", "Extensions reloaded", cx);
        });
        self.sync_extension_repositories(cx);
        self.persist_config();
    }

    fn handle_extension_event(
        &mut self,
        event: ExtensionEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExtensionEvent::RunQueued {
                extension_id,
                run_id,
            } => self.extensions_panel.update(cx, |panel, cx| {
                panel.set_status(
                    &extension_id,
                    format!("Queued run {run_id}"),
                    cx,
                )
            }),
            ExtensionEvent::RunStarted {
                extension_id,
                run_id,
            } => self.extensions_panel.update(cx, |panel, cx| {
                panel.set_status(&extension_id, format!("Running {run_id}"), cx)
            }),
            ExtensionEvent::WorkerError {
                extension_id,
                summary,
            } => self.extensions_panel.update(cx, |panel, cx| {
                panel.set_status(&extension_id, summary, cx)
            }),
            ExtensionEvent::RunFinished {
                extension_id,
                run_id,
                record,
                error,
            } => {
                if let Err(write_error) =
                    crate::extension::append_run_history(&extension_id, &record)
                {
                    log::warn!(
                        "[extensions] failed to save run history: {write_error}"
                    );
                }
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.append_history(&extension_id, record, cx)
                });
                let status = error
                    .map(|error| format!("Run {run_id} failed: {error}"))
                    .unwrap_or_else(|| format!("Run {run_id} completed"));
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.set_status(&extension_id, status, cx)
                });
            }
        }
    }

    fn handle_host_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Log {
                extension_id,
                level,
                message,
                fields,
            } => {
                log::info!(
                    "[extensions] id={extension_id}, level={level}, message={message}, fields={fields}"
                );
            }
            HostEvent::Notify {
                extension_id,
                level,
                title,
                body,
            } => {
                log::info!(
                    "[extensions] notification id={extension_id}, level={level}, title={title}, body={body}"
                );
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.set_status(
                        &extension_id,
                        format!("{title}: {body}"),
                        cx,
                    )
                });
            }
            HostEvent::RepositoryChanged {
                tab_id,
                origin_extension_id,
                origin_run_id,
            } => {
                self.extension_pending_origins
                    .insert(tab_id, (origin_extension_id, origin_run_id));
                if let Some(entry) =
                    self.tabs.iter().find(|entry| entry.id == tab_id)
                {
                    if let TabContent::Repo(tab) = &entry.content {
                        tab.update(cx, |tab, cx| {
                            tab.refresh_after_extension(cx)
                        });
                    }
                }
            }
        }
    }
}

fn repository_state_changed(
    previous: &crate::extension::RepositorySnapshot,
    current: &crate::extension::RepositorySnapshot,
) -> bool {
    previous.branch != current.branch
        || previous.head != current.head
        || previous.upstream != current.upstream
        || previous.dirty != current.dirty
        || previous.conflicts != current.conflicts
        || previous.busy != current.busy
        || previous.ahead != current.ahead
        || previous.behind != current.behind
        || previous.remotes != current.remotes
}
