//! About page for the application shell.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use crate::core::{build_info, i18n};

use super::Workspace;

pub(super) fn render_about(
    workspace: &Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let colors = cx.theme().colors.clone();
    let locale = workspace.locale;
    let this = cx.entity();

    let back_button = Button::new("about-back")
        .label(i18n::text(locale, "about-back"))
        .ghost()
        .on_click(move |_event, _window, cx| {
            this.update(cx, |workspace, cx| {
                workspace.show_main(cx);
            });
        });

    v_flex()
        .id("about-page")
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .px_6()
        .child(
            v_flex()
                .id("about-card")
                .items_center()
                .gap_3()
                .p_6()
                .min_w(px(380.))
                .max_w(px(520.))
                .bg(colors.background)
                .border_1()
                .border_color(colors.border)
                .rounded_lg()
                .when(cx.theme().shadow, |element| element.shadow_md())
                .child(img("augur-git-logo.svg").size(px(104.)))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.muted_foreground)
                        .child(i18n::text(locale, "about-title")),
                )
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(build_info::APP_NAME),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.muted_foreground)
                        .child(i18n::text(locale, "about-tagline")),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap_1p5()
                        .mt_3()
                        .child(metadata_row(
                            i18n::text(locale, "about-author"),
                            build_info::APP_AUTHORS,
                            &colors,
                        ))
                        .child(metadata_row(
                            i18n::text(locale, "about-version"),
                            build_info::APP_VERSION,
                            &colors,
                        ))
                        .child(metadata_row(
                            i18n::text(locale, "about-commit"),
                            build_info::GIT_COMMIT,
                            &colors,
                        )),
                )
                .child(back_button),
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
