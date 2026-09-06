//! Windows-only dialog for opening a repository inside a WSL distribution.
//!
//! The dialog lists the installed distributions (`wsl -l -q`), accepts an
//! absolute Linux path (including `\\wsl$` UNC pastes, which are split into
//! distro and path), and validates the combination inline with the same
//! `rev-parse` probe the worker uses on open.

use std::time::Duration;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::PopupMenuItem,
    v_flex,
};

use crate::core::config::LocationConfig;
use crate::core::git::{self, GitError};
use crate::core::i18n::{self, Locale};
use crate::dropdown::DropdownMenuExt;
use crate::git::shared;

/// WSL open dialog → Workspace events.
#[derive(Clone, Debug)]
pub enum WslOpenDialogEvent {
    /// The user confirmed a validated distro/path combination.
    Open { distro: String, path: String },
}

/// Monotone probe identity: a finished probe only applies when it is still
/// the most recently requested one.
type ProbeId = u64;

enum Validation {
    Idle,
    Probing,
    Ok,
    Failed { key: String, detail: String },
}

pub struct WslOpenDialog {
    locale: Locale,
    distros: Vec<String>,
    distros_loading: bool,
    distro: Option<String>,
    path_input: Entity<InputState>,
    probe_id: ProbeId,
    validation: Validation,
}

impl EventEmitter<WslOpenDialogEvent> for WslOpenDialog {}

impl WslOpenDialog {
    pub fn new(
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("/srv/git"));
        cx.subscribe_in(
            &path_input,
            window,
            |this, _input, event: &InputEvent, window, cx| match event {
                InputEvent::Change | InputEvent::PressEnter { .. } => {
                    this.schedule_probe(window, cx);
                }
                _ => {}
            },
        )
        .detach();

        cx.spawn_in(window, async move |dialog, cx| {
            load_distros(dialog, cx, false).await;
        })
        .detach();

