use gpui::prelude::*;
use std::collections::HashMap;

use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, IconName, IndexPath, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    searchable_list::{SearchableListItem, SearchableVec},
    select::{Select, SelectEvent, SelectState},
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};

use crate::agent::{
    AgentCliCapabilities, AgentLaunchOverrides, AgentSettings, BuiltInAgent,
    CustomAgentProfile,
};
use crate::core::config::{
    AppConfig, DiffLayoutPreference, GraphHistoryPreference,
    LanguagePreference, MAX_DIFF_FONT_SIZE, MAX_UI_FONT_SIZE,
    MIN_DIFF_FONT_SIZE, MIN_UI_FONT_SIZE, ThemePreference,
};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

use super::agent_profiles::{AgentProfileEditor, AgentProfileEditorEvent};

#[derive(Clone, Debug)]
pub enum SettingsPanelEvent {
    Close,
    LanguageChanged(LanguagePreference),
    AutoRefreshOnFocusChanged(bool),
    ThemeChanged(ThemePreference),
    DiffLayoutChanged(DiffLayoutPreference),
    GraphHistoryChanged(GraphHistoryPreference),
    UiFontChanged(Option<String>),
    MonoFontChanged(Option<String>),
    UiFontSizeChanged(f32),
    DiffFontSizeChanged(f32),
    AgentDefaultProfileChanged(String),
    AgentExecutableOverrideChanged {
        agent: BuiltInAgent,
        executable: Option<std::path::PathBuf>,
    },
    AgentModelOverrideChanged {
        agent: BuiltInAgent,
        model: Option<String>,
    },
    AgentReasoningOverrideChanged {
        agent: BuiltInAgent,
        reasoning_effort: Option<String>,
    },
    AgentVariantOverrideChanged {
        agent: BuiltInAgent,
        variant: Option<String>,
    },
    AgentConnectivityTestRequested(String),
    AgentProfileSaved {
        previous_id: Option<String>,
        profile: CustomAgentProfile,
    },
    AgentProfileRemoved(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSection {
    General,
    Appearance,
    Layout,
    Agents,
}

#[derive(Clone, Debug)]
struct SettingsOption<T: Clone + PartialEq> {
    value: T,
    label: SharedString,
}

impl<T: Clone + PartialEq> SettingsOption<T> {
    fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}

impl<T: Clone + PartialEq> SearchableListItem for SettingsOption<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

pub struct SettingsPanel {
    locale: Locale,
    section: SettingsSection,
    language: LanguagePreference,
    auto_refresh_on_focus: bool,
    theme: ThemePreference,
    diff_layout: DiffLayoutPreference,
    graph_history: GraphHistoryPreference,
    ui_font: Option<String>,
    mono_font: Option<String>,
    ui_font_size: f32,
    diff_font_size: f32,
    agent_settings: AgentSettings,
    agent_override_errors: HashMap<BuiltInAgent, String>,
    agent_probe_results: Vec<(String, Option<Result<String, String>>)>,
    agent_probe_capabilities: HashMap<String, AgentCliCapabilities>,
    agent_probe_generation: u64,
    font_families: Vec<String>,
    language_state:
        Entity<SelectState<Vec<SettingsOption<LanguagePreference>>>>,
    auto_refresh_state: Entity<SelectState<Vec<SettingsOption<bool>>>>,
    theme_state: Entity<SelectState<Vec<SettingsOption<ThemePreference>>>>,
    diff_layout_state:
        Entity<SelectState<Vec<SettingsOption<DiffLayoutPreference>>>>,
    graph_history_state:
        Entity<SelectState<Vec<SettingsOption<GraphHistoryPreference>>>>,
    ui_font_state:
        Entity<SelectState<SearchableVec<SettingsOption<Option<String>>>>>,
    mono_font_state:
        Entity<SelectState<SearchableVec<SettingsOption<Option<String>>>>>,
    ui_font_size_state: Entity<SliderState>,
    diff_font_size_state: Entity<SliderState>,
    agent_default_profile_state:
        Entity<SelectState<Vec<SettingsOption<String>>>>,
    agent_executable_inputs: Vec<(BuiltInAgent, Entity<InputState>)>,
    agent_model_inputs: Vec<(BuiltInAgent, Entity<InputState>)>,
    agent_variant_inputs: Vec<(BuiltInAgent, Entity<InputState>)>,
    agent_reasoning_states: Vec<(
        BuiltInAgent,
        Entity<SelectState<Vec<SettingsOption<Option<String>>>>>,
    )>,
    agent_profile_editor: Option<Entity<AgentProfileEditor>>,
}

impl EventEmitter<SettingsPanelEvent> for SettingsPanel {}

impl SettingsPanel {
    pub fn new(
        config: &AppConfig,
        font_families: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let locale = i18n::resolve(&config.language);
        let language = config.language;
        let auto_refresh_on_focus = config.view.auto_refresh_on_focus;
        let theme = config.theme;
        let diff_layout = config.view.diff_layout;
        let graph_history = config.view.graph_history;
        let ui_font = config.typography.ui_font_family.clone();
        let mono_font = config.typography.mono_font_family.clone();
        let ui_font_size = config.typography.ui_font_size;
        let diff_font_size = config.typography.diff_font_size;
        let agent_settings = config.agent.clone();
        let agent_default_profile = agent_settings.default_profile_id();

        let language_state = cx.new(|cx| {
            SelectState::new(
                language_options(locale),
                selected_index(&language_options(locale), &language),
                window,
                cx,
            )
        });
        let auto_refresh_state = cx.new(|cx| {
            SelectState::new(
                auto_refresh_options(locale),
                selected_index(
                    &auto_refresh_options(locale),
                    &auto_refresh_on_focus,
                ),
                window,
                cx,
            )
        });
        let theme_state = cx.new(|cx| {
            SelectState::new(
                theme_options(locale),
                selected_index(&theme_options(locale), &theme),
                window,
                cx,
            )
        });
        let diff_layout_state = cx.new(|cx| {
            SelectState::new(
                diff_layout_options(locale),
                selected_index(&diff_layout_options(locale), &diff_layout),
                window,
                cx,
            )
        });
        let graph_history_state = cx.new(|cx| {
            SelectState::new(
                graph_history_options(locale),
                selected_index(&graph_history_options(locale), &graph_history),
                window,
                cx,
            )
        });
        let ui_font_state = cx.new(|cx| {
            let options = font_options(locale, &font_families);
            SelectState::new(
                SearchableVec::from(options.clone()),
                selected_index(&options, &ui_font),
                window,
                cx,
            )
            .searchable(true)
        });
        let mono_font_state = cx.new(|cx| {
            let options = font_options(locale, &font_families);
            SelectState::new(
                SearchableVec::from(options.clone()),
                selected_index(&options, &mono_font),
                window,
                cx,
            )
            .searchable(true)
        });
        let ui_font_size_state = cx.new(|_| {
            SliderState::new()
                .min(MIN_UI_FONT_SIZE)
                .max(MAX_UI_FONT_SIZE)
                .step(1.0)
                .default_value(ui_font_size)
        });
        let diff_font_size_state = cx.new(|_| {
            SliderState::new()
                .min(MIN_DIFF_FONT_SIZE)
                .max(MAX_DIFF_FONT_SIZE)
                .step(1.0)
                .default_value(diff_font_size)
        });
        let agent_default_profile_state = cx.new(|cx| {
            let options = agent_profile_options(locale, &agent_settings);
            SelectState::new(
                options.clone(),
                selected_index(&options, &agent_default_profile),
                window,
                cx,
            )
        });
        let agent_executable_inputs = BuiltInAgent::ALL
            .iter()
            .copied()
            .map(|agent| {
                let value = agent_settings
                    .executable_overrides
                    .get(&agent)
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(agent.executable())
                });
                (agent, input)
            })
            .collect::<Vec<_>>();
        let agent_model_inputs = BuiltInAgent::ALL
            .iter()
            .copied()
            .map(|agent| {
                let value = agent_settings
                    .launch_overrides
                    .get(&agent)
                    .and_then(|overrides| overrides.model.clone())
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(i18n::text(
                            locale,
                            "agent-model-placeholder",
                        ))
                });
                (agent, input)
            })
            .collect::<Vec<_>>();
        let agent_variant_inputs = BuiltInAgent::ALL
            .iter()
            .copied()
            .map(|agent| {
                let value = agent_settings
                    .launch_overrides
                    .get(&agent)
                    .and_then(|overrides| overrides.variant.clone())
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(i18n::text(
                            locale,
                            "agent-variant-placeholder",
                        ))
                });
                (agent, input)
            })
            .collect::<Vec<_>>();
        let agent_reasoning_states = BuiltInAgent::ALL
            .iter()
            .copied()
            .map(|agent| {
                let options = agent_reasoning_options(locale, agent);
                let value = agent_settings
                    .launch_overrides
                    .get(&agent)
                    .and_then(|overrides| overrides.reasoning_effort.clone());
                let state = cx.new(|cx| {
                    SelectState::new(
                        options.clone(),
                        selected_index(&options, &value),
                        window,
                        cx,
                    )
                });
                (agent, state)
            })
            .collect::<Vec<_>>();

        let mut panel = Self {
            locale,
            section: SettingsSection::General,
            language,
            auto_refresh_on_focus,
            theme,
            diff_layout,
            graph_history,
            ui_font,
            mono_font,
            ui_font_size,
            diff_font_size,
            agent_settings,
            agent_override_errors: HashMap::new(),
            agent_probe_results: Vec::new(),
            agent_probe_capabilities: HashMap::new(),
            agent_probe_generation: 0,
            font_families,
            language_state,
            auto_refresh_state,
            theme_state,
            diff_layout_state,
            graph_history_state,
            ui_font_state,
            mono_font_state,
            ui_font_size_state,
            diff_font_size_state,
            agent_default_profile_state,
            agent_executable_inputs,
            agent_model_inputs,
            agent_variant_inputs,
            agent_reasoning_states,
            agent_profile_editor: None,
        };

        let language_state_for_events = panel.language_state.clone();
        cx.subscribe(&language_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.language = *value;
            cx.emit(SettingsPanelEvent::LanguageChanged(*value));
        })
        .detach();

        let auto_refresh_state_for_events = panel.auto_refresh_state.clone();
        cx.subscribe(&auto_refresh_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.auto_refresh_on_focus = *value;
            cx.emit(SettingsPanelEvent::AutoRefreshOnFocusChanged(*value));
        })
        .detach();

        let theme_state_for_events = panel.theme_state.clone();
        cx.subscribe(&theme_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.theme = *value;
            cx.emit(SettingsPanelEvent::ThemeChanged(*value));
        })
        .detach();

        let diff_layout_state_for_events = panel.diff_layout_state.clone();
        cx.subscribe(&diff_layout_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.diff_layout = *value;
            cx.emit(SettingsPanelEvent::DiffLayoutChanged(*value));
        })
        .detach();

        let graph_history_state_for_events = panel.graph_history_state.clone();
        cx.subscribe(&graph_history_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.graph_history = *value;
            cx.emit(SettingsPanelEvent::GraphHistoryChanged(*value));
        })
        .detach();

        let ui_font_state_for_events = panel.ui_font_state.clone();
        cx.subscribe(&ui_font_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.ui_font = value.clone();
            cx.emit(SettingsPanelEvent::UiFontChanged(value.clone()));
        })
        .detach();

        let mono_font_state_for_events = panel.mono_font_state.clone();
        cx.subscribe(&mono_font_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.mono_font = value.clone();
            cx.emit(SettingsPanelEvent::MonoFontChanged(value.clone()));
        })
        .detach();

        let ui_font_size_state_for_events = panel.ui_font_size_state.clone();
        cx.subscribe(
            &ui_font_size_state_for_events,
            |panel, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(value) = event else {
                    return;
                };
                let size = value.start();
                if (panel.ui_font_size - size).abs() <= f32::EPSILON {
                    return;
                }
                panel.ui_font_size = size;
                cx.emit(SettingsPanelEvent::UiFontSizeChanged(size));
                cx.notify();
            },
        )
        .detach();

        let diff_font_size_state_for_events =
            panel.diff_font_size_state.clone();
        cx.subscribe(
            &diff_font_size_state_for_events,
            |panel, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(value) = event else {
                    return;
                };
                let size = value.start();
                if (panel.diff_font_size - size).abs() <= f32::EPSILON {
                    return;
                }
                panel.diff_font_size = size;
                cx.emit(SettingsPanelEvent::DiffFontSizeChanged(size));
                cx.notify();
            },
        )
        .detach();

        let agent_default_profile_state_for_events =
            panel.agent_default_profile_state.clone();
        cx.subscribe(
            &agent_default_profile_state_for_events,
            |panel, _, event, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                if panel.agent_settings.default_profile_id.as_deref()
                    == Some(value.as_str())
                {
                    return;
                }
                panel.agent_settings.default_profile_id = Some(value.clone());
                cx.emit(SettingsPanelEvent::AgentDefaultProfileChanged(
                    value.clone(),
                ));
            },
        )
        .detach();

        for (agent, input) in &panel.agent_executable_inputs {
            let agent = *agent;
            let input = input.clone();
            cx.subscribe(&input, move |_panel, state, event, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = state.read(cx).value().trim().to_string();
                let executable = (!value.is_empty())
                    .then(|| std::path::PathBuf::from(value));
                cx.emit(SettingsPanelEvent::AgentExecutableOverrideChanged {
                    agent,
                    executable,
                });
            })
            .detach();
        }

        for (agent, input) in &panel.agent_model_inputs {
            let agent = *agent;
            let input = input.clone();
            cx.subscribe(&input, move |panel, state, event, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = state.read(cx).value().trim().to_string();
                let model = (!value.is_empty()).then_some(value);
                let mut overrides = panel
                    .agent_settings
                    .launch_overrides
                    .get(&agent)
                    .cloned()
                    .unwrap_or_else(AgentLaunchOverrides::default);
                overrides.model = model.clone();
                if let Err(error) = overrides.validate_for(agent) {
                    panel.agent_override_errors.insert(agent, error);
                    cx.notify();
                    return;
                }
                panel.agent_override_errors.remove(&agent);
                cx.emit(SettingsPanelEvent::AgentModelOverrideChanged {
                    agent,
                    model,
                });
            })
            .detach();
        }

        for (agent, input) in &panel.agent_variant_inputs {
            let agent = *agent;
            let input = input.clone();
            cx.subscribe(&input, move |panel, state, event, cx| {
                if agent != BuiltInAgent::OpenCode
                    || !matches!(event, InputEvent::Change)
                {
                    return;
                }
                let value = state.read(cx).value().trim().to_string();
                let variant = (!value.is_empty()).then_some(value);
                let mut overrides = panel
                    .agent_settings
                    .launch_overrides
                    .get(&agent)
                    .cloned()
                    .unwrap_or_else(AgentLaunchOverrides::default);
                overrides.variant = variant.clone();
                if let Err(error) = overrides.validate_for(agent) {
                    panel.agent_override_errors.insert(agent, error);
                    cx.notify();
                    return;
                }
                panel.agent_override_errors.remove(&agent);
                cx.emit(SettingsPanelEvent::AgentVariantOverrideChanged {
                    agent,
                    variant,
                });
            })
            .detach();
        }

        for (agent, state) in &panel.agent_reasoning_states {
            let agent = *agent;
            let state = state.clone();
            cx.subscribe(&state, move |panel, _, event, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                panel.agent_override_errors.remove(&agent);
                cx.emit(SettingsPanelEvent::AgentReasoningOverrideChanged {
                    agent,
                    reasoning_effort: value.clone(),
                });
            })
            .detach();
        }

        panel.start_agent_probes(cx);
        panel
    }

    fn open_agent_profile_editor(
        &mut self,
        profile_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_profile_editor.is_some() {
            return;
        }
        let profile = profile_id.as_deref().and_then(|id| {
            self.agent_settings
                .custom_profiles
                .iter()
                .find(|profile| profile.id == id)
                .cloned()
        });
        let editor = cx.new(|cx| {
            AgentProfileEditor::new(profile, self.locale, window, cx)
        });
        cx.subscribe_in(
            &editor,
            window,
            |panel, _editor, event, _window, cx| match event {
                AgentProfileEditorEvent::Cancel => {
                    panel.agent_profile_editor = None;
                    cx.notify();
                }
                AgentProfileEditorEvent::Save {
                    previous_id,
                    profile,
                } => {
                    let mut candidate = panel.agent_settings.clone();
                    if let Some(previous_id) = previous_id {
                        if candidate.default_profile_id.as_deref()
                            == Some(previous_id)
                        {
                            candidate.default_profile_id =
                                Some(profile.id.clone());
                        }
                        candidate
                            .custom_profiles
                            .retain(|entry| entry.id != *previous_id);
                    }
                    candidate.custom_profiles.push(profile.clone());
                    if let Err(errors) = candidate.validate() {
                        if let Some(editor) = panel.agent_profile_editor.clone()
                        {
                            editor.update(cx, |editor, cx| {
                                let error =
                                    errors.into_iter().next().unwrap_or_else(
                                        || "invalid Agent profile".to_string(),
                                    );
                                editor.set_error(
                                    i18n::text_args(
                                        panel.locale,
                                        "agent-profile-validation-error",
                                        &[("error", &error)],
                                    ),
                                    cx,
                                );
                            });
                        }
                        return;
                    }
                    panel.agent_profile_editor = None;
                    cx.emit(SettingsPanelEvent::AgentProfileSaved {
                        previous_id: previous_id.clone(),
                        profile: profile.clone(),
                    });
                    cx.notify();
                }
            },
        )
        .detach();
        self.agent_profile_editor = Some(editor);
        cx.notify();
    }

    fn remove_agent_profile(
        &mut self,
        profile_id: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .agent_settings
            .custom_profiles
            .iter()
            .all(|profile| profile.id != profile_id)
        {
            return;
        }
        self.agent_profile_editor = None;
        cx.emit(SettingsPanelEvent::AgentProfileRemoved(profile_id));
    }

    pub fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        self.agent_profile_editor = None;
        let language = self.language;
        let auto_refresh_on_focus = self.auto_refresh_on_focus;
        let theme = self.theme;
        let diff_layout = self.diff_layout;
        let graph_history = self.graph_history;
        let ui_font = self.ui_font.clone();
        let mono_font = self.mono_font.clone();
        let fonts = self.font_families.clone();
        let agent_settings = self.agent_settings.clone();
        let agent_default_profile = agent_settings.default_profile_id();

        self.language_state.update(cx, |state, cx| {
            let options = language_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&language, window, cx);
        });
        self.auto_refresh_state.update(cx, |state, cx| {
            let options = auto_refresh_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&auto_refresh_on_focus, window, cx);
        });
        self.theme_state.update(cx, |state, cx| {
            let options = theme_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&theme, window, cx);
        });
        self.diff_layout_state.update(cx, |state, cx| {
            let options = diff_layout_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&diff_layout, window, cx);
        });
        self.graph_history_state.update(cx, |state, cx| {
            let options = graph_history_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&graph_history, window, cx);
        });
        self.ui_font_state.update(cx, |state, cx| {
            let options = font_options(locale, &fonts);
            state.set_items(SearchableVec::from(options), window, cx);
            state.set_selected_value(&ui_font, window, cx);
        });
        self.mono_font_state.update(cx, |state, cx| {
            let options = font_options(locale, &fonts);
            state.set_items(SearchableVec::from(options), window, cx);
            state.set_selected_value(&mono_font, window, cx);
        });
        self.agent_default_profile_state.update(cx, |state, cx| {
            let options = agent_profile_options(locale, &agent_settings);
            state.set_items(options.clone(), window, cx);
            state.set_selected_value(&agent_default_profile, window, cx);
        });
        for (agent, state) in &self.agent_reasoning_states {
            let options = agent_reasoning_options(locale, *agent);
            let value = agent_settings
                .launch_overrides
                .get(agent)
                .and_then(|overrides| overrides.reasoning_effort.clone());
            state.update(cx, |state, cx| {
                state.set_items(options.clone(), window, cx);
                state.set_selected_value(&value, window, cx);
            });
        }
        cx.notify();
    }

    pub fn set_agent_settings(
        &mut self,
        settings: AgentSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_settings = settings.clone();
        self.agent_override_errors.clear();
        self.agent_profile_editor = None;
        self.start_agent_probes(cx);
        let selected = settings.default_profile_id();
        let options = agent_profile_options(self.locale, &settings);
        self.agent_default_profile_state.update(cx, |state, cx| {
            state.set_items(options.clone(), window, cx);
            state.set_selected_value(&selected, window, cx);
        });
        for (agent, input) in &self.agent_executable_inputs {
            let value = settings
                .executable_overrides
                .get(agent)
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            input.update(cx, |state, cx| {
                state.set_value(value, window, cx);
            });
        }
        for (agent, input) in &self.agent_model_inputs {
            let value = settings
                .launch_overrides
                .get(agent)
                .and_then(|overrides| overrides.model.clone())
                .unwrap_or_default();
            input.update(cx, |state, cx| {
                state.set_value(value, window, cx);
            });
        }
        for (agent, input) in &self.agent_variant_inputs {
            let value = settings
                .launch_overrides
                .get(agent)
                .and_then(|overrides| overrides.variant.clone())
                .unwrap_or_default();
            input.update(cx, |state, cx| {
                state.set_value(value, window, cx);
            });
        }
        for (agent, state) in &self.agent_reasoning_states {
            let options = agent_reasoning_options(self.locale, *agent);
            let value = settings
                .launch_overrides
                .get(agent)
                .and_then(|overrides| overrides.reasoning_effort.clone());
            state.update(cx, |state, cx| {
                state.set_items(options.clone(), window, cx);
                state.set_selected_value(&value, window, cx);
            });
        }
        cx.notify();
    }

    fn start_agent_probes(&mut self, cx: &mut Context<Self>) {
        self.agent_probe_generation =
            self.agent_probe_generation.wrapping_add(1);
        let generation = self.agent_probe_generation;
        let profiles = agent_profile_options(self.locale, &self.agent_settings)
            .into_iter()
            .filter_map(|option| {
                self.agent_settings
                    .profile(&option.value)
                    .map(|profile| (option.value, profile))
            })
            .collect::<Vec<_>>();
        self.agent_probe_results =
            profiles.iter().map(|(id, _)| (id.clone(), None)).collect();
        self.agent_probe_capabilities.clear();
        let panel = cx.entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let current_generation =
                panel.read_with(cx, |panel, _| panel.agent_probe_generation);
            if current_generation != generation {
                return;
            }
            let results = cx
                .background_spawn(async move {
                    profiles
                        .into_iter()
                        .map(|(id, profile)| {
                            let result = crate::agent::probe_profile(&profile)
                                .map(|version| first_line(&version).to_string())
                                .map_err(|error| {
                                    first_line(&error.to_string()).to_string()
                                });
                            let capabilities = if profile.built_in
                                == Some(BuiltInAgent::OpenCode)
                            {
                                crate::agent::probe_profile_capabilities(
                                    &profile,
                                )
                                .ok()
                            } else {
                                None
                            };
                            (id, result, capabilities)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                if panel.agent_probe_generation != generation {
                    return;
                }
                for (id, _, capabilities) in &results {
                    if let Some(capabilities) = capabilities {
                        log::info!(
                            "[agent_terminal] capability probe: profile={id}, interactive_variant={}",
                            capabilities.supports_interactive_variant
                        );
                    }
                }
                panel.agent_probe_capabilities = results
                    .iter()
                    .filter_map(|(id, _, capabilities)| {
                        capabilities
                            .map(|capabilities| (id.clone(), capabilities))
                    })
                    .collect();
                panel.agent_probe_results = results
                    .into_iter()
                    .map(|(id, result, _)| (id, Some(result)))
                    .collect();
                cx.notify();
            });
        })
        .detach();
    }

    pub fn update_agent_settings(
        &mut self,
        settings: AgentSettings,
        cx: &mut Context<Self>,
    ) {
        self.agent_settings = settings;
        self.agent_override_errors.clear();
        self.start_agent_probes(cx);
    }

    pub(super) fn agent_supports_interactive_variant(
        &self,
        profile_id: &str,
    ) -> Option<bool> {
        let profile = self.agent_settings.profile(profile_id)?;
        if profile.built_in != Some(BuiltInAgent::OpenCode) {
            return None;
        }
        self.agent_probe_capabilities
            .get(profile_id)
            .map(|capabilities| capabilities.supports_interactive_variant)
    }

    pub(super) fn agent_variant_capability_ready(
        &self,
        profile_id: &str,
    ) -> bool {
        let Some(profile) = self.agent_settings.profile(profile_id) else {
            return false;
        };
        if profile.built_in != Some(BuiltInAgent::OpenCode) {
            return true;
        }
        self.agent_probe_capabilities.contains_key(profile_id)
    }

    fn select_section(
        &mut self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) {
        self.section = section;
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.agent_profile_editor = None;
        cx.emit(SettingsPanelEvent::Close);
    }

    fn category_button(
        &self,
        id: &'static str,
        label: String,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.section == section;
        let this = cx.entity();
        // Selected rows use the same accent pair as hovered menu rows so the
        // label stays readable and inverted on every theme; list_active keeps
        // the theme foreground, which never flips on light accent blues.
        div()
            .id(id)
            .w_full()
            .px_3()
            .py_2()
            .rounded_md()
            .text_size(crate::theme::scaled_text_size(12.))
            .text_color(if selected {
                cx.theme().accent_foreground
            } else {
                cx.theme().colors.muted_foreground
            })
            .bg(if selected {
                cx.theme().tokens.accent.color
            } else {
                cx.theme().transparent
            })
            .hover(|element| {
                if selected {
                    // Keep the accent pairing while hovered; the list hover
                    // background would hide the inverted label.
                    element.bg(cx.theme().tokens.accent.color)
                } else {
                    element.bg(cx.theme().colors.list_hover)
                }
            })
            .on_click(move |_event, _window, cx| {
                this.update(cx, |panel, cx| panel.select_section(section, cx));
            })
            .child(shared(label))
    }

    fn field(
        label: String,
        control: AnyElement,
        foreground: Hsla,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(foreground)
                    .child(shared(label)),
            )
            .child(control)
    }

    fn section_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors.clone();
        let ui_font_size_control = h_flex()
            .w_full()
            .items_center()
            .gap_3()
            .child(Slider::new(&self.ui_font_size_state).flex_1())
            .child(
                div()
                    .w(px(52.))
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(format!("{:.0} px", self.ui_font_size))),
            );
        let diff_font_size_control = h_flex()
            .w_full()
            .items_center()
            .gap_3()
            .child(Slider::new(&self.diff_font_size_state).flex_1())
            .child(
                div()
                    .w(px(52.))
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(format!("{:.0} px", self.diff_font_size))),
            );
        match self.section {
            SettingsSection::General => v_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "settings-general",
                        ))),
                )
                .child(Self::field(
                    i18n::text(self.locale, "language-title"),
                    Select::new(&self.language_state)
                        .w_full()
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "auto-refresh-on-focus-title"),
                    Select::new(&self.auto_refresh_state)
                        .w_full()
                        .into_any_element(),
                    colors.foreground,
                ))
                .into_any_element(),
            SettingsSection::Appearance => v_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "settings-appearance",
                        ))),
                )
                .child(Self::field(
                    i18n::text(self.locale, "theme-title"),
                    Select::new(&self.theme_state).w_full().into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "ui-font-title"),
                    Select::new(&self.ui_font_state)
                        .w_full()
                        .search_placeholder(i18n::text(
                            self.locale,
                            "font-search-placeholder",
                        ))
                        .menu_width(px(360.))
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "mono-font-title"),
                    Select::new(&self.mono_font_state)
                        .w_full()
                        .search_placeholder(i18n::text(
                            self.locale,
                            "font-search-placeholder",
                        ))
                        .menu_width(px(360.))
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "ui-font-size-title"),
                    ui_font_size_control.into_any_element(),
                    colors.foreground,
                ))
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "ui-font-size-description",
                        ))),
                )
                .child(Self::field(
                    i18n::text(self.locale, "diff-font-size-title"),
                    diff_font_size_control.into_any_element(),
                    colors.foreground,
                ))
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "diff-font-size-description",
                        ))),
                )
                .into_any_element(),
            SettingsSection::Layout => v_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "settings-layout",
                        ))),
                )
                .child(Self::field(
                    i18n::text(self.locale, "diff-layout-title"),
                    Select::new(&self.diff_layout_state)
                        .w_full()
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "graph-history-title"),
                    Select::new(&self.graph_history_state)
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
                            "graph-history-description",
                        ))),
                )
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "layout-persistence-description",
                        ))),
                )
                .into_any_element(),
            SettingsSection::Agents => {
                let profiles =
                    agent_profile_options(self.locale, &self.agent_settings);
                let this = cx.entity();
                let profile_rows = profiles.iter().map(|profile| {
                    let resolved = self.agent_settings.profile(&profile.value);
                    let executable_path_missing = resolved
                        .as_ref()
                        .is_some_and(|profile| explicit_executable_path_missing(&profile.executable));
                    let probe_result = self
                        .agent_probe_results
                        .iter()
                        .find(|(id, _)| id == &profile.value)
                        .and_then(|(_, result)| result.as_ref());
                    let executable = resolved
                        .as_ref()
                        .map(|profile| profile.executable.display().to_string())
                        .unwrap_or_else(|| {
                            i18n::text(self.locale, "agent-profile-invalid")
                        });
                    let probe_label = if resolved.is_none() {
                        i18n::text(self.locale, "agent-profile-invalid")
                    } else if executable_path_missing {
                        i18n::text(self.locale, "agent-executable-not-found")
                    } else {
                        match probe_result {
                            Some(Ok(version)) => version.clone(),
                            Some(Err(error)) => i18n::text_args(
                                self.locale,
                                "agent-probe-unavailable",
                                &[("error", error)],
                            ),
                            None => {
                                i18n::text(self.locale, "agent-probe-checking")
                            }
                        }
                    };
                    let custom_id = self
                        .agent_settings
                        .custom_profiles
                        .iter()
                        .find(|custom| custom.id == profile.value)
                        .map(|_| profile.value.clone());
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
                            let has_variant = resolved
                                .built_in
                                == Some(BuiltInAgent::OpenCode)
                                && self
                                    .agent_settings
                                    .launch_overrides_for(resolved)
                                    .variant
                                    .is_some();
                            has_variant.then(|| {
                                self.agent_variant_capability_ready(
                                    &resolved.id,
                                )
                            })
                        })
                        .unwrap_or(true);
                    let can_test = resolved.is_some()
                        && !executable_path_missing
                        && launch_override_valid
                        && variant_capability_ready;
                    let test = this.clone();
                    let test_id = profile.value.clone();
                    let mut row = v_flex()
                        .w_full()
                        .gap_1()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(colors.secondary)
                        .child(
                            div()
                                .text_color(colors.foreground)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(profile.label.clone()),
                        )
                        .child(
                            div()
                                .text_color(colors.muted_foreground)
                                .text_size(crate::theme::scaled_text_size(11.))
                                .child(SharedString::from(executable)),
                        )
                        .child(
                            div()
                                .text_color(colors.muted_foreground)
                                .text_size(crate::theme::scaled_text_size(11.))
                                .child(SharedString::from(probe_label)),
                        );
                    let mut actions = h_flex()
                        .w_full()
                        .justify_end()
                        .gap_1()
                        .child(
                            Button::new(SharedString::from(format!(
                                "agent-profile-test-{}",
                                profile.value
                            )))
                            .label(i18n::text(
                                self.locale,
                                "agent-profile-test",
                            ))
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
                    if let Some(id) = custom_id {
                        let edit = this.clone();
                        let remove = this.clone();
                        let edit_id = id.clone();
                        let remove_id = id.clone();
                        actions = actions
                            .child(
                                    Button::new(SharedString::from(format!(
                                        "agent-profile-edit-{id}"
                                    )))
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-profile-edit",
                                    ))
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
                                        "agent-profile-remove-{id}"
                                    )))
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-profile-remove",
                                    ))
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
                    row = row.child(actions);
                    row
                });
                let new_profile = this.clone();
                v_flex()
                    .w_full()
                    .gap_4()
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(20.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(colors.foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "settings-agents",
                            ))),
                    )
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
                    )
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(12.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "agent-profiles-description",
                            ))),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(12.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "agent-test-description",
                            ))),
                    )
                    .child(
                        div()
                            .text_color(colors.foreground)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(shared(i18n::text(
                                self.locale,
                                "agent-executable-title",
                            ))),
                    )
                    .children(self.agent_executable_inputs.iter().map(
                        |(agent, input)| {
                            v_flex()
                                .w_full()
                                .gap_1()
                                .child(
                                    div()
                                        .text_color(colors.muted_foreground)
                                        .text_size(
                                            crate::theme::scaled_text_size(12.),
                                        )
                                        .child(SharedString::from(
                                            agent.display_name(),
                                        )),
                                )
                                .child(Input::new(input).w_full())
                        },
                    ))
                    .child(
                        div()
                            .text_color(colors.foreground)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(shared(i18n::text(
                                self.locale,
                                "agent-launch-settings-title",
                            ))),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(12.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "agent-launch-settings-description",
                            ))),
                    )
                    .children(BuiltInAgent::ALL.iter().filter_map(|agent| {
                        let model_input = self
                            .agent_model_inputs
                            .iter()
                            .find(|(entry, _)| entry == agent)
                            .map(|(_, input)| input)?;
                        let overrides =
                            self.agent_settings.launch_overrides.get(agent);
                        let override_error = self
                            .agent_override_errors
                            .get(agent)
                            .cloned()
                            .or_else(|| {
                                overrides.and_then(|overrides| {
                                    overrides.validate_for(*agent).err()
                                })
                            })
                            .map(|error| {
                                div()
                                    .text_size(crate::theme::scaled_text_size(
                                        11.,
                                    ))
                                    .text_color(colors.red)
                                    .child(shared(i18n::text_args(
                                        self.locale,
                                        "agent-launch-invalid",
                                        &[("error", &error)],
                                    )))
                            });
                        let reasoning_control: AnyElement = if *agent
                            == BuiltInAgent::OpenCode
                        {
                            let variant_input = self
                                .agent_variant_inputs
                                .iter()
                                .find(|(entry, _)| entry == agent)
                                .map(|(_, input)| input)?;
                            Self::field(
                                i18n::text(self.locale, "agent-variant-title"),
                                Input::new(variant_input)
                                    .w_full()
                                    .into_any_element(),
                                colors.foreground,
                            )
                            .into_any_element()
                        } else {
                            let reasoning_state = self
                                .agent_reasoning_states
                                .iter()
                                .find(|(entry, _)| entry == agent)
                                .map(|(_, state)| state)?;
                            Self::field(
                                i18n::text(
                                    self.locale,
                                    "agent-reasoning-title",
                                ),
                                Select::new(reasoning_state)
                                    .w_full()
                                    .into_any_element(),
                                colors.foreground,
                            )
                            .into_any_element()
                        };
                        let variant_note = (*agent == BuiltInAgent::OpenCode)
                            .then(|| {
                                div()
                                    .text_size(crate::theme::scaled_text_size(
                                        11.,
                                    ))
                                    .text_color(colors.muted_foreground)
                                    .child(shared(i18n::text(
                                        self.locale,
                                        "agent-opencode-variant-note",
                                    )))
                            });
                        Some(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(colors.secondary)
                                .child(
                                    div()
                                        .text_color(colors.foreground)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(SharedString::from(
                                            agent.display_name(),
                                        )),
                                )
                                .child(Self::field(
                                    i18n::text(
                                        self.locale,
                                        "agent-model-title",
                                    ),
                                    Input::new(model_input)
                                        .w_full()
                                        .into_any_element(),
                                    colors.foreground,
                                ))
                                .child(reasoning_control)
                                .when_some(variant_note, |element, note| {
                                    element.child(note)
                                })
                                .when_some(override_error, |element, error| {
                                    element.child(error)
                                }),
                        )
                    }))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_color(colors.foreground)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(shared(i18n::text(
                                        self.locale,
                                        "agent-custom-profiles-title",
                                    ))),
                            )
                            .child(
                                Button::new("agent-profile-add")
                                    .label(i18n::text(
                                        self.locale,
                                        "agent-profile-add",
                                    ))
                                    .ghost()
                                    .small()
                                    .on_click(move |_event, window, cx| {
                                        new_profile.update(cx, |panel, cx| {
                                            panel.open_agent_profile_editor(
                                                None, window, cx,
                                            );
                                        });
                                    }),
                            ),
                    )
                    .children(profile_rows)
                    .into_any_element()
            }
        }
    }
}

