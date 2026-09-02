//! Native window hosting the extension management surface.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, TitleBar, v_flex};

use super::Workspace;
use super::extensions::ExtensionsPanel;
use super::window_state;
use crate::core::config::WindowState;
use crate::core::i18n::{self, Locale};

pub(super) struct ExtensionsWindow {
    pub(super) panel: Entity<ExtensionsPanel>,
    locale: Locale,
}

impl ExtensionsWindow {
    pub(super) fn new(
        panel: Entity<ExtensionsPanel>,
        locale: Locale,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let workspace_for_events = workspace.clone();
        cx.subscribe_in(
            &panel,
            window,
            move |_owner, _panel, event, window, cx| {
                let _ = workspace_for_events.update(cx, |workspace, cx| {
                    workspace.handle_extensions_panel_event(event, window, cx);
                });
            },
        )
        .detach();
        let workspace_for_bounds = workspace;
        cx.observe_window_bounds(window, move |_owner, window, cx| {
            let _ = workspace_for_bounds.update(cx, |workspace, _| {
                window_state::update_ui_state_extensions_window(
                    &mut workspace.ui_state,
                    window,
                );
            });
        })
        .detach();
        Self { panel, locale }
    }

    pub(super) fn set_locale(
        &mut self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        self.panel.update(cx, |panel, _cx| panel.set_locale(locale));
        cx.notify();
    }
}

pub(super) fn window_options(
    cx: &mut Context<Workspace>,
    state: &WindowState,
) -> WindowOptions {
    let mut options =
        window_state::initial_extensions_window_options(cx, state);
    options.is_resizable = true;
    options.is_minimizable = true;
    options.kind = WindowKind::Normal;
    options.window_decorations = Some(WindowDecorations::Client);
    options
}

impl Render for ExtensionsWindow {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        window.set_rem_size(cx.theme().font_size);
        let colors = cx.theme().colors.clone();
        let title = i18n::text(self.locale, "extensions-title");
        v_flex()
            .id("extensions-window")
            .size_full()
            .bg(colors.background)
            .child(
                TitleBar::new().child(
                    div()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(SharedString::from(title)),
                ),
            )
            .child(self.panel.clone())
    }
}