        Self {
            locale,
            distros: Vec::new(),
            distros_loading: true,
            distro: None,
            path_input,
            probe_id: 0,
            validation: Validation::Idle,
        }
    }

    /// The raw path text currently entered.
    fn path_text(&self, cx: &Context<Self>) -> String {
        self.path_input.read(cx).value().to_string()
    }

    /// Translate a UNC paste into distro + path before validating.
    fn absorb_unc_input(
        &mut self,
        raw: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((distro, path)) = git::parse_unc_path(raw) {
            if self.distro.as_deref() != Some(distro.as_str()) {
                log::info!(
                    "[workspace_wsl] UNC input selects distro {distro:?}"
                );
                self.distro = Some(distro);
            }
            if self.path_text(cx) != path {
                self.path_input.update(cx, |input, cx| {
                    input.set_value(path, window, cx);
                });
            }
        }
    }

    /// Re-run validation for the current distro/path combination.
    fn schedule_probe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.path_text(cx);
        self.absorb_unc_input(&raw, window, cx);
        let path = self.path_text(cx);
        let parsed = match git::validate_linux_path(&path) {
            Ok(path) => path,
            Err(reason) => {
                self.probe_id = self.probe_id.wrapping_add(1);
                self.validation = Validation::Failed {
                    key: format!("wsl-path-{reason}"),
                    detail: String::new(),
                };
                cx.notify();
                return;
            }
        };
        let Some(distro) = self.distro.clone() else {
            self.probe_id = self.probe_id.wrapping_add(1);
            self.validation = Validation::Idle;
            cx.notify();
            return;
        };

        self.probe_id = self.probe_id.wrapping_add(1);
        let probe_id = self.probe_id;
        self.validation = Validation::Probing;
        cx.spawn_in(window, async move |dialog, cx| {
            // Debounce: only the latest keystroke's probe survives.
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let result = cx
                .background_executor()
                .spawn(async move { probe_repository(&distro, &parsed) })
                .await;
            let _ = dialog.update_in(cx, |dialog, _window, cx| {
                if dialog.probe_id == probe_id {
                    dialog.validation = match result {
                        Ok(()) => Validation::Ok,
                        Err(error) => Validation::Failed {
                            key: error.key.to_string(),
                            detail: error.detail,
                        },
                    };
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_distros(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.distros_loading = true;
        cx.notify();
        cx.spawn_in(window, async move |dialog, cx| {
            load_distros(dialog, cx, true).await;
        })
        .detach();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(distro) = self.distro.clone() else {
            return;
        };
        if !matches!(self.validation, Validation::Ok) {
            return;
        }
        // The confirm button is only enabled after a successful probe, but
        // the path is re-validated so the emit can never carry raw input.
        let Ok(path) = git::validate_linux_path(&self.path_text(cx)) else {
            return;
        };
        log::info!(
            "[workspace_wsl] opening wsl repository: distro={distro:?}, path={path:?}"
        );
        cx.emit(WslOpenDialogEvent::Open { distro, path });
    }

    fn validation_message(&self, cx: &Context<Self>) -> Option<(Hsla, String)> {
        let colors = &cx.theme().colors;
        match &self.validation {
            Validation::Idle => None,
            Validation::Probing => Some((
                colors.muted_foreground,
                i18n::text(self.locale, "wsl-checking"),
            )),
            Validation::Ok => {
                Some((colors.green, i18n::text(self.locale, "wsl-check-ok")))
            }
            Validation::Failed { key, detail } => {
                let message = if key.starts_with("wsl-path-") {
                    i18n::text(self.locale, key.as_str())
                } else {
                    i18n::text_args(
                        self.locale,
                        key.as_str(),
                        &[("detail", detail.as_str())],
                    )
                };
                Some((colors.red, message))
            }
        }
    }
}

/// Load (or reload) the distro list on the background executor, keeping a
/// selection that still exists.
async fn load_distros(
    dialog: WeakEntity<WslOpenDialog>,
    cx: &mut AsyncApp,
    keep_selection: bool,
) {
    let distros = cx
        .background_executor()
        .spawn(async { git::list_wsl_distros() })
        .await;
    let _ = dialog.update(cx, |dialog, cx| {
        dialog.distros_loading = false;
        let selection_valid = dialog
            .distro
            .as_ref()
            .is_some_and(|selected| distros.contains(selected));
        if !keep_selection || !selection_valid {
            dialog.distro = distros.first().cloned();
        }
        dialog.distros = distros;
        cx.notify();
    });
}

/// Blocking probe executed on the background executor.
fn probe_repository(distro: &str, path: &str) -> Result<(), GitError> {
    match LocationConfig::wsl(distro).to_repo(path.to_string()) {
        Ok(repo) => git::probe_wsl_repository(&repo),
        Err(error) => Err(error),
    }
}

impl Render for WslOpenDialog {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();

        let distro_label = self.distro.clone().unwrap_or_else(|| {
            if self.distros_loading {
                i18n::text(self.locale, "wsl-loading-distros")
            } else {
                i18n::text(self.locale, "wsl-no-distros")
            }
        });

        let distros = self.distros.clone();
        let selector = Button::new("wsl-distro-selector")
            .ghost()
            .label(distro_label)
            .icon(IconName::ChevronDown)
            .dropdown_menu_below(move |menu, _, _| {
                let this = this.clone();
                distros.iter().fold(menu, |menu, name| {
                    let name = name.clone();
                    let item_entity = this.clone();
                    menu.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_event, window, cx| {
                            item_entity.update(cx, |dialog, cx| {
                                dialog.distro = Some(name.clone());
                                dialog.schedule_probe(window, cx);
                            });
                        },
                    ))
                })
            });

        let refresh = {
            let this = cx.entity();
            Button::new("wsl-refresh-distros")
                .ghost()
                .small()
                .icon(crate::git::lucide("refresh-cw"))
                .tooltip(i18n::text(self.locale, "wsl-refresh-distros"))
                .on_click(move |_event, window, cx| {
                    this.update(cx, |dialog, cx| {
                        dialog.refresh_distros(window, cx);
                    });
                })
        };

        let validation = self.validation_message(cx).map(|(color, message)| {
            div()
                .text_size(crate::theme::scaled_text_size(12.))
                .text_color(color)
                .child(shared(message))
        });

        let can_open =
            self.distro.is_some() && matches!(self.validation, Validation::Ok);
        let confirm = {
            let this = cx.entity();
            let mut button = Button::new("wsl-open-confirm")
                .label(i18n::text(self.locale, "wsl-open-confirm"))
                .flex_1();
            button = if can_open {
                button.primary().on_click(move |_event, _window, cx| {
                    this.update(cx, |dialog, cx| dialog.confirm(cx));
                })
            } else {
                button.disabled(true)
            };
            button
        };

        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(12.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "wsl-distro-label",
                            ))),
                    )
                    .child(selector)
                    .child(refresh),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(12.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "wsl-path-label",
                            ))),
                    )
                    .child(div().w_full().child(Input::new(&self.path_input)))
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "wsl-path-hint",
                            ))),
                    )
                    .children(validation),
            )
            .child(confirm)
    }
}
