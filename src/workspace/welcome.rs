use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, Icon, IconName, h_flex, v_flex};

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

    // Open button picks a repository folder and loads it directly.
    let btn_open = cx.entity();
    let open_btn = div()
        .id("welcome-open")
        .px_4()
        .py_1()
        .rounded_md()
        .bg(colors.blue)
        .text_color(colors.primary_foreground)
        .text_size(px(12.))
        .child(shared(i18n::text(workspace.locale, "welcome-open")))
        .on_click(move |_event, window, cx| {
            btn_open.update(cx, |ws, cx| {
                ws.pick_repo_folder(window, cx);
            });
        });

    let recents = workspace
        .config
        .recent_repos
        .iter()
        .map(|path| {
            let this = cx.entity();
            let path = path.clone();
            let path_for_click = path.clone();
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
                .on_click(move |_event, window, cx| {
                    this.update(cx, |ws, cx| {
                        ws.open_repo_path(
                            path_for_click.clone(),
                            false,
                            window,
                            cx,
                        );
                    });
                })
        })
        .collect::<Vec<_>>();

    // Accept a folder dropped from the OS file manager anywhere on the page.
    let this = cx.entity();
    v_flex()
        .id("welcome")
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        // The border is always reserved so the drag-over highlight cannot
        // shift the layout; it only becomes visible while paths hover the page.
        .border_1()
        .border_color(transparent_black())
        .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
            style.border_color(cx.theme().colors.blue)
        })
        .on_drop(move |paths: &ExternalPaths, window, cx| {
            this.update(cx, |workspace, cx| {
                workspace.open_dropped_paths(paths.paths(), window, cx);
            });
        })
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
        // Single action: pick a repository folder and open it.
        .child(open_btn)
        .child(
            div()
                .text_size(px(11.))
                .text_color(colors.muted_foreground)
                .child(shared(i18n::text(
                    workspace.locale,
                    "welcome-drop-hint",
                ))),
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
