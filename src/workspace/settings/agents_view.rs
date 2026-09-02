//! Render-only view of the Agents settings section.
//!
//! State plumbing, probes, and editors live in `super::agents`; this module
//! turns that state into the card list shown inside the settings panel.

use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    popover::Popover,
    select::Select,
    v_flex,
};

use crate::agent::BuiltInAgent;
use crate::core::i18n;
use crate::git::shared;

use super::SettingsPanel;
use super::SettingsPanelEvent;
use super::agents::agent_profile_options;

impl SettingsPanel {
    pub(super) fn render_agents_section(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors.clone();
        let accent = cx.theme().tokens.accent.color;
        let accent_foreground = cx.theme().accent_foreground;
        let this = cx.entity();
        let enabled = self.agent_settings.enabled_builtins();
        let profiles = agent_profile_options(self.locale, &self.agent_settings);
        let default_id = self.agent_settings.default_profile_id();

        let missing_presets = BuiltInAgent::ALL
            .iter()
            .copied()
            .filter(|agent| !enabled.contains(agent))
            .collect::<Vec<_>>();
        let popover_locale = self.locale;
        let add_popover = Popover::new("settings-agent-add-menu")
            .anchor(Anchor::BottomRight)
            .open(self.agent_add_open)
            .on_open_change({
                let this = this.clone();
                move |open, _window, cx| {
                    this.update(cx, |panel, cx| {
                        panel.agent_add_open = *open;
                        cx.notify();
                    });
                }
            })
            .trigger(
                Button::new("settings-agent-add-trigger")
                    .label(shared(i18n::text(
                        self.locale,
                        "agent-add-agent",
                    )))
                    .ghost()
                    .small()
                    .dropdown_caret(true),
            )
            .content({
                let this = this.clone();
                move |_state, _window, _cx| {
                    let mut menu = v_flex().w(px(220.)).gap_1();
                    for agent in missing_presets.clone() {
                        let add = this.clone();
                        menu = menu.child(
                            Button::new(SharedString::from(format!(
                                "settings-agent-add-{agent_id}",
                                agent_id = agent.id()
                            )))
                            .label(shared(format!(
                                "＋ {}",
                                agent.display_name()
                            )))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .small()
                            .on_click(move |_event, _window, cx| {
                                add.update(cx, |panel, cx| {
                                    panel.agent_add_open = false;
                                    cx.emit(
                                        SettingsPanelEvent::AgentBuiltinAddRequested(agent),
                                    );
                                });
                            }),
                        );
                    }
                    let custom = this.clone();
                    menu.child(
                        Button::new("settings-agent-add-custom")
                            .label(shared(i18n::text(
                                popover_locale,
                                "agent-add-custom-profile",
                            )))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .small()
                            .on_click(move |_event, window, cx| {
                                custom.update(cx, |panel, cx| {
                                    panel.agent_add_open = false;
                                    panel.open_agent_profile_editor(
                                        None, window, cx,
                                    );
                                });
                            }),
                    )
                }
            });

        let header = h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(20.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(self.locale, "settings-agents"))),
            )
            .child(add_popover);

        if profiles.is_empty() {
            return v_flex()
                .w_full()
                .gap_4()
                .child(header)
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .px_4()
                        .py_6()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border)
                        .child(
                            div()
                                .text_color(colors.foreground)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(shared(i18n::text(
                                    self.locale,
                                    "agent-empty-title",
                                ))),
                        )
                        .child(
                            div()
                                .text_size(crate::theme::scaled_text_size(12.))
                                .text_color(colors.muted_foreground)
                                .child(shared(i18n::text(
                                    self.locale,
                                    "agent-empty-description",
                                ))),
                        ),
                )
                .into_any_element();
        }

        let default_field = v_flex()
            .w_full()
            .gap_1()
            .child(Self::field(
                i18n::text(self.locale, "agent-current-profile-title"),
                Select::new(&self.agent_default_profile_state)
                    .w_full()
                    .into_any_element(),
                colors.foreground,
            ))
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(i18n::text(
                        self.locale,
                        "agent-current-profile-description",
                    ))),
            );

        let cards = profiles.iter().map(|option| {
            let profile_id = option.value.clone();
            let resolved = self.agent_settings.profile(&profile_id);
            let built_in =
                resolved.as_ref().and_then(|resolved| resolved.built_in);
            let executable_path_missing = resolved
                .as_ref()
                .is_some_and(|profile| {
                    explicit_executable_path_missing(&profile.executable)
                });
            let probe_result = self
                .agent_probe_results
                .iter()
                .find(|(id, _)| id == &profile_id)
                .and_then(|(_, result)| result.as_ref());
            let (status_text, status_color) = if resolved.is_none() {
                (
                    i18n::text(self.locale, "agent-profile-invalid"),
                    colors.red,
                )
            } else if executable_path_missing {
                (
                    i18n::text(self.locale, "agent-executable-not-found"),
                    colors.red,
                )
            } else {
                match probe_result {
                    Some(Ok(version)) => (version.clone(), colors.green),
                    Some(Err(error)) => (
                        i18n::text_args(
                            self.locale,
                            "agent-probe-unavailable",
                            &[("error", error)],
                        ),
                        colors.red,
                    ),
                    None => (
                        i18n::text(self.locale, "agent-probe-checking"),
                        colors.muted_foreground,
                    ),
                }
            };
            let executable_label = resolved
                .as_ref()
                .map(|profile| profile.executable.display().to_string())
                .unwrap_or_default();
            let launch_override_valid = resolved
                .as_ref()
                .and_then(|resolved| {
                    resolved.built_in.and_then(|agent| {
                        let saved_is_valid = self
                            .agent_settings
                            .launch_overrides
                            .get(&agent)
                            .map(|overrides| {
                                overrides.validate_for(agent).is_ok()
                            })
                            .unwrap_or(true);
                        Some(
                            saved_is_valid
                                && !self
                                    .agent_override_errors
                                    .contains_key(&agent),
                        )
                    })
                })
                .unwrap_or(true);
            let variant_capability_ready = resolved
                .as_ref()
                .and_then(|resolved| {
                    let has_variant = resolved.built_in
                        == Some(BuiltInAgent::OpenCode)
                        && self
                            .agent_settings
                            .launch_overrides_for(resolved)
                            .variant
                            .is_some();
                    has_variant.then(|| {
                        self.agent_variant_capability_ready(&resolved.id)
                    })
                })
                .unwrap_or(true);
            let can_test = resolved.is_some()
                && !executable_path_missing
                && launch_override_valid
                && variant_capability_ready;

            let expanded = self.agent_expanded.contains(&profile_id);
            let disclosure = this.clone();
            let disclosure_id = profile_id.clone();
            let disclosure_button = Button::new(SharedString::from(format!(
                "agent-card-toggle-{profile_id}"
            )))
            .label(shared(i18n::text(
                self.locale,
                "agent-launch-settings-toggle",
            )))
            .icon(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .ghost()
            .xsmall()
            .on_click(move |_event, _window, cx| {
                disclosure.update(cx, |panel, cx| {
                    panel.toggle_agent_expanded(disclosure_id.clone(), cx);
                });
            });

            let test = this.clone();
            let test_id = profile_id.clone();
            let mut actions = h_flex().items_center().gap_1().child(
                Button::new(SharedString::from(format!(
                    "agent-profile-test-{test_id}"
                )))
                .label(shared(i18n::text(
                    self.locale,
                    "agent-profile-test",
                )))
                .tooltip(shared(i18n::text(
                    self.locale,
                    "agent-test-description",
                )))
                .ghost()
                .xsmall()
                .disabled(!can_test)
                .on_click(move |_event, _window, cx| {
                    if can_test {
                        test.update(cx, |_panel, cx| {
                            cx.emit(
                                SettingsPanelEvent::AgentConnectivityTestRequested(
                                    test_id.clone(),
                                ),
                            );
                        });
                    }
                }),
            );
            if let Some(agent) = built_in {
                let remove = this.clone();
                actions = actions.child(
                    Button::new(SharedString::from(format!(
                        "agent-card-remove-{profile_id}"
                    )))
                    .label(shared(i18n::text(self.locale, "agent-remove")))
                    .ghost()
                    .xsmall()
                    .on_click(move |_event, _window, cx| {
                        remove.update(cx, |_panel, cx| {
                            cx.emit(
                                SettingsPanelEvent::AgentBuiltinRemoveRequested(agent),
                            );
                        });
                    }),
                );
            } else if self
                .agent_settings
                .custom_profiles
                .iter()
                .any(|custom| custom.id == profile_id)
            {
                let edit = this.clone();
                let remove = this.clone();
                let edit_id = profile_id.clone();
                let remove_id = profile_id.clone();
                actions = actions
                    .child(
                        Button::new(SharedString::from(format!(
                            "agent-profile-edit-{edit_id}"
                        )))
                        .label(shared(i18n::text(
                            self.locale,
                            "agent-profile-edit",
                        )))
                        .ghost()
                        .xsmall()
                        .on_click(move |_event, window, cx| {
                            edit.update(cx, |panel, cx| {
                                panel.open_agent_profile_editor(
                                    Some(edit_id.clone()),
                                    window,
                                    cx,
                                );
                            });
                        }),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "agent-profile-remove-{remove_id}"
                        )))
                        .label(shared(i18n::text(
                            self.locale,
                            "agent-profile-remove",
                        )))
                        .ghost()
                        .xsmall()
                        .on_click(move |_event, _window, cx| {
                            remove.update(cx, |panel, cx| {
                                panel.remove_agent_profile(
                                    remove_id.clone(),
                                    cx,
                                );
                            });
                        }),
                    );
            }

            let mut card = v_flex()
                .w_full()
                .gap_1()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .bg(colors.secondary)
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_color(colors.foreground)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(option.label.clone()),
                        )
                        .when(profile_id == default_id, |row| {
                            row.child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .text_size(crate::theme::scaled_text_size(
                                        11.,
                                    ))
                                    .text_color(accent_foreground)
                                    .bg(accent)
                                    .child(shared(i18n::text(
                                        self.locale,
                                        "agent-default-badge",
                                    ))),
                            )
                        })
                        .when(!executable_label.is_empty(), |row| {
                            row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .text_color(colors.muted_foreground)
                                    .text_size(crate::theme::scaled_text_size(
                                        11.,
                                    ))
                                    .child(SharedString::from(
                                        executable_label.clone(),
                                    )),
                            )
                        }),
                )
                .child(
                    div()
                        .text_color(status_color)
                        .text_size(crate::theme::scaled_text_size(11.))
                        .child(SharedString::from(status_text)),
                )
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(disclosure_button)
                        .child(actions),
                );
            if expanded {
                card =
                    card.child(self.render_agent_card_body(
                        &profile_id,
                        built_in,
                        cx,
                    ));
            }
            card
        });

        v_flex()
            .w_full()
            .gap_4()
            .child(header)
            .child(default_field)
            .children(cards)
            .into_any_element()
    }

    fn render_agent_card_body(
        &self,
        profile_id: &str,
        built_in: Option<BuiltInAgent>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let Some(agent) = built_in else {
            // Custom profiles are edited through the shared profile editor;
            // the card body only summarizes the launch configuration.
            let summary = self
                .agent_settings
                .custom_profiles
                .iter()
                .find(|profile| profile.id == profile_id)
                .map(|profile| {
                    let args = if profile.args.is_empty() {
                        i18n::text(self.locale, "agent-card-no-args")
                    } else {
                        profile.args.join(" ")
                    };
                    let prompt_mode = match &profile.prompt_mode {
                        crate::agent::PromptMode::TrailingArgument => {
                            i18n::text(
                                self.locale,
                                "agent-profile-prompt-trailing",
                            )
                        }
                        crate::agent::PromptMode::Flag(flag) => flag.clone(),
                    };
                    format!(
                        "{} · {args} · {prompt_mode}",
                        profile.executable.display()
                    )
                })
                .unwrap_or_default();
            return div()
                .w_full()
                .text_color(colors.muted_foreground)
                .text_size(crate::theme::scaled_text_size(11.))
                .child(SharedString::from(summary))
                .into_any_element();
        };
        let Some(executable_input) = self
            .agent_executable_inputs
            .iter()
            .find(|(entry, _)| *entry == agent)
            .map(|(_, input)| input.clone())
        else {
            return div().w_full().into_any_element();
        };
        let Some(model_input) = self
            .agent_model_inputs
            .iter()
            .find(|(entry, _)| *entry == agent)
            .map(|(_, input)| input.clone())
        else {
            return div().w_full().into_any_element();
        };
        let body = v_flex()
            .w_full()
            .gap_2()
            .pt_2()
            .child(Self::field(
                i18n::text(self.locale, "agent-profile-executable"),
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&executable_input).w_full()),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "agent-executable-browse-{agent_id}",
                            agent_id = agent.id()
                        )))
                        .label(shared(i18n::text(
                            self.locale,
                            "agent-executable-browse",
                        )))
                        .tooltip(shared(i18n::text(
                            self.locale,
                            "agent-executable-description",
                        )))
                        .ghost()
                        .small()
                        .on_click(
                            move |_event, window, cx| {
                                this.update(cx, |panel, cx| {
                                    panel.browse_agent_executable(
                                        agent, window, cx,
                                    );
                                });
                            },
                        ),
                    )
                    .into_any_element(),
                colors.foreground,
            ))
            .child(Self::field(
                i18n::text(self.locale, "agent-model-title"),
                Input::new(&model_input).w_full().into_any_element(),
                colors.foreground,
            ));
        let reasoning_control: AnyElement = if agent == BuiltInAgent::OpenCode {
            let Some(variant_input) = self
                .agent_variant_inputs
                .iter()
                .find(|(entry, _)| *entry == agent)
                .map(|(_, input)| input.clone())
            else {
                return body.into_any_element();
            };
            v_flex()
                .w_full()
                .gap_1()
                .child(Self::field(
                    i18n::text(self.locale, "agent-variant-title"),
                    Input::new(&variant_input).w_full().into_any_element(),
                    colors.foreground,
                ))
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "agent-opencode-variant-note",
                        ))),
                )
                .into_any_element()
        } else {
            let Some(reasoning_state) = self
                .agent_reasoning_states
                .iter()
                .find(|(entry, _)| *entry == agent)
                .map(|(_, state)| state.clone())
            else {
                return body.into_any_element();
            };
            Self::field(
                i18n::text(self.locale, "agent-reasoning-title"),
                Select::new(&reasoning_state).w_full().into_any_element(),
                colors.foreground,
            )
            .into_any_element()
        };
        let launch_error =
            self.agent_override_errors
                .get(&agent)
                .cloned()
                .or_else(|| {
                    self.agent_settings.launch_overrides.get(&agent).and_then(
                        |overrides| overrides.validate_for(agent).err(),
                    )
                })
                .map(|error| {
                    div()
                        .text_size(crate::theme::scaled_text_size(11.))
                        .text_color(colors.red)
                        .child(shared(i18n::text_args(
                            self.locale,
                            "agent-launch-invalid",
                            &[("error", &error)],
                        )))
                });
        let inherit_note = div()
            .text_size(crate::theme::scaled_text_size(11.))
            .text_color(colors.muted_foreground)
            .child(shared(i18n::text(
                self.locale,
                "agent-launch-settings-description",
            )));
        body.child(reasoning_control)
            .child(inherit_note)
            .when_some(launch_error, |element, error| element.child(error))
            .into_any_element()
    }
}

/// A configured explicit path that no longer resolves means the agent is
/// unusable regardless of probe results.
fn explicit_executable_path_missing(path: &std::path::Path) -> bool {
    let has_directory_component =
        path.is_absolute() || path.components().count() > 1;
    has_directory_component && crate::agent::resolve_executable(path).is_err()
}
