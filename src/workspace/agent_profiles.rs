//! Editor for user-defined external Agent profiles.

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};

use crate::agent::{CustomAgentProfile, PromptMode};
use crate::core::i18n::{self, Locale};

#[derive(Clone, Debug)]
pub enum AgentProfileEditorEvent {
    Cancel,
    Save {
        previous_id: Option<String>,
        profile: CustomAgentProfile,
    },
}

/// Modal form for creating or updating one custom Agent profile.
pub struct AgentProfileEditor {
    locale: Locale,
    previous_id: Option<String>,
    id: Entity<InputState>,
    name: Entity<InputState>,
    executable: Entity<InputState>,
    args: Entity<TextareaState>,
    prompt_flag: Entity<InputState>,
    flag_mode: bool,
    error: Option<String>,
}

impl EventEmitter<AgentProfileEditorEvent> for AgentProfileEditor {}

impl AgentProfileEditor {
    pub fn new(
        profile: Option<CustomAgentProfile>,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let previous_id = profile.as_ref().map(|profile| profile.id.clone());
        let id_value = profile
            .as_ref()
            .map(|profile| profile.id.clone())
            .unwrap_or_else(|| "custom-agent".to_string());
        let name_value = profile
            .as_ref()
            .map(|profile| profile.name.clone())
            .unwrap_or_default();
        let executable_value = profile
            .as_ref()
            .map(|profile| profile.executable.display().to_string())
            .unwrap_or_default();
        let args_value = profile
            .as_ref()
            .map(|profile| profile.args.join("\n"))
            .unwrap_or_default();
        let (flag_mode, flag_value) =
            match profile.as_ref().map(|profile| &profile.prompt_mode) {
                Some(PromptMode::Flag(flag)) => (true, flag.clone()),
                _ => (false, String::new()),
            };

        let id =
            cx.new(|cx| InputState::new(window, cx).default_value(id_value));
        let name =
            cx.new(|cx| InputState::new(window, cx).default_value(name_value));
        let executable = cx.new(|cx| {
            InputState::new(window, cx).default_value(executable_value)
        });
        let args = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 8)
                .default_value(args_value)
        });
        let prompt_flag = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(flag_value)
                .placeholder("--prompt")
        });

        Self {
            locale,
            previous_id,
            id,
            name,
            executable,
            args,
            prompt_flag,
            flag_mode,
            error: None,
        }
    }

    fn toggle_prompt_mode(&mut self, cx: &mut Context<Self>) {
        self.flag_mode = !self.flag_mode;
        self.error = None;
        cx.notify();
    }

    pub(crate) fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let profile = CustomAgentProfile {
            id: self.id.read(cx).value().trim().to_string(),
            name: self.name.read(cx).value().trim().to_string(),
            executable: std::path::PathBuf::from(
                self.executable.read(cx).value().trim(),
            ),
            args: self
                .args
                .read(cx)
                .value()
                .lines()
                .map(str::trim_end)
                .filter(|arg| !arg.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            prompt_mode: if self.flag_mode {
                PromptMode::Flag(
                    self.prompt_flag.read(cx).value().trim().to_string(),
                )
            } else {
                PromptMode::TrailingArgument
            },
        };
        if let Err(error) = profile.validate() {
            self.error = Some(i18n::text_args(
                self.locale,
                "agent-profile-validation-error",
                &[("error", &error)],
            ));
            cx.notify();
            return;
        }
        cx.emit(AgentProfileEditorEvent::Save {
            previous_id: self.previous_id.clone(),
            profile,
        });
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(AgentProfileEditorEvent::Cancel);
    }

    fn field(
        &self,
        label: String,
        input: impl IntoElement,
        colors: &gpui_component::theme::ThemeColor,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(SharedString::from(label)),
            )
            .child(input)
    }
}

impl Render for AgentProfileEditor {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let cancel = this.clone();
        let save = this.clone();
        let toggle = this.clone();
        let title_key = if self.previous_id.is_some() {
            "agent-profile-edit-title"
        } else {
            "agent-profile-new-title"
        };
        let mode_label = if self.flag_mode {
            i18n::text(self.locale, "agent-profile-prompt-flag")
        } else {
            i18n::text(self.locale, "agent-profile-prompt-trailing")
        };

        v_flex()
            .id("agent-profile-editor-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(colors.background.opacity(0.92))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .id("agent-profile-editor-card")
                    .w(px(560.))
                    .max_w(relative(0.9))
                    .gap_3()
                    .p_5()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .when(cx.theme().shadow, |element| element.shadow_lg())
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::new(IconName::Bot).size(px(16.)),
                                    )
                                    .child(
                                        div()
                                            .text_color(colors.foreground)
                                            .font_weight(FontWeight::BOLD)
                                            .child(SharedString::from(
                                                i18n::text(
                                                    self.locale,
                                                    title_key,
                                                ),
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("agent-profile-editor-close")
                                    .icon(IconName::Close)
                                    .ghost()
                                    .small()
                                    .on_click(move |_event, _window, cx| {
                                        cancel.update(cx, |editor, cx| {
                                            editor.cancel(cx)
                                        });
                                    }),
                            ),
                    )
                    .child(self.field(
                        i18n::text(self.locale, "agent-profile-id"),
                        Input::new(&self.id).w_full(),
                        &colors,
                    ))
                    .child(self.field(
                        i18n::text(self.locale, "agent-profile-name"),
                        Input::new(&self.name).w_full(),
                        &colors,
                    ))
                    .child(self.field(
                        i18n::text(self.locale, "agent-profile-executable"),
                        Input::new(&self.executable).w_full(),
                        &colors,
                    ))
                    .child(self.field(
                        i18n::text(self.locale, "agent-profile-args"),
                        Textarea::new(&self.args).w_full(),
                        &colors,
                    ))
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(11.))
                            .text_color(colors.muted_foreground)
                            .child(SharedString::from(i18n::text(
                                self.locale,
                                "agent-profile-args-hint",
                            ))),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(crate::theme::scaled_text_size(
                                        12.,
                                    ))
                                    .text_color(colors.muted_foreground)
                                    .child(SharedString::from(i18n::text(
                                        self.locale,
                                        "agent-profile-prompt-mode",
                                    ))),
                            )
                            .child(
                                Button::new("agent-profile-prompt-mode-toggle")
                                    .label(mode_label)
                                    .ghost()
                                    .small()
                                    .on_click(move |_event, _window, cx| {
                                        toggle.update(cx, |editor, cx| {
                                            editor.toggle_prompt_mode(cx)
                                        });
                                    }),
                            ),
                    )
                    .when(self.flag_mode, |element| {
                        element.child(self.field(
                            i18n::text(self.locale, "agent-profile-flag"),
                            Input::new(&self.prompt_flag).w_full(),
                            &colors,
                        ))
                    })
                    .when_some(self.error.clone(), |element, error| {
                        element.child(
                            div()
                                .text_size(crate::theme::scaled_text_size(12.))
                                .text_color(colors.red)
                                .child(SharedString::from(error)),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("agent-profile-editor-cancel")
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-profile-cancel",
                                    ))
                                    .ghost()
                                    .on_click(move |_event, _window, cx| {
                                        save.update(cx, |editor, cx| {
                                            editor.cancel(cx)
                                        });
                                    }),
                            )
                            .child(
                                Button::new("agent-profile-editor-save")
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-profile-save",
                                    ))
                                    .primary()
                                    .on_click(move |_event, _window, cx| {
                                        this.update(cx, |editor, cx| {
                                            editor.save(cx)
                                        });
                                    }),
                            ),
                    ),
            )
    }
}
