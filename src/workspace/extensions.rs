//! Declarative management page for installed Lua extensions.

use std::collections::BTreeMap;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    v_flex,
};

use crate::core::config::AppConfig;
use crate::core::extension::{
    ExtensionRunRecord, ExtensionSettings, RepositoryRunResult,
    SettingDefinition, SettingValue,
};
use crate::core::i18n::Locale;
use crate::extension::{ExtensionDefinition, load_run_history};

#[derive(Clone, Debug)]
pub enum ExtensionsPanelEvent {
    Close,
    InstallDirectory,
    Reload,
    Uninstall(String),
    RunNow(String),
    EnabledChanged {
        extension_id: String,
        enabled: bool,
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

pub struct ExtensionsPanel {
    locale: Locale,
    rows: Vec<ExtensionRow>,
    inputs: BTreeMap<(String, String), Entity<InputState>>,
    statuses: BTreeMap<String, String>,
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
        let (rows, inputs) = Self::build_rows(definitions, config, window, cx);
        Self {
            locale,
            rows,
            inputs,
            statuses: BTreeMap::new(),
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
    ) {
        let mut rows = Vec::new();
        let mut inputs = BTreeMap::new();
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
                        | SettingDefinition::Time { .. }
                ) {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx).default_value(text.clone())
                    });
                    let extension_id = id.clone();
                    let key_for_event = key.clone();
                    let is_time =
                        matches!(setting, SettingDefinition::Time { .. });
                    cx.subscribe(&input, move |_panel, state, event, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let value = state.read(cx).value().trim().to_string();
                        cx.emit(ExtensionsPanelEvent::SettingChanged {
                            extension_id: extension_id.clone(),
                            key: key_for_event.clone(),
                            value: if is_time {
                                SettingValue::Time(value)
                            } else {
                                SettingValue::String(value)
                            },
                        });
                    })
                    .detach();
                    inputs.insert((id.clone(), key.clone()), input);
                }
            }
            rows.push(ExtensionRow {
                definition,
                settings,
                history: load_run_history(&id).unwrap_or_default(),
            });
        }
        (rows, inputs)
    }

    pub fn replace_definitions(
        &mut self,
        definitions: Vec<ExtensionDefinition>,
        config: &AppConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (rows, inputs) = Self::build_rows(definitions, config, window, cx);
        self.rows = rows;
        self.inputs = inputs;
        self.statuses.retain(|id, _| {
            self.rows
                .iter()
                .any(|row| row.definition.package.manifest.id == *id)
        });
        cx.notify();
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
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

    pub fn update_flags(
        &mut self,
        extension_id: &str,
        enabled: bool,
        trusted: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.definition.package.manifest.id == extension_id)
        {
            row.settings.enabled = enabled;
            row.settings.trusted = trusted;
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
        if let Some(input) = self.inputs.get(&(id, key.to_string())) {
            line = line.child(Input::new(input).w_full());
        } else if let SettingDefinition::Boolean { default, .. } = definition {
            let current = match value {
                Some(SettingValue::Boolean(current)) => *current,
                _ => *default,
            };
            let panel = cx.entity();
            let extension_id = row.definition.package.manifest.id.clone();
            let key = key.to_string();
            line = line.child(
                Button::new(SharedString::from(format!(
                    "extension-setting-{extension_id}-{key}"
                )))
                .label(current.to_string())
                .ghost()
                .small()
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
        } else if let SettingDefinition::Select {
            options, default, ..
        } = definition
        {
            let current = match value {
                Some(SettingValue::Select(current)) => current.clone(),
                _ => default.clone(),
            };
            let next = if options.is_empty() {
                current.clone()
            } else {
                options
                    .iter()
                    .position(|option| option.value == current)
                    .and_then(|index| options.get((index + 1) % options.len()))
                    .map(|option| option.value.clone())
                    .unwrap_or_else(|| options[0].value.clone())
            };
            let panel = cx.entity();
            let extension_id = row.definition.package.manifest.id.clone();
            let key = key.to_string();
            line = line.child(
                Button::new(SharedString::from(format!(
                    "extension-setting-{extension_id}-{key}"
                )))
                .label(current)
                .ghost()
                .small()
                .on_click(move |_event, _window, cx| {
                    panel.update(cx, |_panel, cx| {
                        cx.emit(ExtensionsPanelEvent::SettingChanged {
                            extension_id: extension_id.clone(),
                            key: key.clone(),
                            value: SettingValue::Select(next.clone()),
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
        let rows = self.rows.iter().map(|row| {
            let id = row.definition.package.manifest.id.clone();
            let id_for_run = id.clone();
            let id_for_enable = id.clone();
            let id_for_trust = id.clone();
            let panel_for_enable = this.clone();
            let panel_for_trust = this.clone();
            let panel_for_run = this.clone();
            let panel_for_uninstall = this.clone();
            let id_for_uninstall = id.clone();
            let enabled_label = if row.settings.enabled { "Disable" } else { "Enable" };
            let trust_label = if row.settings.trusted { "Trusted" } else { "Trust" };
            let source = format!("source: {:?} · fingerprint: {}", row.definition.package.source, row.definition.package.fingerprint);
            let path = row.definition.package.root.as_ref().map(|path| path.display().to_string()).unwrap_or_else(|| "bundled".to_string());
            let status = self.statuses.get(&id).cloned();
            let history = row.history.iter().rev().take(3).map(|record| {
                let repository_summary = record
                    .repositories
                    .iter()
                    .map(|repository| {
                        let result = match &repository.result {
                            RepositoryRunResult::Success { summary } => {
                                format!("ok: {summary}")
                            }
                            RepositoryRunResult::Failed { code, summary } => {
                                format!("failed ({code}): {summary}")
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
                        "run {} · {} · {}",
                        record.run_id, record.summary, repository_summary
                    )))
            }).collect::<Vec<_>>();
            let can_uninstall = !row.definition.package.bundled;
            let settings = row.definition.package.manifest.settings.iter().map(|(key, definition)| {
                self.render_setting(row, key, definition, &colors, cx)
            }).collect::<Vec<_>>();
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
                        .child(Button::new(SharedString::from(format!("extension-enable-{id}"))).label(enabled_label).ghost().small().on_click(move |_event, _window, cx| {
                            panel_for_enable.update(cx, |panel, cx| cx.emit(ExtensionsPanelEvent::EnabledChanged { extension_id: id_for_enable.clone(), enabled: !panel.rows.iter().find(|row| row.definition.package.manifest.id == id_for_enable).is_some_and(|row| row.settings.enabled) }));
                        }))
                        .child(Button::new(SharedString::from(format!("extension-trust-{id}"))).label(trust_label).ghost().small().on_click(move |_event, _window, cx| {
                            panel_for_trust.update(cx, |panel, cx| cx.emit(ExtensionsPanelEvent::TrustedChanged { extension_id: id_for_trust.clone(), trusted: !panel.rows.iter().find(|row| row.definition.package.manifest.id == id_for_trust).is_some_and(|row| row.settings.trusted) }));
                        }))
                        .child(Button::new(SharedString::from(format!("extension-run-{id}"))).label("Run now").primary().small().on_click(move |_event, _window, cx| {
                            panel_for_run.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::RunNow(id_for_run.clone())));
                        }))
                        .when(can_uninstall, |element| element.child(Button::new(SharedString::from(format!("extension-uninstall-{id}"))).label("Uninstall").ghost().small().on_click(move |_event, _window, cx| {
                            panel_for_uninstall.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::Uninstall(id_for_uninstall.clone())));
                        }))),
                )
                .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(row.definition.package.manifest.description.clone())))
                .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(10.)).child(SharedString::from(source)))
                .child(div().text_color(colors.muted_foreground).text_size(crate::theme::scaled_text_size(10.)).child(SharedString::from(format!("path: {path}"))))
                .when(!row.settings.trusted, |element| element.child(div().text_color(colors.warning).text_size(crate::theme::scaled_text_size(11.)).child("This extension can execute local code. Trust it only if you reviewed the package.")))
                .children(settings)
                .when(!history.is_empty(), |element| {
                    element
                        .child(div().text_color(colors.foreground).text_size(crate::theme::scaled_text_size(11.)).child("Recent runs"))
                        .children(history)
                })
                .when_some(status, |element, status| element.child(div().text_color(colors.blue).text_size(crate::theme::scaled_text_size(11.)).child(SharedString::from(status))))
                .into_any_element()
        }).collect::<Vec<_>>();

        v_flex()
            .id("extensions-panel")
            .size_full()
            .min_h_0()
            .gap_3()
            .p_4()
            .overflow_y_scrollbar()
            .bg(colors.background)
            .child(h_flex().w_full().items_center().child(div().flex_1().font_weight(FontWeight::BOLD).text_color(colors.foreground).text_size(crate::theme::scaled_text_size(18.)).child("Extensions")).child(Button::new("extensions-install").label("Install directory").ghost().on_click({ let this = this.clone(); move |_event, _window, cx| { this.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::InstallDirectory)); }})).child(Button::new("extensions-reload").label("Reload").ghost().on_click({ let this = this.clone(); move |_event, _window, cx| { this.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::Reload)); }})).child(Button::new("extensions-close").label("Close").ghost().on_click(move |_event, _window, cx| { this.update(cx, |_panel, cx| cx.emit(ExtensionsPanelEvent::Close)); })))
            .child(div().text_color(colors.warning).text_size(crate::theme::scaled_text_size(12.)).child("Trusted extensions run with full Lua standard libraries and may execute local commands."))
            .children(rows)
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
