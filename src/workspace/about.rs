//! About window for the application shell.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, TitleBar, h_flex, v_flex};

use crate::core::i18n::Locale;
use crate::core::{build_info, i18n};

use super::Workspace;

pub(super) struct AboutWindow {
    locale: Locale,
}

impl AboutWindow {
    fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub(super) fn set_locale(
        &mut self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        cx.notify();
    }
}

pub(super) fn open_about_window(
    workspace: &mut Workspace,
    cx: &mut Context<Workspace>,
) {
    if let Some(existing) = workspace.about_window {
        if existing
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        workspace.about_window = None;
    }

    let locale = workspace.locale;
    let window_size = size(px(400.), px(340.));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(window_size, cx)),
        is_resizable: false,
        is_minimizable: false,
        kind: WindowKind::Floating,
        window_min_size: Some(window_size),
        ..TitleBar::window_options()
    };

    match cx.open_window(options, |window, cx| {
        let about_window = cx.new(|_| AboutWindow::new(locale));
        window.activate_window();
        about_window
    }) {
        Ok(handle) => workspace.about_window = Some(handle),
        Err(error) => {
            log::error!(
                "[workspace_about] failed to open About window: {error}"
            );
        }
    }
}

impl Render for AboutWindow {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let locale = self.locale;

        v_flex()
            .id("about-window")
            .size_full()
            .bg(colors.background)
            .child(
                TitleBar::new().child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(i18n::text(locale, "menu-about")),
                ),
            )
            .child(render_about_content(locale, &colors))
    }
}

fn render_about_content(
    locale: Locale,
    colors: &gpui_component::theme::ThemeColor,
) -> impl IntoElement {
    v_flex()
        .id("about-content")
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .px_6()
        .gap_2()
        .child(
            v_flex()
                .id("about-summary")
                .items_center()
                .gap_1p5()
                .child(img("augur-git-logo.svg").size(px(72.)))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.muted_foreground)
                        .child(i18n::text(locale, "about-title")),
                )
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(build_info::APP_NAME),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.muted_foreground)
                        .child(i18n::text(locale, "about-tagline")),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .gap_1()
                .mt_3()
                .child(metadata_row(
                    i18n::text(locale, "about-author"),
                    build_info::APP_AUTHORS,
                    colors,
                ))
                .child(metadata_row(
                    i18n::text(locale, "about-version"),
                    build_info::APP_VERSION,
                    colors,
                ))
                .child(metadata_row(
                    i18n::text(locale, "about-commit"),
                    build_info::GIT_COMMIT,
                    colors,
                )),
        )
}

fn metadata_row(
    label: String,
    value: &'static str,
    colors: &gpui_component::theme::ThemeColor,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_start()
        .gap_3()
        .child(
            div()
                .w(px(76.))
                .flex_shrink_0()
                .text_size(px(12.))
                .text_color(colors.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(12.))
                .text_color(colors.foreground)
                .child(value),
        )
}
