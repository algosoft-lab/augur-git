//! Agents section of the settings panel.
//!
//! Built-in agent presets are opt-in: they are listed here only after the
//! user adds them, and connectivity probes never touch an agent that has not
//! been added. Custom profiles live alongside built-ins as cards with a
//! collapsible launch-settings body.

use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    input::{InputEvent, InputState},
    select::SelectEvent,
};

use crate::agent::{AgentLaunchOverrides, AgentSettings, BuiltInAgent};
use crate::core::i18n::{self, Locale};

use super::super::agent_profiles::{
    AgentProfileEditor, AgentProfileEditorEvent,
};
use super::{
    SettingsOption, SettingsPanel, SettingsPanelEvent, SettingsSection,
};

impl SettingsPanel {
    /// Subscribe the per-agent input and select entities created during
    /// construction. Runs once from `SettingsPanel::new`.
    pub(super) fn wire_agent_subscriptions(&mut self, cx: &mut Context<Self>) {
        let agent_default_profile_state_for_events =
            self.agent_default_profile_state.clone();
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

        for (agent, input) in &self.agent_executable_inputs {
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

        for (agent, input) in &self.agent_model_inputs {
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

        for (agent, input) in &self.agent_variant_inputs {
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

        for (agent, state) in &self.agent_reasoning_states {
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

        self.start_agent_probes(cx);
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
        // Only agents the user has added are probed; a configuration with no
        // agents starts zero subprocesses at startup.
        let profiles = agent_profile_options(self.locale, &self.agent_settings)
            .into_iter()
            .filter_map(|option| {
                self.agent_settings
                    .profile(&option.value)
                    .map(|profile| (option.value, profile))
            })
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            self.agent_probe_results.clear();
            self.agent_probe_capabilities.clear();
            return;
        }
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

    pub(in crate::workspace) fn agent_supports_interactive_variant(
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

    pub(in crate::workspace) fn agent_variant_capability_ready(
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

    /// Jump the panel to the Agents section; used when an AI Git operation
    /// needs a profile but none has been configured yet.
    pub fn reveal_agents(&mut self, cx: &mut Context<Self>) {
        self.section = SettingsSection::Agents;
        cx.notify();
    }

    pub(super) fn toggle_agent_expanded(
        &mut self,
        profile_id: String,
        cx: &mut Context<Self>,
    ) {
        if !self.agent_expanded.remove(&profile_id) {
            self.agent_expanded.insert(profile_id);
        }
        cx.notify();
    }

    pub(super) fn open_agent_profile_editor(
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

    pub(super) fn remove_agent_profile(
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

    /// Open the native file picker so the user can point one built-in Agent
    /// at an executable that auto-detection cannot find (for example, a CLI
    /// installed outside the minimal PATH of a GUI desktop session).
    pub(super) fn browse_agent_executable(
        &mut self,
        agent: BuiltInAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, input)) = self
            .agent_executable_inputs
            .iter()
            .find(|(entry, _)| entry == &agent)
        else {
            return;
        };
        let input = input.clone();
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(SharedString::from(i18n::text(
                self.locale,
                "agent-executable-browse-prompt",
            ))),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                return;
            };
            let _ = cx.update(|window, app| {
                let _ = this.update(app, |panel, cx| {
                    panel.apply_agent_executable_path(
                        agent, &path, &input, window, cx,
                    );
                });
            });
        })
        .detach();
    }

    fn apply_agent_executable_path(
        &mut self,
        agent: BuiltInAgent,
        path: &std::path::Path,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!(
            "[agent_terminal] executable path selected for {}",
            agent.id()
        );
        let value = path.display().to_string();
        input.update(cx, |state, cx| {
            state.set_value(value, window, cx);
        });
        let already_selected =
            self.agent_settings.executable_overrides.get(&agent)
                == Some(&path.to_path_buf());
        if already_selected {
            cx.notify();
            return;
        }
        self.agent_settings
            .executable_overrides
            .insert(agent, path.to_path_buf());
        cx.emit(SettingsPanelEvent::AgentExecutableOverrideChanged {
            agent,
            executable: Some(path.to_path_buf()),
        });
        cx.notify();
    }
}

pub(super) fn agent_reasoning_options(
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

pub(super) fn agent_profile_options(
    _locale: Locale,
    settings: &AgentSettings,
) -> Vec<SettingsOption<String>> {
    // Only built-in agents the user has added appear in the UI and get
    // probed; custom profiles are always listed once they validate.
    let mut options = settings
        .enabled_builtins()
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

pub(super) fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}
