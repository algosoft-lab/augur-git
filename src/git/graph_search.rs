//! Search controls for the commit graph.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    theme::ThemeColor,
};

use crate::core::commit_search::CommitSearchField;
use crate::core::i18n::{self, Locale};
use crate::git::shared;

use super::GraphView;

pub(super) fn render(
    graph: Entity<GraphView>,
    search_input: &Entity<InputState>,
    locale: Locale,
    field: CommitSearchField,
    query: &str,
    matches: usize,
    total: usize,
    colors: ThemeColor,
) -> impl IntoElement {
    let field_label = i18n::text(
        locale,
        match field {
            CommitSearchField::Subject => "commit-search-subject",
            CommitSearchField::FullMessage => "commit-search-full-message",
        },
    );
    let subject_label = i18n::text(locale, "commit-search-subject");
    let full_message_label = i18n::text(locale, "commit-search-full-message");
    let field_graph = graph.clone();
    let field_button = Button::new("commit-search-field")
        .label(field_label)
        .icon(IconName::ChevronDown)
        .ghost()
        .xsmall()
        .dropdown_menu_with_anchor(
            Anchor::TopRight,
            move |menu, _window, _cx| {
                let subject_graph = field_graph.clone();
                let full_graph = field_graph.clone();
                menu.item(
                    PopupMenuItem::new(subject_label.clone())
                        .checked(field == CommitSearchField::Subject)
                        .on_click(move |_event, _window, cx| {
                            subject_graph.update(cx, |graph, cx| {
                                graph.set_search_field(
                                    CommitSearchField::Subject,
                                    cx,
                                );
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(full_message_label.clone())
                        .checked(field == CommitSearchField::FullMessage)
                        .on_click(move |_event, _window, cx| {
                            full_graph.update(cx, |graph, cx| {
                                graph.set_search_field(
                                    CommitSearchField::FullMessage,
                                    cx,
                                );
                            });
                        }),
                )
            },
        );
    let result_label = i18n::text_args(
        locale,
        "commit-search-results",
        &[
            ("matches", &matches.to_string()),
            ("total", &total.to_string()),
        ],
    );
    h_flex()
        .id("commit-search-toolbar")
        .w_full()
        .h(px(34.))
        .flex_shrink_0()
        .px_2()
        .gap_1()
        .items_center()
        .bg(colors.tab_bar)
        .border_b_1()
        .border_color(colors.border)
        .child(
            Input::new(search_input)
                .flex_1()
                .min_w(px(120.))
                .cleanable(true)
                .prefix(Icon::new(IconName::Search).small()),
        )
        .child(field_button)
        .when(!query.is_empty(), |toolbar| {
            toolbar.child(
                div()
                    .text_size(px(10.))
                    .text_color(colors.muted_foreground)
                    .child(shared(result_label)),
            )
        })
}