impl Render for SettingsPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let close = this.clone();
        let card = h_flex()
            .id("settings-card")
            .w(px(760.))
            .h(relative(0.9))
            .max_w(px(820.))
            .min_w(px(620.))
            .bg(colors.background)
            .border_1()
            .border_color(colors.border)
            .rounded_lg()
            .when(cx.theme().shadow, |element| element.shadow_lg())
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .w(px(172.))
                    .h_full()
                    .flex_shrink_0()
                    .p_3()
                    .gap_1()
                    .border_r_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .px_2()
                            .py_2()
                            .text_size(crate::theme::scaled_text_size(15.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(colors.foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "settings-title",
                            ))),
                    )
                    .child(self.category_button(
                        "settings-category-general",
                        i18n::text(self.locale, "settings-general"),
                        SettingsSection::General,
                        cx,
                    ))
                    .child(self.category_button(
                        "settings-category-appearance",
                        i18n::text(self.locale, "settings-appearance"),
                        SettingsSection::Appearance,
                        cx,
                    ))
                    .child(self.category_button(
                        "settings-category-layout",
                        i18n::text(self.locale, "settings-layout"),
                        SettingsSection::Layout,
                        cx,
                    ))
                    .child(self.category_button(
                        "settings-category-agents",
                        i18n::text(self.locale, "settings-agents"),
                        SettingsSection::Agents,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        h_flex()
                            .w_full()
                            .flex_shrink_0()
                            .items_center()
                            .justify_between()
                            .p_4()
                            .border_b_1()
                            .border_color(colors.border)
                            .child(
                                div()
                                    .text_size(crate::theme::scaled_text_size(
                                        13.,
                                    ))
                                    .text_color(colors.muted_foreground)
                                    .child(shared(i18n::text(
                                        self.locale,
                                        "settings-description",
                                    ))),
                            )
                            .child(
                                Button::new("settings-close")
                                    .icon(IconName::Close)
                                    .ghost()
                                    .small()
                                    .tooltip(i18n::text(
                                        self.locale,
                                        "settings-close",
                                    ))
                                    .on_click(move |_event, _window, cx| {
                                        close.update(cx, |panel, cx| {
                                            panel.close(cx)
                                        });
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .p_6()
                            .child(self.section_content(cx)),
                    ),
            );

        v_flex()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .bg(colors.background.opacity(0.9))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                this.update(cx, |panel, cx| panel.close(cx));
            })
            .child(card)
            .when_some(self.agent_profile_editor.clone(), |element, editor| {
                element.child(editor)
            })
    }
}

