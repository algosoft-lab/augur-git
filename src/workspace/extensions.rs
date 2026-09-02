//! Declarative management page for installed Lua extensions.

use std::collections::BTreeMap;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, IndexPath, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
    v_flex,
};

use crate::core::config::AppConfig;
use crate::core::extension::{
    EventTrigger, ExtensionRunRecord, ExtensionRunTrigger, ExtensionSettings,
    ExtensionSource, RepositoryRunResult, SettingDefinition, SettingValue,
};
use crate::core::i18n::{self, Locale};
use crate::extension::{ExtensionDefinition, load_run_history};

#[derive(Clone, Debug)]
pub enum ExtensionsPanelEvent {
    Close,
    InstallDirectory,
    ConfirmInstall(String),
    CancelInstall,
    Reload,
    Uninstall(String),
    RunNow(String),
    Cancel(String),
    SaveSettings(String),
    SubscriptionChanged {
        extension_id: String,
        trigger_id: String,
        subscribed: bool,
    },
    TrustedChanged {
        extension_id: String,
        trusted: bool,
    },
    SettingChanged {
        extension_id: String,
        key: String,
        value: SettingValue,
    },
}

struct ExtensionRow {
    definition: ExtensionDefinition,
    settings: ExtensionSettings,
    history: Vec<ExtensionRunRecord>,
}

#[derive(Clone, Debug)]
struct ExtensionSelectOption {
    value: String,
    label: SharedString,
}

impl SearchableListItem for ExtensionSelectOption {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

pub struct ExtensionsPanel {
    locale: Locale,
    rows: Vec<ExtensionRow>,
    selected_extension: Option<String>,
    inputs: BTreeMap<(String, String), Entity<InputState>>,
    selects: BTreeMap<
        (String, String),
        Entity<SelectState<Vec<ExtensionSelectOption>>>,
    >,
    statuses: BTreeMap<String, String>,
    setting_errors: BTreeMap<(String, String), String>,
    trust_confirmations: std::collections::BTreeSet<String>,
    pending_install: Option<String>,
}

impl EventEmitter<ExtensionsPanelEvent> for ExtensionsPanel {}

impl ExtensionsPanel {
    pub fn new(
        definitions: Vec<ExtensionDefinition>,
        config: &AppConfig,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (rows, inputs, selects) =
            Self::build_rows(definitions, config, window, cx);
        let selected_extension = rows
            .first()
            .map(|row| row.definition.package.manifest.id.clone());
        Self {
            locale,
            rows,
            selected_extension,
            inputs,
            selects,
            statuses: BTreeMap::new(),
            setting_errors: BTreeMap::new(),
            trust_confirmations: Default::default(),
            pending_install: None,
        }
    }

