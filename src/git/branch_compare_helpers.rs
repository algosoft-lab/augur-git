//! Shared controls and pure helpers for the revision comparison view.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{h_flex, v_flex};

use crate::core::diff::stat_blocks;
use crate::core::git::{CompareRevision, CompareRevisionKind};
use crate::core::graph::LogRow;
use crate::core::i18n::{self, Locale};

use super::{lucide, shared};

pub(super) fn compare_field<T>(
    label: &str,
    control: T,
    label_color: Hsla,
) -> AnyElement
where
    T: IntoElement,
{
    v_flex()
        .gap_0p5()
        .child(
            div()
                .text_size(px(10.))
                .text_color(label_color)
                .child(shared(label.to_string())),
        )
        .child(control)
        .into_any_element()
}

pub(super) fn format_revision_label(
    locale: Locale,
    reference: &CompareRevision,
    subject: Option<&str>,
) -> String {
    let prefix = match reference.kind {
        CompareRevisionKind::Local => {
            i18n::text(locale, "branch-compare-local")
        }
        CompareRevisionKind::Remote => {
            i18n::text(locale, "branch-compare-remote")
        }
        CompareRevisionKind::Tag => i18n::text(locale, "branch-compare-tag"),
        CompareRevisionKind::Commit => {
            i18n::text(locale, "branch-compare-commit")
        }
    };
    match subject.filter(|subject| !subject.is_empty()) {
        Some(subject) => format!("{prefix} · {} · {subject}", reference.name),
        None => format!("{prefix} · {}", reference.name),
    }
}

pub(super) fn format_commit_revision_label(
    locale: Locale,
    reference: &CompareRevision,
    row: &LogRow,
) -> String {
    let prefix = i18n::text(locale, "branch-compare-commit");
    let mut parts = vec![prefix, reference.name.clone()];
    if !row.subject.is_empty() {
        parts.push(row.subject.clone());
    }
    if !row.date.is_empty() {
        parts.push(row.date.clone());
    }
    if !row.decorations.is_empty() {
        parts.push(row.decorations.clone());
    }
    parts.join(" · ")
}

pub(super) fn choose_selection(
    current: &Option<CompareRevision>,
    values: &[CompareRevision],
    current_branch: &str,
) -> Option<CompareRevision> {
    current
        .as_ref()
        .and_then(|value| {
            values.iter().find(|candidate| *candidate == value).cloned()
        })
        .or_else(|| {
            values
                .iter()
                .find(|value| {
                    value.kind == CompareRevisionKind::Local
                        && value.name == current_branch
                })
                .cloned()
        })
        .or_else(|| values.first().cloned())
}

pub(super) fn choose_target(
    current: &Option<CompareRevision>,
    values: &[CompareRevision],
    base: Option<&CompareRevision>,
) -> Option<CompareRevision> {
    current
        .as_ref()
        .and_then(|value| {
            values.iter().find(|candidate| *candidate == value).cloned()
        })
        .filter(|value| {
            base.is_none_or(|base| value.full_name != base.full_name)
        })
        .or_else(|| {
            values
                .iter()
                .find(|value| {
                    base.is_none_or(|base| value.full_name != base.full_name)
                })
                .cloned()
        })
}

pub(super) fn empty_state(
    id: &'static str,
    colors: &gpui_component::theme::ThemeColor,
    message: String,
) -> Stateful<Div> {
    v_flex()
        .id(id)
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(colors.muted_foreground)
        .child(lucide("git-branch"))
        .child(shared(message))
}

pub(super) fn stat_bar(
    colors: &gpui_component::theme::ThemeColor,
    added: usize,
    deleted: usize,
) -> Div {
    let (green, red) = stat_blocks(added, deleted);
    let mut bar = h_flex().gap(px(1.)).items_center();
    for index in 0..5 {
        let color = if index < green {
            colors.green
        } else if index < green + red {
            colors.red
        } else {
            colors.muted_foreground.opacity(0.5)
        };
        bar = bar.child(div().w(px(3.)).h(px(8.)).rounded(px(1.)).bg(color));
    }
    h_flex()
        .gap_1()
        .items_center()
        .flex_shrink_0()
        .text_size(px(10.))
        .child(
            div()
                .text_color(colors.green)
                .child(shared(format!("+{added}"))),
        )
        .child(
            div()
                .text_color(colors.red)
                .child(shared(format!("-{deleted}"))),
        )
        .child(bar)
}

pub(super) fn stat_summary(
    colors: &gpui_component::theme::ThemeColor,
    added: usize,
    deleted: usize,
) -> Div {
    h_flex()
        .gap_1()
        .items_center()
        .text_size(px(11.))
        .child(
            div()
                .text_color(colors.green)
                .child(shared(format!("+{added}"))),
        )
        .child(
            div()
                .text_color(colors.red)
                .child(shared(format!("-{deleted}"))),
        )
}

pub(super) fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}