fn selected_index<T: Clone + PartialEq>(
    options: &[SettingsOption<T>],
    value: &T,
) -> Option<IndexPath> {
    options
        .iter()
        .position(|option| &option.value == value)
        .map(|index| IndexPath::default().row(index))
}

fn language_options(locale: Locale) -> Vec<SettingsOption<LanguagePreference>> {
    vec![
        SettingsOption::new(
            LanguagePreference::System,
            i18n::text(locale, "language-system"),
        ),
        SettingsOption::new(
            LanguagePreference::SimplifiedChinese,
            i18n::text(locale, "language-chinese"),
        ),
        SettingsOption::new(
            LanguagePreference::English,
            i18n::text(locale, "language-english"),
        ),
    ]
}

fn theme_options(locale: Locale) -> Vec<SettingsOption<ThemePreference>> {
    vec![
        SettingsOption::new(
            ThemePreference::GitHubDark,
            i18n::text(locale, "theme-github-dark"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinLatte,
            i18n::text(locale, "theme-catppuccin-latte"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinFrappe,
            i18n::text(locale, "theme-catppuccin-frappe"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinMacchiato,
            i18n::text(locale, "theme-catppuccin-macchiato"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinMocha,
            i18n::text(locale, "theme-catppuccin-mocha"),
        ),
    ]
}

fn diff_layout_options(
    locale: Locale,
) -> Vec<SettingsOption<DiffLayoutPreference>> {
    vec![
        SettingsOption::new(
            DiffLayoutPreference::SideBySide,
            i18n::text(locale, "diff-layout-side-by-side"),
        ),
        SettingsOption::new(
            DiffLayoutPreference::Inline,
            i18n::text(locale, "diff-layout-inline"),
        ),
    ]
}

fn graph_history_options(
    locale: Locale,
) -> Vec<SettingsOption<GraphHistoryPreference>> {
    vec![
        SettingsOption::new(
            GraphHistoryPreference::CurrentBranch,
            i18n::text(locale, "graph-history-current"),
        ),
        SettingsOption::new(
            GraphHistoryPreference::AllBranches,
            i18n::text(locale, "graph-history-all"),
        ),
    ]
}

fn auto_refresh_options(locale: Locale) -> Vec<SettingsOption<bool>> {
    vec![
        SettingsOption::new(true, i18n::text(locale, "setting-enabled")),
        SettingsOption::new(false, i18n::text(locale, "setting-disabled")),
    ]
}

fn agent_reasoning_options(
    locale: Locale,
    agent: BuiltInAgent,
) -> Vec<SettingsOption<Option<String>>> {
    let mut options = vec![SettingsOption::new(
        None,
        i18n::text(locale, "agent-launch-inherit"),
    )];
    options.extend(agent.supported_reasoning_efforts().iter().map(|effort| {
        SettingsOption::new(
            Some((*effort).to_string()),
            i18n::text_args(
                locale,
                "agent-reasoning-option",
                &[("effort", effort)],
            ),
        )
    }));
    options
}

fn font_options(
    locale: Locale,
    families: &[String],
) -> Vec<SettingsOption<Option<String>>> {
    let mut options = Vec::with_capacity(families.len() + 1);
    options.push(SettingsOption::new(
        None,
        i18n::text(locale, "font-system-default"),
    ));
    options.extend(
        families
            .iter()
            .cloned()
            .map(|family| SettingsOption::new(Some(family.clone()), family)),
    );
    options
}

fn agent_profile_options(
    _locale: Locale,
    settings: &AgentSettings,
) -> Vec<SettingsOption<String>> {
    let mut options = BuiltInAgent::ALL
        .iter()
        .map(|agent| {
            SettingsOption::new(agent.id().to_string(), agent.display_name())
        })
        .collect::<Vec<_>>();
    for profile in &settings.custom_profiles {
        if options.iter().any(|option| option.value == profile.id) {
            continue;
        }
        options.push(SettingsOption::new(
            profile.id.clone(),
            profile.name.clone(),
        ));
    }
    options
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}

fn explicit_executable_path_missing(path: &std::path::Path) -> bool {
    let has_directory_component =
        path.is_absolute() || path.components().count() > 1;
    has_directory_component && crate::agent::resolve_executable(path).is_err()
}
