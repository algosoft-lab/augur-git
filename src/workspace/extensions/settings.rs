//! Setting editor construction and per-setting rendering for the extensions panel.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    IndexPath, Sizable,
    input::{Input, InputEvent, InputState},
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
    v_flex,
};

use crate::core::extension::{SelectOption, SettingDefinition, SettingValue};

use super::{ExtensionRow, ExtensionsPanel, ExtensionsPanelEvent};

#[derive(Clone, Debug)]
pub(super) struct ExtensionSelectOption {
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

/// Creates the text editor for a string, integer, or time setting and emits
/// `SettingChanged` whenever its content changes.
pub(super) fn build_text_editor(
    extension_id: &str,
    key: &str,
    text: String,
    is_time: bool,
    is_integer: bool,
    window: &mut Window,
    cx: &mut Context<ExtensionsPanel>,
) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
    let extension_id = extension_id.to_string();
    let key = key.to_string();
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
            key: key.clone(),
            value: setting_value,
        });
    })
    .detach();
    input
}

/// Creates the dropdown editor for a select setting and emits `SettingChanged`
/// when the user confirms a new option.
pub(super) fn build_select_editor(
    extension_id: &str,
    key: &str,
    options: &[SelectOption],
    current: String,
    window: &mut Window,
    cx: &mut Context<ExtensionsPanel>,
) -> Entity<SelectState<Vec<ExtensionSelectOption>>> {
    let select_options = options
        .iter()
        .map(|option| ExtensionSelectOption {
            value: option.value.clone(),
            label: SharedString::from(option.label.clone()),
        })
        .collect::<Vec<_>>();
    let selected_index = select_options
        .iter()
        .position(|option| option.value == current)
        .map(|index| IndexPath::default().row(index));
    let state = cx
        .new(|cx| SelectState::new(select_options, selected_index, window, cx));
    let extension_id = extension_id.to_string();
    let key = key.to_string();
    cx.subscribe(&state, move |_panel, _, event, cx| {
        let SelectEvent::Confirm(Some(value)) = event else {
            return;
        };
        cx.emit(ExtensionsPanelEvent::SettingChanged {
            extension_id: extension_id.clone(),
            key: key.clone(),
            value: SettingValue::Select(value.clone()),
        });
    })
    .detach();
    state
}

/// Renders one declarative setting row: label, optional description, the
/// matching editor, and any pending validation error.
pub(super) fn render_setting(
    panel: &ExtensionsPanel,
    row: &ExtensionRow,
    key: &str,
    definition: &SettingDefinition,
    colors: &gpui_component::theme::ThemeColor,
    cx: &Context<ExtensionsPanel>,
) -> AnyElement {
    let value = row.settings.values.get(key);
    let label = definition.label().to_string();
    let description = match definition {
        SettingDefinition::String { description, .. }
        | SettingDefinition::Integer { description, .. }
        | SettingDefinition::Boolean { description, .. }
        | SettingDefinition::Time { description, .. }
        | SettingDefinition::Select { description, .. } => description.clone(),
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
    let setting_error = panel
        .setting_errors
        .get(&(id.clone(), key.to_string()))
        .cloned();
    if let Some(input) = panel.inputs.get(&(id.clone(), key.to_string())) {
        line = line.child(Input::new(input).w_full());
    } else if let Some(select) = panel
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

pub(super) fn setting_display(value: &SettingValue) -> String {
    match value {
        SettingValue::String(value)
        | SettingValue::Time(value)
        | SettingValue::Select(value) => value.clone(),
        SettingValue::Integer(value) => value.to_string(),
        SettingValue::Boolean(value) => value.to_string(),
    }
}
