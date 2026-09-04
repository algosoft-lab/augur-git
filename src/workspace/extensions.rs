//! Declarative management page for installed Lua extensions.

mod detail;
mod settings;

use std::collections::BTreeMap;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::InputState,
    scroll::ScrollableElement,
    select::SelectState,
    v_flex,
};

use crate::core::config::AppConfig;
use crate::core::extension::{
    ExtensionRunRecord, ExtensionSettings, SettingDefinition, SettingValue,
};
use crate::core::i18n::{self, Locale};
use crate::extension::{ExtensionDefinition, load_run_history};

use settings::ExtensionSelectOption;

#[derive(Clone, Debug)]
pub enum ExtensionsPanelEvent {
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
                if matches!(
                    setting,
                    SettingDefinition::String { .. }
                        | SettingDefinition::Integer { .. }
                        | SettingDefinition::Time { .. }
                ) {
                    let text = settings
                        .values
                        .get(key)
                        .map(settings::setting_display)
                        .unwrap_or_default();
                    let input = settings::build_text_editor(
                        &id,
                        key,
                        text,
                        matches!(setting, SettingDefinition::Time { .. }),
                        matches!(setting, SettingDefinition::Integer { .. }),
                        window,
                        cx,
                    );
                    inputs.insert((id.clone(), key.clone()), input);
                }
                if let SettingDefinition::Select {
                    options, default, ..
                } = setting
                {
                    let current = match settings.values.get(key) {
                        Some(SettingValue::Select(value)) => value.clone(),
                        _ => default.clone(),
                    };
                    let state = settings::build_select_editor(
                        &id, key, options, current, window, cx,
                    );
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
        let manual_capability = i18n::text(locale, "extensions-manual");
        let events_capability = i18n::text(locale, "extensions-events");
        let selected_id = self.selected_extension.clone().or_else(|| {
            self.rows
                .first()
                .map(|row| row.definition.package.manifest.id.clone())
        });
        let details = self
            .rows
            .iter()
            .filter(|row| {
                selected_id.as_deref()
                    == Some(row.definition.package.manifest.id.as_str())
            })
            .map(|row| detail::detail_card(self, &this, row, cx))
            .collect::<Vec<_>>();
        let extension_list = self
            .rows
            .iter()
            .map(|row| {
                let id = row.definition.package.manifest.id.clone();
                let name = row.definition.package.manifest.name.clone();
                let capabilities = capabilities_summary(
                    row,
                    &manual_capability,
                    &events_capability,
                );
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

        v_flex()
            .id("extensions-panel")
            .size_full()
            .min_h_0()
            .gap_3()
            .p_4()
            .overflow_y_scrollbar()
            .bg(colors.background)
            // Input value text inherits the ambient window text color
            // (gpui-component's editor element uses window.text_style()),
            // so the panel must establish a themed default for it.
            .text_color(colors.foreground)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .text_size(crate::theme::scaled_text_size(18.))
                    .child(SharedString::from(title)),
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

/// Builds the "Manual · Events" capability summary shared by the extension
/// list rows and the detail card header.
fn capabilities_summary(
    row: &ExtensionRow,
    manual_label: &str,
    events_label: &str,
) -> String {
    [
        row.definition
            .package
            .manifest
            .manual_handler
            .as_ref()
            .map(|_| manual_label),
        (!row.definition.package.manifest.event_triggers().is_empty())
            .then_some(events_label),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}