    fn build_rows(
        definitions: Vec<ExtensionDefinition>,
        config: &AppConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        Vec<ExtensionRow>,
        BTreeMap<(String, String), Entity<InputState>>,
        BTreeMap<
            (String, String),
            Entity<SelectState<Vec<ExtensionSelectOption>>>,
        >,
    ) {
        let mut rows = Vec::new();
        let mut inputs = BTreeMap::new();
        let mut selects = BTreeMap::new();
        for definition in definitions {
            let id = definition.package.manifest.id.clone();
            let settings = config
                .extensions
                .get(&id)
                .cloned()
                .unwrap_or_else(|| {
                    ExtensionSettings::with_defaults(
                        &definition.package.manifest,
                    )
                })
                .normalized_for(&definition.package.manifest);
            for (key, setting) in &definition.package.manifest.settings {
                let text = settings
                    .values
                    .get(key)
                    .map(setting_display)
                    .unwrap_or_default();
                if matches!(
                    setting,
                    SettingDefinition::String { .. }
                        | SettingDefinition::Integer { .. }
                        | SettingDefinition::Time { .. }
                ) {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx).default_value(text.clone())
                    });
                    let extension_id = id.clone();
                    let key_for_event = key.clone();
                    let is_time =
                        matches!(setting, SettingDefinition::Time { .. });
                    let is_integer =
                        matches!(setting, SettingDefinition::Integer { .. });
                    cx.subscribe(&input, move |_panel, state, event, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let value = state.read(cx).value().trim().to_string();
                        let setting_value = if is_time {
                            SettingValue::Time(value)
                        } else if is_integer {
                            value
                                .parse::<i64>()
                                .map(SettingValue::Integer)
                                .unwrap_or(SettingValue::String(value))
                        } else {
                            SettingValue::String(value)
                        };
                        cx.emit(ExtensionsPanelEvent::SettingChanged {
                            extension_id: extension_id.clone(),
                            key: key_for_event.clone(),
                            value: setting_value,
                        });
                    })
                    .detach();
                    inputs.insert((id.clone(), key.clone()), input);
                }
                if let SettingDefinition::Select {
                    options, default, ..
                } = setting
                {
                    let select_options = options
                        .iter()
                        .map(|option| ExtensionSelectOption {
                            value: option.value.clone(),
                            label: SharedString::from(option.label.clone()),
                        })
                        .collect::<Vec<_>>();
                    let current = match settings.values.get(key) {
                        Some(SettingValue::Select(value)) => value.clone(),
                        _ => default.clone(),
                    };
                    let selected_index = select_options
                        .iter()
                        .position(|option| option.value == current)
                        .map(|index| IndexPath::default().row(index));
                    let state = cx.new(|cx| {
                        SelectState::new(
                            select_options.clone(),
                            selected_index,
                            window,
                            cx,
                        )
                    });
                    let extension_id = id.clone();
                    let key_for_event = key.clone();
                    cx.subscribe(&state, move |_panel, _, event, cx| {
                        let SelectEvent::Confirm(Some(value)) = event else {
                            return;
                        };
                        cx.emit(ExtensionsPanelEvent::SettingChanged {
                            extension_id: extension_id.clone(),
                            key: key_for_event.clone(),
                            value: SettingValue::Select(value.clone()),
                        });
                    })
                    .detach();
                    selects.insert((id.clone(), key.clone()), state);
                }
            }
            rows.push(ExtensionRow {
                definition,
                settings,
                history: load_run_history(&id).unwrap_or_default(),
            });
        }
        (rows, inputs, selects)
    }

    pub fn replace_definitions(
        &mut self,
        definitions: Vec<ExtensionDefinition>,
        config: &AppConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (rows, inputs, selects) =
            Self::build_rows(definitions, config, window, cx);
        let selected_extension = self
            .selected_extension
            .clone()
            .filter(|id| {
                rows.iter()
                    .any(|row| row.definition.package.manifest.id == *id)
            })
            .or_else(|| {
                rows.first()
                    .map(|row| row.definition.package.manifest.id.clone())
            });
        self.rows = rows;
        self.selected_extension = selected_extension;
        self.inputs = inputs;
        self.selects = selects;
        self.statuses.retain(|id, _| {
            self.rows
                .iter()
                .any(|row| row.definition.package.manifest.id == *id)
        });
        self.setting_errors.clear();
        self.trust_confirmations.retain(|id| {
            self.rows
                .iter()
                .any(|row| row.definition.package.manifest.id == *id)
        });
        self.pending_install = self.pending_install.clone().filter(|id| {
            self.rows
                .iter()
                .any(|row| row.definition.package.manifest.id == *id)
        });
        cx.notify();
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    pub fn select_extension(
        &mut self,
        extension_id: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .rows
            .iter()
            .any(|row| row.definition.package.manifest.id == extension_id)
        {
            self.selected_extension = Some(extension_id);
            cx.notify();
        }
    }

    fn handle_trust_click(
        &mut self,
        extension_id: String,
        cx: &mut Context<Self>,
    ) {
        let trusted = self
            .rows
            .iter()
            .find(|row| row.definition.package.manifest.id == extension_id)
            .is_some_and(|row| row.settings.trusted);
        if trusted {
            cx.emit(ExtensionsPanelEvent::TrustedChanged {
                extension_id,
                trusted: false,
            });
        } else if self.trust_confirmations.insert(extension_id.clone()) {
            cx.notify();
        } else {
            self.trust_confirmations.remove(&extension_id);
            cx.emit(ExtensionsPanelEvent::TrustedChanged {
                extension_id,
                trusted: true,
            });
        }
    }

    pub fn set_status(
        &mut self,
        extension_id: &str,
        status: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.statuses
            .insert(extension_id.to_string(), status.into());
        cx.notify();
    }

    pub fn set_pending_install(
        &mut self,
        extension_id: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.pending_install = Some(extension_id.into());
        cx.notify();
    }

    pub fn clear_pending_install(&mut self, cx: &mut Context<Self>) {
        if self.pending_install.take().is_some() {
            cx.notify();
        }
    }

    pub fn set_setting_error(
        &mut self,
        extension_id: &str,
        key: &str,
        error: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.setting_errors
            .insert((extension_id.to_string(), key.to_string()), error.into());
        cx.notify();
    }

    pub fn clear_setting_error(
        &mut self,
        extension_id: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        if self
            .setting_errors
            .remove(&(extension_id.to_string(), key.to_string()))
            .is_some()
        {
            cx.notify();
        }
    }

    pub fn update_setting(
        &mut self,
        extension_id: &str,
        key: &str,
        value: SettingValue,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.definition.package.manifest.id == extension_id)
        {
            row.settings.values.insert(key.to_string(), value);
            cx.notify();
        }
    }

    pub fn update_trust(
        &mut self,
        extension_id: &str,
        trusted: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.definition.package.manifest.id == extension_id)
        {
            row.settings.trusted = trusted;
            if !trusted {
                row.settings.subscribed_triggers.clear();
            }
            cx.notify();
        }
    }

    pub fn update_subscription(
        &mut self,
        extension_id: &str,
        trigger_id: &str,
        subscribed: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.definition.package.manifest.id == extension_id)
        {
            if subscribed {
                row.settings
                    .subscribed_triggers
                    .insert(trigger_id.to_string());
            } else {
                row.settings.subscribed_triggers.remove(trigger_id);
            }
            cx.notify();
        }
    }

    pub fn append_history(
        &mut self,
        extension_id: &str,
        record: ExtensionRunRecord,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.definition.package.manifest.id == extension_id)
        {
            row.history.push(record);
            if row.history.len()
                > crate::core::extension::MAX_EXTENSION_RUN_HISTORY
            {
                let keep_from = row.history.len()
                    - crate::core::extension::MAX_EXTENSION_RUN_HISTORY;
                row.history.drain(..keep_from);
            }
            cx.notify();
        }
    }

    fn render_setting(
        &self,
        row: &ExtensionRow,
        key: &str,
        definition: &SettingDefinition,
        colors: &gpui_component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> AnyElement {
        let value = row.settings.values.get(key);
        let label = definition.label().to_string();
        let description = match definition {
            SettingDefinition::String { description, .. }
            | SettingDefinition::Integer { description, .. }
            | SettingDefinition::Boolean { description, .. }
            | SettingDefinition::Time { description, .. }
            | SettingDefinition::Select { description, .. } => {
                description.clone()
            }
        };
        let mut line = v_flex().w_full().gap_1().child(
            div()
                .text_color(colors.foreground)
                .text_size(crate::theme::scaled_text_size(12.))
                .child(SharedString::from(label)),
        );
        if let Some(description) = description {
            line = line.child(
                div()
                    .text_color(colors.muted_foreground)
                    .text_size(crate::theme::scaled_text_size(11.))
                    .child(SharedString::from(description)),
            );
        }
        let id = row.definition.package.manifest.id.clone();
        let setting_error = self
            .setting_errors
            .get(&(id.clone(), key.to_string()))
            .cloned();
        if let Some(input) = self.inputs.get(&(id.clone(), key.to_string())) {
            line = line.child(Input::new(input).w_full());
        } else if let Some(select) = self
            .selects
            .get(&(row.definition.package.manifest.id.clone(), key.to_string()))
        {
            line = line.child(Select::new(select).w_full().into_any_element());
        } else if let SettingDefinition::Boolean { default, .. } = definition {
            let current = match value {
                Some(SettingValue::Boolean(current)) => *current,
                _ => *default,
            };
            let panel = cx.entity();
            let extension_id = row.definition.package.manifest.id.clone();
            let key = key.to_string();
            line = line.child(
                Switch::new(SharedString::from(format!(
                    "extension-setting-{extension_id}-{key}"
                )))
                .small()
                .checked(current)
                .on_click(move |_event, _window, cx| {
                    panel.update(cx, |_panel, cx| {
                        cx.emit(ExtensionsPanelEvent::SettingChanged {
                            extension_id: extension_id.clone(),
                            key: key.clone(),
                            value: SettingValue::Boolean(!current),
                        });
                    });
                }),
            );
        } else {
            line = line.child(
                div()
                    .text_color(colors.muted_foreground)
                    .text_size(crate::theme::scaled_text_size(12.))
                    .child(SharedString::from(
                        value.map(setting_display).unwrap_or_default(),
                    )),
            );
        }
        if let Some(error) = setting_error {
            line = line.child(
                div()
                    .text_color(colors.red)
                    .text_size(crate::theme::scaled_text_size(11.))
                    .child(SharedString::from(error)),
            );
        }
        line.into_any_element()
    }
}

