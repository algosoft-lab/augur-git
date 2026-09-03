//! Shortcuts settings section: editable key combinations backed by the
//! compiled-in system defaults plus the user `keybindings.json` overrides.

use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent},
    v_flex,
};

use crate::core::i18n;
use crate::git::shared;
use crate::theme::scaled_text_size;
use crate::workspace::keymap;

use super::{SettingsPanel, SettingsPanelEvent};

impl SettingsPanel {
    /// Commit user edits on Enter or blur so partial keystroke sequences are
    /// never bound mid-typing; live typing only drives validation hints.
    pub(super) fn wire_shortcut_subscriptions(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        for (command, input) in self.shortcut_inputs.clone() {
            let command_for_events = command.clone();
            cx.subscribe(&input, move |panel, state, event, cx| {
                let command = command_for_events.clone();
                match event {
                    InputEvent::Change => {
                        panel.validate_shortcut(
                            &command,
                            state.read(cx).value().as_ref(),
                            cx,
                        );
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        let value = state.read(cx).value().trim().to_string();
                        match keymap::parse_combo_list(&value) {
                            Ok(keys) => {
                                panel.shortcut_errors.remove(&command);
                                if keys != keymap::resolved_keys(cx, &command) {
                                    cx.emit(
                                        SettingsPanelEvent::ShortcutChanged {
                                            command,
                                            keys,
                                        },
                                    );
                                }
                            }
                            Err(invalid) => {
                                panel
                                    .shortcut_errors
                                    .insert(command, invalid.0);
                            }
                        }
                        cx.notify();
                    }
                    _ => {}
                }
            })
            .detach();
        }
    }

    fn validate_shortcut(
        &mut self,
        command: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        let had_error = self.shortcut_errors.contains_key(command);
        match keymap::parse_combo_list(value.trim()) {
            Ok(_) => {
                self.shortcut_errors.remove(command);
            }
            Err(invalid) => {
                self.shortcut_errors.insert(command.to_string(), invalid.0);
            }
        }
        if had_error != self.shortcut_errors.contains_key(command) {
            cx.notify();
        }
    }

    /// Re-read the keymap state and sync the editable values (used after the
    /// workspace applies or resets an override).
    pub(crate) fn sync_shortcuts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (command, input) in self.shortcut_inputs.clone() {
            let text = keymap::resolved_display(cx, &command);
            input.update(cx, |state, cx| {
                if state.value().as_ref() != text.as_str() {
                    state.set_value(text, window, cx);
                }
            });
        }
        self.shortcut_errors.clear();
        cx.notify();
    }

    pub(super) fn render_shortcuts_section(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors.clone();
        let panel = cx.entity();
        let mut section = v_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .text_size(scaled_text_size(20.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(
                        self.locale,
                        "settings-shortcuts",
                    ))),
            )
            .child(
                div()
                    .text_size(scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(i18n::text(
                        self.locale,
                        "shortcut-edit-description",
                    ))),
            );
        for spec in keymap::COMMANDS {
            let command = spec.id;
            let Some((_, input)) = self
                .shortcut_inputs
                .iter()
                .find(|(entry, _)| entry.as_str() == command)
            else {
                continue;
            };
            let input = input.clone();
            let overridden = keymap::is_overridden(cx, command);
            let reset_command = command.to_string();
            let reset_panel = panel.clone();
            let reset = Button::new(SharedString::from(format!(
                "shortcut-reset-{command}"
            )))
            .label(shared(i18n::text(self.locale, "shortcut-reset")))
            .ghost()
            .small()
            .disabled(!overridden)
            .on_click(move |_event, _window, cx| {
                let command = reset_command.clone();
                reset_panel.update(cx, |_panel, cx| {
                    cx.emit(SettingsPanelEvent::ShortcutReset(command));
                });
            });
            section = section.child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(Self::field(
                        i18n::text(self.locale, spec.label_key),
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&input).w_full()),
                            )
                            .child(reset)
                            .into_any_element(),
                        colors.foreground,
                    ))
                    .child(
                        div()
                            .text_size(scaled_text_size(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text_args(
                                self.locale,
                                "shortcut-default-hint",
                                &[(
                                    "keys",
                                    &keymap::default_display(cx, command),
                                )],
                            ))),
                    ),
            );
            if let Some(combo) = self.shortcut_errors.get(command) {
                section = section.child(
                    div()
                        .text_size(scaled_text_size(11.))
                        .text_color(colors.red)
                        .child(shared(i18n::text_args(
                            self.locale,
                            "shortcut-invalid-combo",
                            &[("combo", combo)],
                        ))),
                );
            }
        }
        section.into_any_element()
    }
}
