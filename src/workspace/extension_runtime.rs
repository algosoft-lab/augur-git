//! Workspace-side lifecycle, scheduling, and event routing for extensions.

use std::time::{Duration, Instant};

use chrono::Local;
use gpui::*;

use crate::core::extension::{self, ExtensionSettings, SettingValue};
use crate::core::i18n;
use crate::extension::{
    ExtensionEvent, ExtensionEventPayload, ExtensionRunRequest,
    ExtensionTrigger, HostEvent, discover_definitions,
};

use super::extensions::ExtensionsPanelEvent;
use super::extensions_window::ExtensionsWindow;
use super::{TabContent, Workspace};

const EXTENSION_ORIGIN_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) struct PendingEventBatch {
    pub(super) due_at: Instant,
    pub(super) events: Vec<ExtensionEventPayload>,
}

impl Workspace {
    pub(super) fn open_extensions(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = self.extensions_window {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.extensions_window = None;
        }
        self.show_settings = false;
        self.sync_extension_repositories(cx);
        let definitions = self.extension_definitions.clone();
        let config = self.config.clone();
        let locale = self.locale;
        let workspace = cx.entity().downgrade();
        let extension_window_state = self.ui_state.extensions_window.clone();
        let options = super::extensions_window::window_options(
            cx,
            &extension_window_state,
        );
        match cx.open_window(options, move |window, cx| {
            let panel = cx.new(|cx| {
                super::extensions::ExtensionsPanel::new(
                    definitions,
                    &config,
                    locale,
                    window,
                    cx,
                )
            });
            let workspace_for_close = workspace.clone();
            let extension_window = cx.new(|cx| {
                ExtensionsWindow::new(
                    panel,
                    locale,
                    workspace.clone(),
                    window,
                    cx,
                )
            });
            window.on_window_should_close(cx, move |window, app| {
                let _ = workspace_for_close.update(app, |workspace, cx| {
                    super::window_state::update_ui_state_extensions_window(
                        &mut workspace.ui_state,
                        window,
                    );
                    // Settings edits are window-local drafts. Closing the
                    // management window without saving must not leave a
                    // hidden draft that a later Run Once could commit.
                    workspace.extension_drafts.clear();
                    workspace.extensions_window = None;
                    workspace.persist_ui_state(cx);
                });
                true
            });
            window.activate_window();
            extension_window
        }) {
            Ok(handle) => {
                if let Ok(panel) =
                    handle.update(cx, |window, _, _| window.panel.clone())
                {
                    self.extensions_panel = panel;
                }
                self.extensions_window = Some(handle);
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.set_status(
                        "sync-open-tabs",
                        i18n::text(self.locale, "extensions-status-ready"),
                        cx,
                    );
                    if let Some(manager) = &self.extension_manager {
                        for (extension_id, run_id) in manager.active_runs() {
                            panel.set_status(
                                &extension_id,
                                i18n::text_args(
                                    self.locale,
                                    "extensions-status-active-run",
                                    &[("run_id", &run_id.to_string())],
                                ),
                                cx,
                            );
                        }
                    }
                    if let Some((extension_id, _)) =
                        &self.pending_extension_install
                    {
                        panel.set_pending_install(extension_id.clone(), cx);
                    }
                });
            }
            Err(error) => log::error!(
                "[extension_runtime] failed to open Extensions window: {error}"
            ),
        }
    }

    pub(super) fn close_extensions(&mut self, cx: &mut Context<Self>) {
        self.extension_drafts.clear();
        if let Some(handle) = self.extensions_window.take() {
            let _ = handle.update(cx, |_extensions, window, _| {
                window.remove_window();
            });
        }
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
                    let origin =
                        self.extension_pending_origins.remove(tab_id).map(
                            |(extension_id, run_id, _)| (extension_id, run_id),
                        );
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
                Some(_) => {
                    // A mutating host call can finish without changing the
                    // semantic snapshot (for example an already-up-to-date
                    // fetch). Keep the origin briefly while the asynchronous
                    // Git refresh catches up, then expire it so an unrelated
                    // later event is not suppressed.
                    if self.extension_pending_origins.get(tab_id).is_some_and(
                        |(_, _, observed_at)| {
                            observed_at.elapsed() >= EXTENSION_ORIGIN_TIMEOUT
                        },
                    ) {
                        self.extension_pending_origins.remove(tab_id);
                    }
                }
            }
        }
        for (tab_id, snapshot) in previous {
            if !current.contains_key(&tab_id) {
                self.extension_pending_origins.remove(&tab_id);
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
                        "[extension_events] suppressed self-origin event: id={extension_id}, run={:?}, type={}",
                        event.origin_run_id,
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
                entry.due_at =
                    now + Duration::from_millis(trigger.debounce_duration_ms());
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
                if !trigger.is_schedule() {
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

    fn commit_extension_draft(
        &mut self,
        extension_id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(draft) = self.extension_drafts.get(extension_id).cloned()
        else {
            return Ok(());
        };
        let Some(definition) = self
            .extension_definitions
            .iter()
            .find(|definition| definition.package.manifest.id == extension_id)
            .cloned()
        else {
            return Err("extension definition is unavailable".to_string());
        };
        for (key, value) in &draft {
            let Some(setting) = definition.package.manifest.settings.get(key)
            else {
                return Err(format!("unknown extension setting: {key}"));
            };
            setting
                .validate_value(value)
                .map_err(|error| format!("invalid setting {key}: {error}"))?;
        }
        let mut settings = self
            .config
            .extensions
            .get(extension_id)
            .cloned()
            .unwrap_or_else(|| {
                ExtensionSettings::with_defaults(&definition.package.manifest)
            })
            .normalized_for(&definition.package.manifest);
        for (key, value) in &draft {
            settings.values.insert(key.clone(), value.clone());
        }
        self.config
            .extensions
            .insert(extension_id.to_string(), settings);
        self.extension_drafts.remove(extension_id);
        self.persist_config();
        self.extensions_panel.update(cx, |panel, cx| {
            for (key, value) in draft {
                panel.update_setting(extension_id, &key, value, cx);
            }
        });
        Ok(())
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
            ExtensionsPanelEvent::ConfirmInstall(extension_id) => {
                self.confirm_extension_install(extension_id, window, cx);
            }
            ExtensionsPanelEvent::CancelInstall => {
                self.pending_extension_install = None;
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.clear_pending_install(cx);
                });
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
                            panel.set_status(
                                extension_id,
                                i18n::text(
                                    self.locale,
                                    "extensions-status-uninstalled",
                                ),
                                cx,
                            );
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
            ExtensionsPanelEvent::SubscriptionChanged {
                extension_id,
                trigger_id,
                subscribed,
            } => {
                let Some(definition) = self
                    .extension_definitions
                    .iter()
                    .find(|definition| {
                        definition.package.manifest.id == *extension_id
                    })
                    .cloned()
                else {
                    return;
                };
                let Some(trigger) = definition
                    .package
                    .manifest
                    .event_triggers()
                    .into_iter()
                    .find(|trigger| trigger.id == *trigger_id)
                else {
                    return;
                };
                if let Err(error) =
                    self.commit_extension_draft(extension_id, cx)
                {
                    self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(extension_id, error, cx)
                    });
                    return;
                }
                let mut settings = self
                    .config
                    .extensions
                    .get(extension_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        ExtensionSettings::with_defaults(
                            &definition.package.manifest,
                        )
                    })
                    .normalized_for(&definition.package.manifest);
                if *subscribed && !settings.trusted {
                    self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(
                            extension_id,
                            i18n::text(
                                self.locale,
                                "extensions-status-trust-subscribe",
                            ),
                            cx,
                        );
                    });
                    return;
                }
                if *subscribed {
                    settings.subscribed_triggers.insert(trigger.id.clone());
                } else {
                    settings.subscribed_triggers.remove(trigger.id.as_str());
                }
                self.config
                    .extensions
                    .insert(extension_id.clone(), settings);
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.update_subscription(
                        extension_id,
                        trigger_id,
                        *subscribed,
                        cx,
                    );
                    panel.set_status(
                        extension_id,
                        if *subscribed {
                            i18n::text_args(
                                self.locale,
                                "extensions-status-subscribed",
                                &[("label", trigger.label())],
                            )
                        } else {
                            i18n::text_args(
                                self.locale,
                                "extensions-status-unsubscribed",
                                &[("label", trigger.label())],
                            )
                        },
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
                    if let Some(manager) = &self.extension_manager {
                        let cancelled = manager.cancel_extension(extension_id);
                        if cancelled > 0 {
                            log::info!(
                                "[extension_runtime] trust revoked; cancellation requested for id={extension_id}, runs={cancelled}"
                            );
                        }
                    }
                }
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.update_trust(extension_id, *trusted, cx);
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
                let validation_error = setting.validate_value(value).err();
                self.extension_drafts
                    .entry(extension_id.clone())
                    .or_default()
                    .insert(key.clone(), value.clone());
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.update_setting(extension_id, key, value.clone(), cx);
                    if let Some(error) = validation_error {
                        panel.set_setting_error(extension_id, key, error, cx);
                        panel.set_status(
                            extension_id,
                            i18n::text(
                                self.locale,
                                "extensions-status-invalid-setting",
                            ),
                            cx,
                        );
                    } else {
                        panel.clear_setting_error(extension_id, key, cx);
                        panel.set_status(
                            extension_id,
                            i18n::text(
                                self.locale,
                                "extensions-status-unsaved-settings",
                            ),
                            cx,
                        );
                    }
                });
            }
            ExtensionsPanelEvent::SaveSettings(extension_id) => {
                match self.commit_extension_draft(extension_id, cx) {
                    Ok(()) => self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(
                            extension_id,
                            i18n::text(
                                self.locale,
                                "extensions-status-settings-saved",
                            ),
                            cx,
                        )
                    }),
                    Err(error) => {
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(extension_id, error, cx)
                        })
                    }
                }
            }
            ExtensionsPanelEvent::Cancel(extension_id) => {
                let cancelled = self
                    .extension_manager
                    .as_ref()
                    .map(|manager| manager.cancel_extension(extension_id))
                    .unwrap_or(0);
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.set_status(
                        extension_id,
                        if cancelled == 0 {
                            i18n::text(
                                self.locale,
                                "extensions-status-no-active-run",
                            )
                        } else {
                            i18n::text_args(
                                self.locale,
                                "extensions-status-cancel-requested",
                                &[("count", &cancelled.to_string())],
                            )
                        },
                        cx,
                    );
                });
            }
            ExtensionsPanelEvent::RunNow(extension_id) => {
                if let Err(error) =
                    self.commit_extension_draft(extension_id, cx)
                {
                    self.extensions_panel.update(cx, |panel, cx| {
                        panel.set_status(extension_id, error, cx)
                    });
                    return;
                }
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
                            i18n::text(
                                self.locale,
                                "extensions-status-trust-run",
                            ),
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
                            i18n::text(
                                self.locale,
                                "extensions-status-no-manual-handler",
                            ),
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
                                i18n::text_args(
                                    self.locale,
                                    "extensions-status-queued-run",
                                    &[("run_id", &run_id.to_string())],
                                ),
                                cx,
                            )
                        })
                    }
                    Ok(None) => {
                        self.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(
                                extension_id,
                                i18n::text(
                                    self.locale,
                                    "extensions-status-already-running",
                                ),
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
            prompt: Some(SharedString::from(i18n::text(
                self.locale,
                "extensions-install-prompt",
            ))),
        });
        let locale = self.locale;
        let entity = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.first().cloned(),
                _ => None,
            };
            let Some(path) = path else { return };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let package = extension::load_local_package(&path)?;
                    let exists =
                        extension::local_package_exists(&package.manifest.id)?;
                    Ok::<_, extension::ExtensionError>((
                        path,
                        package.manifest.id,
                        exists,
                    ))
                })
                .await;
            let _ = cx.update(|window, app| {
                entity.update(app, |workspace, cx| match result {
                    Ok((path, id, exists)) => {
                        if workspace.extension_definitions.iter().any(
                            |definition| {
                                definition.package.bundled
                                    && definition.package.manifest.id == id
                            },
                        ) {
                            workspace.extensions_panel.update(
                                cx,
                                |panel, cx| {
                                    panel.set_status(
                                        "sync-open-tabs",
                                        i18n::text(
                                            locale,
                                            "extensions-status-bundled-replace",
                                        ),
                                        cx,
                                    )
                                },
                            );
                            return;
                        }
                        if exists {
                            workspace.pending_extension_install =
                                Some((id.clone(), path));
                            workspace.extensions_panel.update(
                                cx,
                                |panel, cx| {
                                    panel.set_pending_install(id, cx);
                                    panel.set_status(
                                    "sync-open-tabs",
                                    i18n::text(
                                        locale,
                                        "extensions-status-confirm-replacement",
                                    ),
                                    cx,
                                );
                                },
                            );
                        } else {
                            workspace.install_extension_package(
                                path, id, window, cx,
                            );
                        }
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

    fn confirm_extension_install(
        &mut self,
        extension_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pending_id, path)) = self.pending_extension_install.take()
        else {
            return;
        };
        if pending_id != extension_id {
            self.pending_extension_install = Some((pending_id, path));
            return;
        }
        self.extensions_panel.update(cx, |panel, cx| {
            panel.clear_pending_install(cx);
            panel.set_status(
                extension_id,
                i18n::text(self.locale, "extensions-status-replacing"),
                cx,
            );
        });
        self.install_extension_package(
            path,
            extension_id.to_string(),
            window,
            cx,
        );
    }

    fn install_extension_package(
        &mut self,
        path: std::path::PathBuf,
        extension_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let locale = self.locale;
        let entity = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
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
                            panel.set_status(
                                &id,
                                if id == extension_id {
                                    i18n::text(
                                        locale,
                                        "extensions-status-installed",
                                    )
                                } else {
                                    i18n::text(
                                        locale,
                                        "extensions-status-installed-different-id",
                                    )
                                },
                                cx,
                            )
                        });
                    }
                    Err(error) => {
                        workspace.extensions_panel.update(cx, |panel, cx| {
                            panel.set_status(
                                &extension_id,
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
        // Package code and trigger declarations may have changed. Pending
        // payloads and interval anchors belong to the old definitions and
        // must not be replayed against newly loaded Lua VMs.
        self.extension_pending_events.clear();
        self.extension_interval_ticks.clear();
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
            panel.set_status(
                "sync-open-tabs",
                i18n::text(self.locale, "extensions-status-reloaded"),
                cx,
            );
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
                    i18n::text_args(
                        self.locale,
                        "extensions-status-queued-run",
                        &[("run_id", &run_id.to_string())],
                    ),
                    cx,
                )
            }),
            ExtensionEvent::RunStarted {
                extension_id,
                run_id,
            } => self.extensions_panel.update(cx, |panel, cx| {
                panel.set_status(
                    &extension_id,
                    i18n::text_args(
                        self.locale,
                        "extensions-status-run-running",
                        &[("run_id", &run_id.to_string())],
                    ),
                    cx,
                )
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
                    .map(|error| {
                        i18n::text_args(
                            self.locale,
                            "extensions-status-run-failed",
                            &[
                                ("run_id", &run_id.to_string()),
                                ("error", &error),
                            ],
                        )
                    })
                    .unwrap_or_else(|| {
                        i18n::text_args(
                            self.locale,
                            "extensions-status-run-completed",
                            &[("run_id", &run_id.to_string())],
                        )
                    });
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
                self.extension_pending_origins.insert(
                    tab_id,
                    (origin_extension_id, origin_run_id, Instant::now()),
                );
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
