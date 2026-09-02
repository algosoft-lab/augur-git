//! Workspace-side lifecycle, scheduling, and event routing for extensions.

use std::time::Duration;

use chrono::Local;
use gpui::*;

use crate::core::extension::{self, ExtensionSettings, SettingValue};
use crate::extension::{
    ExtensionEvent, ExtensionRunRequest, ExtensionTrigger, HostEvent,
    discover_definitions,
};

use super::extensions::ExtensionsPanelEvent;
use super::{TabContent, Workspace};

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
            .collect();
        self.extension_host.set_repositories(snapshots);
        self.extension_host
            .set_agent_settings(self.config.agent.clone());
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
        self.sync_extension_repositories(cx);
        while let Ok(event) = self.host_events.try_recv() {
            self.handle_host_event(event, cx);
        }
        while let Ok(event) = self.extension_events.try_recv() {
            self.handle_extension_event(event, cx);
        }
        let now = Local::now();
        let previous = self.last_extension_tick;
        self.last_extension_tick = now;
        self.schedule_daily_extensions(previous, now, cx);
    }

    fn schedule_daily_extensions(
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
            if !settings.enabled || !settings.trusted {
                continue;
            }
            for trigger in &definition.package.manifest.daily {
                let Some(SettingValue::Time(time)) =
                    settings.values.get(&trigger.time_setting)
                else {
                    continue;
                };
                let Ok(time) = extension::parse_daily_time(time) else {
                    continue;
                };
                if !extension::should_fire_daily(previous, now, time) {
                    continue;
                }
                let occurrence_date = now.date_naive().to_string();
                if settings.last_scheduled_date.as_deref()
                    == Some(&occurrence_date)
                {
                    continue;
                }
                let request = self.extension_request(
                    &id,
                    ExtensionTrigger::Schedule {
                        trigger_id: trigger.id.clone(),
                    },
                    Some(now),
                    trigger.handler.clone(),
                    cx,
                );
                match request {
                    Ok(request) => match self
                        .extension_manager
                        .as_ref()
                        .map(|manager| manager.run(request))
                    {
                        Some(Ok(Some(run_id))) => {
                            if let Some(entry) =
                                self.config.extensions.get_mut(&id)
                            {
                                entry.last_scheduled_date =
                                    Some(occurrence_date.clone());
                            }
                            self.persist_config();
                            log::info!(
                                "[extensions] daily run queued: id={id}, run_id={run_id}"
                            );
                        }
                        Some(Ok(None)) => {
                            if let Some(entry) =
                                self.config.extensions.get_mut(&id)
                            {
                                entry.last_scheduled_date =
                                    Some(occurrence_date.clone());
                            }
                            self.persist_config();
                        }
                        Some(Err(error)) => log::warn!(
                            "[extensions] daily run rejected: id={id}, error={error}"
                        ),
                        None => {}
                    },
                    Err(error) => log::warn!(
                        "[extensions] failed to create daily run: id={id}, error={error}"
                    ),
                }
            }
        }
        let _ = cx;
    }

    fn extension_request(
        &self,
        extension_id: &str,
        trigger: ExtensionTrigger,
        scheduled_at: Option<chrono::DateTime<Local>>,
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
                settings.enabled = *enabled;
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
                    settings.enabled = false;
                }
                self.extensions_panel.update(cx, |panel, cx| {
                    panel.update_flags(
                        extension_id,
                        settings.enabled,
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
                if !settings.trusted || !settings.enabled {
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
            HostEvent::RepositoryChanged { tab_id } => {
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
