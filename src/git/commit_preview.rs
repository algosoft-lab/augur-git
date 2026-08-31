//! Live full-message preview shown when a commit row is hovered.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, h_flex, v_flex};

use crate::core::git::CommitMessage;
use crate::core::graph::LogRow;
use crate::core::i18n::{self, Locale};
use crate::git::shared;

#[derive(Clone, Debug)]
struct HoverCommit {
    oid: String,
    short: String,
    subject: String,
    author: String,
    date: String,
    decorations: String,
    message: Option<CommitMessage>,
}

/// Live tooltip content for a commit row.
pub struct CommitHoverPreview {
    commit: Option<HoverCommit>,
    locale: Locale,
}

impl CommitHoverPreview {
    pub fn new(locale: Locale) -> Self {
        Self {
            commit: None,
            locale,
        }
    }

    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    pub fn set_commit(
        &mut self,
        row: &LogRow,
        message: Option<CommitMessage>,
        cx: &mut Context<Self>,
    ) {
        self.commit = Some(HoverCommit {
            oid: row.oid.clone(),
            short: row.short.clone(),
            subject: row.subject.clone(),
            author: row.author.clone(),
            date: row.date.clone(),
            decorations: row.decorations.clone(),
            message,
        });
        cx.notify();
    }

    pub fn set_message(
        &mut self,
        oid: &str,
        message: CommitMessage,
        cx: &mut Context<Self>,
    ) {
        let Some(commit) = self.commit.as_mut() else {
            return;
        };
        if commit.oid == oid {
            commit.message = Some(message);
            cx.notify();
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.commit.take().is_some() {
            cx.notify();
        }
    }
}

impl Render for CommitHoverPreview {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let mono = cx.theme().mono_font_family.clone();
        let mut preview = v_flex()
            .id("commit-hover-preview")
            .w(px(360.))
            .max_h(px(320.))
            .overflow_y_scroll()
            .gap_2()
            .p_2();

        let Some(commit) = self.commit.as_ref() else {
            return preview;
        };

        let subject = commit
            .message
            .as_ref()
            .map(|message| {
                if message.subject.is_empty() {
                    commit.subject.clone()
                } else {
                    message.subject.clone()
                }
            })
            .unwrap_or_else(|| commit.subject.clone());

        preview = preview
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(10.))
                    .text_color(colors.muted_foreground)
                    .child(shared(i18n::text(
                        self.locale,
                        "commit-message-preview",
                    ))),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(colors.input)
                            .font_family(mono.clone())
                            .text_size(crate::theme::scaled_text_size(11.))
                            .text_color(colors.accent)
                            .child(shared(commit.short.clone())),
                    )
                    .when(!commit.decorations.is_empty(), |row| {
                        row.child(
                            div()
                                .text_size(crate::theme::scaled_text_size(10.))
                                .text_color(colors.muted_foreground)
                                .truncate()
                                .child(shared(commit.decorations.clone())),
                        )
                    }),
            )
            .child(
                div()
                    .w_full()
                    .text_size(crate::theme::scaled_text_size(13.))
                    .text_color(colors.foreground)
                    .child(shared(subject)),
            );

        if let Some(message) = commit.message.as_ref() {
            if !message.body.is_empty() {
                preview = preview.child(
                    div()
                        .w_full()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.foreground)
                        .child(shared(message.body.clone())),
                );
            }
            if !message.co_authors.is_empty() {
                preview = preview.child(
                    v_flex()
                        .w_full()
                        .gap_0p5()
                        .child(
                            div()
                                .text_size(crate::theme::scaled_text_size(11.))
                                .text_color(colors.muted_foreground)
                                .child(shared(i18n::text(
                                    self.locale,
                                    "commit-coauthors",
                                ))),
                        )
                        .children(message.co_authors.iter().map(|co_author| {
                            div()
                                .text_size(crate::theme::scaled_text_size(11.))
                                .text_color(colors.foreground)
                                .child(shared(co_author.display()))
                        })),
                );
            }
        } else {
            preview = preview.child(
                div()
                    .text_size(crate::theme::scaled_text_size(11.))
                    .text_color(colors.muted_foreground)
                    .child(shared(i18n::text(
                        self.locale,
                        "commit-message-loading",
                    ))),
            );
        }

        preview.child(
            h_flex()
                .gap_3()
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text_args(
                            self.locale,
                            "commit-author",
                            &[("author", &commit.author)],
                        ))),
                )
                .child(
                    div()
                        .text_size(crate::theme::scaled_text_size(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text_args(
                            self.locale,
                            "commit-date",
                            &[("date", &commit.date)],
                        ))),
                ),
        )
    }
}