impl Render for ExtensionsPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let locale = self.locale;
        let title = i18n::text(locale, "extensions-title");
        let install_label = i18n::text(locale, "extensions-install");
        let confirm_update_label =
            i18n::text(locale, "extensions-confirm-update");
        let cancel_update_label =
            i18n::text(locale, "extensions-cancel-update");
        let update_pending_label =
            i18n::text(locale, "extensions-update-pending");
        let reload_label = i18n::text(locale, "extensions-reload");
        let close_label = i18n::text(locale, "extensions-close");
        let run_once_label = i18n::text(locale, "extensions-run-once");
        let cancel_label = i18n::text(locale, "extensions-cancel");
        let save_settings_label =
            i18n::text(locale, "extensions-save-settings");
        let trust_label_text = i18n::text(locale, "extensions-trust");
        let trusted_label = i18n::text(locale, "extensions-trusted");
        let subscribe_label = i18n::text(locale, "extensions-subscribe");
        let subscribed_label = i18n::text(locale, "extensions-subscribed");
        let events_title = i18n::text(locale, "extensions-event-subscriptions");
        let settings_title = i18n::text(locale, "extensions-settings");
        let history_title = i18n::text(locale, "extensions-recent-runs");
        let manual_capability = i18n::text(locale, "extensions-manual");
        let events_capability = i18n::text(locale, "extensions-events");
        let source_label = i18n::text(locale, "extensions-source");
        let fingerprint_label = i18n::text(locale, "extensions-fingerprint");
        let version_label = i18n::text(locale, "extensions-version");
        let author_label = i18n::text(locale, "extensions-author");
        let path_label = i18n::text(locale, "extensions-path");
        let bundled_label = i18n::text(locale, "extensions-bundled");
        let local_source_label =
            i18n::text(locale, "extensions-local-directory");
        let uninstall_label = i18n::text(locale, "extensions-uninstall");
        let permission_warning =
            i18n::text(locale, "extensions-permission-warning");
        let untrusted_warning =
            i18n::text(locale, "extensions-untrusted-warning");
        let selected_id = self.selected_extension.clone().or_else(|| {
            self.rows
                .first()
                .map(|row| row.definition.package.manifest.id.clone())
        });
        let pending_install = self.pending_install.clone();
        let details = self
            .rows
            .iter()
            .filter(|row| {
                selected_id.as_deref()
                    == Some(row.definition.package.manifest.id.as_str())
            })
            .map(|row| {
            let id = row.definition.package.manifest.id.clone();
            let id_for_run = id.clone();
            let id_for_trust = id.clone();
            let panel_for_trust = this.clone();
            let panel_for_run = this.clone();
            let panel_for_cancel = this.clone();
            let panel_for_save = this.clone();
            let panel_for_uninstall = this.clone();
            let id_for_uninstall = id.clone();
            let id_for_cancel = id.clone();
            let id_for_save = id.clone();
            let trust_label = if row.settings.trusted {
                trusted_label.clone()
            } else {
                trust_label_text.clone()
            };
            let source_name = match row.definition.package.source {
                ExtensionSource::Bundled => bundled_label.clone(),
                ExtensionSource::LocalDirectory => local_source_label.clone(),
            };
            let source = format!(
                "{source_label}: {source_name} · {fingerprint_label}: {}",
                row.definition.package.fingerprint
            );
            let path = row
                .definition
                .package
                .root
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| bundled_label.clone());
            let author = row.definition.package.manifest.author.clone().unwrap_or_else(|| "—".to_string());
            let version = row.definition.package.manifest.version.clone();
            let status = self.statuses.get(&id).cloned();
            let capabilities = [
                row.definition
                    .package
                    .manifest
                    .manual_handler
                    .as_ref()
                        .map(|_| manual_capability.as_str()),
                (!row.definition.package.manifest.event_triggers().is_empty())
                    .then_some(events_capability.as_str()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let history = row.history.iter().rev().take(3).map(|record| {
                let trigger = trigger_display(locale, &record.trigger);
                let repository_summary = record
                    .repositories
                    .iter()
                    .map(|repository| {
                        let result = match &repository.result {
                            RepositoryRunResult::Success { summary } => {
                                i18n::text_args(
                                    locale,
                                    "extensions-history-ok",
                                    &[("summary", summary)],
                                )
                            }
                            RepositoryRunResult::Failed { code, summary } => {
                                i18n::text_args(
                                    locale,
                                    "extensions-history-failed",
                                    &[("code", code), ("summary", summary)],
                                )
                            }
                        };
                        format!("{} — {result}", repository.display_name)
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                div()
                    .text_color(colors.muted_foreground)
                    .text_size(crate::theme::scaled_text_size(10.))
                    .child(SharedString::from(format!(
                        "{} · {}",
                        i18n::text_args(
                            locale,
                            "extensions-history-run",
                            &[
                                ("run_id", &record.run_id.to_string()),
                                ("trigger", &trigger),
                                ("summary", &record.summary),
                            ],
                        ),
                        repository_summary
                    )))
            }).collect::<Vec<_>>();
            let can_uninstall = !row.definition.package.bundled;
            let has_settings =
                !row.definition.package.manifest.settings.is_empty();
            let settings = row.definition.package.manifest.settings.iter().map(|(key, definition)| {
                self.render_setting(row, key, definition, &colors, cx)
            }).collect::<Vec<_>>();
            let events = row
                .definition
                .package
                .manifest
                .event_triggers()
                .into_iter()
                .map(|trigger| {
                    let trigger_id = trigger.id.clone();
                    let extension_id = id.clone();
                    let subscribed = row.settings.is_subscribed(&trigger_id);
                    let panel = this.clone();
                    let subscription_label = if subscribed {
                        subscribed_label.clone()
                    } else {
                        subscribe_label.clone()
                    };
                    let mut label =
                        format!("{} · {}", trigger.label(), trigger.event_type);
                    if let Some(description) = trigger.description.as_deref() {
                        label.push_str(" — ");
                        label.push_str(description);
                    }
                    if let Some(schedule) =
                        next_event_hint(locale, &trigger, &row.settings)
                    {
                        label.push_str(" · ");
                        label.push_str(&schedule);
                    }
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_color(colors.foreground)
                                .text_size(crate::theme::scaled_text_size(11.))
                                .child(SharedString::from(label)),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    Switch::new(SharedString::from(format!(
                                        "extension-event-{extension_id}-{trigger_id}"
                                    )))
                                    .small()
                                    .checked(subscribed)
                                    .on_click(move |_event, _window, cx| {
                                        panel.update(cx, |_panel, cx| {
                                            cx.emit(ExtensionsPanelEvent::SubscriptionChanged {
                                                extension_id: extension_id.clone(),
                                                trigger_id: trigger_id.clone(),
                                                subscribed: !subscribed,
                                            });
                                        });
                                    }),
                                )
                                .child(
                                    div()
                                        .text_color(colors.muted_foreground)
                                        .text_size(crate::theme::scaled_text_size(10.))
                                        .child(SharedString::from(subscription_label)),
                                ),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            v_flex()
                .w_full()
                .gap_2()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().font_weight(FontWeight::BOLD).text_color(colors.foreground).child(SharedString::from(row.definition.package.manifest.name.clone())))
                        .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(10.)).child(SharedString::from(capabilities)))
                        .child(Button::new(SharedString::from(format!("extension-trust-{id}"))).label(trust_label).ghost().small().on_click(move |_event, _window, cx| {
                            panel_for_trust.update(cx, |panel, cx| panel.handle_trust_click(id_for_trust.clone(), cx));
                        }))
                        .child(Button::new(SharedString::from(format!("extension-run-{id}"))).label(run_once_label.clone()).primary().small().on_click(move |_event, _window, cx| {
                            panel_for_run.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::RunNow(id_for_run.clone())));
                        }))
                .child(Button::new(SharedString::from(format!("extension-cancel-{id}"))).label(cancel_label.clone()).ghost().small().on_click(move |_event, _window, cx| {
                    panel_for_cancel.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::Cancel(id_for_cancel.clone())));
                }))
                .when(has_settings, |element| element.child(Button::new(SharedString::from(format!("extension-save-{id}"))).label(save_settings_label.clone()).ghost().small().on_click(move |_event, _window, cx| {
                    panel_for_save.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::SaveSettings(id_for_save.clone())));
                })))
                        .when(can_uninstall, |element| element.child(Button::new(SharedString::from(format!("extension-uninstall-{id}"))).label(uninstall_label.clone()).ghost().small().on_click(move |_event, _window, cx| {
                            panel_for_uninstall.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::Uninstall(id_for_uninstall.clone())));
                        }))),
                )
                .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(row.definition.package.manifest.description.clone())))
                .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(10.)).child(SharedString::from(source)))
                .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(10.)).child(SharedString::from(format!("{version_label}: {version} · {author_label}: {author}"))))
                .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(10.)).child(SharedString::from(format!("{path_label}: {path}"))))
                .when(!row.settings.trusted, |element| {
                    element
                        .child(div().text_color(colors.warning).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(untrusted_warning.clone())))
                        .when(self.trust_confirmations.contains(&id), |element| {
                            element.child(div().text_color(colors.warning).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(i18n::text(locale, "extensions-permission-confirm"))))
                        })
                })
                .when(!events.is_empty(), |element| {
                    element
                        .child(div().text_color(colors.foreground).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(events_title.clone())))
                        .children(events)
                })
                .when(!settings.is_empty(), |element| {
                    element
                        .child(div().text_color(colors.foreground).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(settings_title.clone())))
                        .children(settings)
                })
                .when(!history.is_empty(), |element| {
                    element
                        .child(div().text_color(colors.foreground).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(history_title.clone())))
                        .children(history)
                })
                .when_some(status, |element, status| element.child(div().text_color(colors.blue).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(status))))
                .into_any_element()
            })
            .collect::<Vec<_>>();
        let extension_list = self
            .rows
            .iter()
            .map(|row| {
                let id = row.definition.package.manifest.id.clone();
                let name = row.definition.package.manifest.name.clone();
                let capabilities = [
                    row.definition
                        .package
                        .manifest
                        .manual_handler
                        .as_ref()
                        .map(|_| manual_capability.as_str()),
                    (!row
                        .definition
                        .package
                        .manifest
                        .event_triggers()
                        .is_empty())
                    .then_some(events_capability.as_str()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                let selected = selected_id.as_deref() == Some(id.as_str());
                let panel = this.clone();
                v_flex()
                    .w_full()
                    .gap_1()
                    .p_2()
                    .rounded_sm()
                    .border_1()
                    .border_color(if selected {
                        colors.accent
                    } else {
                        colors.border
                    })
                    .child(
                        Button::new(SharedString::from(format!(
                            "extension-select-{id}"
                        )))
                        .label(name)
                        .ghost()
                        .small()
                        .on_click(
                            move |_event, _window, cx| {
                                panel.update(cx, |panel, cx| {
                                    panel.select_extension(id.clone(), cx);
                                });
                            },
                        ),
                    )
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .text_size(crate::theme::scaled_text_size(10.))
                            .child(SharedString::from(capabilities)),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        let header = h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .text_size(crate::theme::scaled_text_size(18.))
                    .child(SharedString::from(title)),
            )
            .child(
                Button::new("extensions-install")
                    .label(install_label)
                    .ghost()
                    .on_click({
                        let this = this.clone();
                        move |_event, _window, cx| {
                            this.update(cx, |_panel, cx| {
                                cx.emit(ExtensionsPanelEvent::InstallDirectory)
                            });
                        }
                    }),
            )
            .child(
                Button::new("extensions-reload")
                    .label(reload_label)
                    .ghost()
                    .on_click({
                        let this = this.clone();
                        move |_event, _window, cx| {
                            this.update(cx, |_panel, cx| {
                                cx.emit(ExtensionsPanelEvent::Reload)
                            });
                        }
                    }),
            )
            .child(
                Button::new("extensions-close")
                    .label(close_label)
                    .ghost()
                    .on_click(move |_event, _window, cx| {
                        this.update(cx, |_panel, cx| {
                            cx.emit(ExtensionsPanelEvent::Close)
                        });
                    }),
            );
        let pending_controls = pending_install.map(|extension_id| {
            let confirm_panel = cx.entity();
            let cancel_panel = confirm_panel.clone();
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .text_color(colors.warning)
                        .text_size(crate::theme::scaled_text_size(11.))
                        .child(SharedString::from(
                            update_pending_label.clone(),
                        )),
                )
                .child(
                    Button::new("extensions-confirm-update")
                        .label(confirm_update_label.clone())
                        .primary()
                        .small()
                        .on_click(move |_event, _window, cx| {
                            confirm_panel.update(cx, |_panel, cx| {
                                cx.emit(ExtensionsPanelEvent::ConfirmInstall(
                                    extension_id.clone(),
                                ));
                            });
                        }),
                )
                .child(
                    Button::new("extensions-cancel-update")
                        .label(cancel_update_label.clone())
                        .ghost()
                        .small()
                        .on_click(move |_event, _window, cx| {
                            cancel_panel.update(cx, |_panel, cx| {
                                cx.emit(ExtensionsPanelEvent::CancelInstall);
                            });
                        }),
                )
                .into_any_element()
        });

        v_flex()
            .id("extensions-panel")
            .size_full()
            .min_h_0()
            .gap_3()
            .p_4()
            .overflow_y_scrollbar()
            .bg(colors.background)
            .child(header)
            .when_some(pending_controls, |element, controls| {
                element.child(controls)
            })
            .child(
                div()
                    .text_color(colors.warning)
                    .text_size(crate::theme::scaled_text_size(12.))
                    .child(SharedString::from(permission_warning)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(
                        v_flex()
                            .w(px(230.))
                            .min_h_0()
                            .gap_1()
                            .overflow_y_scrollbar()
                            .children(extension_list),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .gap_2()
                            .overflow_y_scrollbar()
                            .children(details),
                    ),
            )
    }
}

fn setting_display(value: &SettingValue) -> String {
    match value {
        SettingValue::String(value)
        | SettingValue::Time(value)
        | SettingValue::Select(value) => value.clone(),
        SettingValue::Integer(value) => value.to_string(),
        SettingValue::Boolean(value) => value.to_string(),
    }
}

fn trigger_display(locale: Locale, trigger: &ExtensionRunTrigger) -> String {
    match trigger {
        ExtensionRunTrigger::Manual => {
            i18n::text(locale, "extensions-history-trigger-manual")
        }
        ExtensionRunTrigger::Schedule {
            trigger_id,
            event_type,
        }
        | ExtensionRunTrigger::Repository {
            trigger_id,
            event_type,
        } => i18n::text_args(
            locale,
            "extensions-history-trigger-event",
            &[("event_type", event_type), ("trigger_id", trigger_id)],
        ),
    }
}

fn next_event_hint(
    locale: Locale,
    trigger: &EventTrigger,
    settings: &ExtensionSettings,
) -> Option<String> {
    match trigger.event_type.as_str() {
        "schedule.daily" => trigger
            .time_setting
            .as_deref()
            .and_then(|key| settings.values.get(key))
            .and_then(|value| match value {
                SettingValue::Time(time) => Some(i18n::text_args(
                    locale,
                    "extensions-daily-at",
                    &[("time", time.as_str())],
                )),
                _ => None,
            }),
        "schedule.interval" => trigger
            .interval_setting
            .as_deref()
            .and_then(|key| settings.values.get(key))
            .and_then(|value| match value {
                SettingValue::Integer(minutes) => Some(i18n::text_args(
                    locale,
                    "extensions-every-minutes",
                    &[("minutes", &minutes.to_string())],
                )),
                _ => None,
            }),
        _ => Some(i18n::text(locale, "extensions-on-event")),
    }
}
