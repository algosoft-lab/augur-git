use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    v_flex,
};

use crate::core::config::LanguagePreference;
use crate::core::i18n;
use crate::git::shared;

use super::Workspace;

/// Render the welcome page shown when no repository is open.
pub(super) fn render_welcome(
    workspace: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let colors = cx.theme().colors.clone();

    // Open button reads the shared repository-path input.
    let btn_open = cx.entity();
    let open_btn = div()
        .id("welcome-open")
        .px_3()
        .py_1()
        .rounded_md()
        .bg(colors.blue)
        .text_color(gpui::white())
        .text_size(px(12.))
        .child(shared(i18n::text(workspace.locale, "welcome-open")))
        .on_click(move |_e, _w, cx| {
            btn_open.update(cx, |ws, cx| ws.open_repo_from_input(cx));
        });

    // Browse button opens the system folder picker.
    let btn_browse = cx.entity();
    let browse_btn = div()
        .id("welcome-browse")
        .px_3()
        .py_1()
        .rounded_md()
        .bg(colors.input)
        .text_color(colors.foreground)
        .text_size(px(12.))
        .child(shared(i18n::text(workspace.locale, "welcome-browse")))
        .on_click(move |_e, _w, cx| {
            btn_browse.update(cx, |ws, cx| ws.pick_repo_folder(cx));
        });

    let recents = workspace
        .config
        .recent_repos
        .iter()
        .map(|path| {
            let this = cx.entity();
            let path = path.clone();
            h_flex()
                .id(SharedString::from(format!("welcome-recent-{path}")))
                .w(px(380.))
                .px_2()
                .py_1()
                .rounded_md()
                .gap_2()
                .hover(|this| this.bg(colors.list_hover))
                .items_center()
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(colors.muted_foreground)
                        .child(Icon::new(IconName::Folder)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(colors.muted_foreground)
                        .child(SharedString::from(path.clone())),
                )
                .on_click(move |_e, _w, cx| {
                    this.update(cx, |ws, cx| {
                        ws.git_view
                            .update(cx, |view, cx| view.open_repo(&path, cx));
                    });
                })
        })
        .collect::<Vec<_>>();

    v_flex()
        .id("welcome")
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(48.))
                .h(px(48.))
                .rounded(px(12.))
                .bg(colors.tab_bar)
                .border_1()
                .border_color(colors.border)
                .text_size(px(24.))
                .text_color(colors.blue)
                .child(crate::git::lucide("git-branch")),
        )
        .child(
            div()
                .text_size(px(20.))
                .font_weight(FontWeight::BOLD)
                .text_color(colors.foreground)
                .child("augur-git"),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(colors.muted_foreground)
                .child(shared(i18n::text(workspace.locale, "app-tagline"))),
        )
        // Path input row: input field, open button, and browse button.
        .child(
            h_flex()
                .w(px(380.))
                .gap_2()
                .child(Input::new(&workspace.repo_path_input).flex_1())
                .child(open_btn)
                .child(browse_btn),
        )
        .when(!recents.is_empty(), |w| {
            w.child(
                v_flex()
                    .w(px(380.))
                    .gap_0p5()
                    .mt_2()
                    .child(
                        div()
                            .px_2()
                            .text_size(px(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                workspace.locale,
                                "recent-repos",
                            ))),
                    )
                    .children(recents),
            )
        })
}

/// Render the settings overlay for language selection.
pub(super) fn render_settings_overlay(
    workspace: &Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let colors = cx.theme().colors.clone();
    let locale = workspace.locale;
    let current = workspace.language_preference;

    let lang_btn = |id: &'static str,
                    key: &str,
                    pref: LanguagePreference,
                    cx: &Context<Workspace>| {
        let this = cx.entity();
        Button::new(id)
            .label(i18n::text(locale, key))
            .flex_1()
            .when(pref == current, |b| b.primary())
            .when(pref != current, |b| b.ghost())
            .on_click(move |_e, window, cx| {
                this.update(cx, |ws, cx| ws.set_language(pref, window, cx));
            })
    };

    let this_close = cx.entity();
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
        // Clicking the overlay closes the settings panel.
        .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
            this_close.update(cx, |ws, cx| {
                ws.show_settings = false;
                cx.notify();
            });
        })
        .child(
            v_flex()
                .id("settings-card")
                .items_start()
                .gap_3()
                .p_6()
                .bg(colors.background)
                .border_1()
                .border_color(colors.border)
                .rounded_lg()
                .min_w(px(380.))
                .max_w(px(460.))
                .when(cx.theme().shadow, |el| el.shadow_md())
                // Stop clicks inside the card from closing the overlay.
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(locale, "settings-title"))),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.foreground)
                                .child(shared(i18n::text(
                                    locale,
                                    "language-title",
                                ))),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .child(lang_btn(
                                    "lang-system",
                                    "language-system",
                                    LanguagePreference::System,
                                    cx,
                                ))
                                .child(lang_btn(
                                    "lang-chinese",
                                    "language-chinese",
                                    LanguagePreference::SimplifiedChinese,
                                    cx,
                                ))
                                .child(lang_btn(
                                    "lang-english",
                                    "language-english",
                                    LanguagePreference::English,
                                    cx,
                                )),
                        ),
                )
                .child(
                    Button::new("settings-close")
                        .label(i18n::text(locale, "settings-close"))
                        .ghost()
                        .w_full()
                        .on_click({
                            let this = cx.entity();
                            move |_e, _w, cx| {
                                this.update(cx, |ws, cx| {
                                    ws.show_settings = false;
                                    cx.notify();
                                });
                            }
                        }),
                ),
        )
}
