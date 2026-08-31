//! Modal dialog content presenting a commit's complete message.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, h_flex, v_flex};

use crate::core::git::CommitMessage;
use crate::core::graph::LogRow;
use crate::core::i18n::{self, Locale};
use crate::git::shared;

/// Row snapshot shown by the dialog, plus its async-loaded full message.
#[derive(Clone, Debug)]
struct DialogCommit {
    oid: String,
    short: String,
    subject: String,
    author: String,
    date: String,
    decorations: String,
    message: Option<CommitMessage>,
}

/// Content of the "show full commit message" dialog.
///
/// The dialog host (see [`crate::git::graph`]) owns window chrome and the
/// scroll container; this view only renders the message content and updates
/// in place when the full message arrives from the Git worker.
pub struct CommitMessageDialog {
    commit: Option<DialogCommit>,
    locale: Locale,
}

impl CommitMessageDialog {
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

    /// Replace the displayed commit. Pass the cached full message when the
    /// graph already has one; otherwise the view shows a loading hint until
    /// [`CommitMessageDialog::set_message`] delivers it.
    pub fn set_commit(
        &mut self,
        row: &LogRow,
        message: Option<CommitMessage>,
        cx: &mut Context<Self>,
    ) {
        self.commit = Some(DialogCommit {
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

    /// Store an asynchronously loaded message, ignoring responses for a
    /// commit that is no longer displayed.
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
}

impl Render for CommitMessageDialog {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let mono = cx.theme().mono_font_family.clone();

        let Some(commit) = self.commit.as_ref() else {
            return v_flex().child(
                div()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(i18n::text(
                        self.locale,
                        "commit-message-loading",
                    ))),
            );
        };

        let subject = commit
            .message
            .as_ref()
            .filter(|message| !message.subject.is_empty())
            .map(|message| message.subject.clone())
            .unwrap_or_else(|| commit.subject.clone());

        let mut content = v_flex()
            .w_full()
            .gap_2()
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
                            .font_family(mono)
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

        match commit.message.as_ref() {
            Some(message) => {
                if !message.body.is_empty() {
                    content = content.child(
                        div()
                            .w_full()
                            .text_size(crate::theme::scaled_text_size(12.))
                            .text_color(colors.foreground)
                            .child(shared(message.body.clone())),
                    );
                }
                if !message.co_authors.is_empty() {
                    content = content.child(
                        v_flex()
                            .w_full()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(crate::theme::scaled_text_size(
                                        11.,
                                    ))
                                    .text_color(colors.muted_foreground)
                                    .child(shared(i18n::text(
                                        self.locale,
                                        "commit-coauthors",
                                    ))),
                            )
                            .children(message.co_authors.iter().map(
                                |co_author| {
                                    div()
                                        .text_size(
                                            crate::theme::scaled_text_size(11.),
                                        )
                                        .text_color(colors.foreground)
                                        .child(shared(co_author.display()))
                                },
                            )),
                    );
                }
            }
            None => {
                content = content.child(
                    div()
                        .text_size(crate::theme::scaled_text_size(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "commit-message-loading",
                        ))),
                );
            }
        }

        content.child(
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
