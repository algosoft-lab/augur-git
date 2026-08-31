//! Shared controls and pure helpers for the branch comparison view.

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, v_flex};

use crate::core::diff::stat_blocks;
use crate::core::git::{BranchCompareMode, BranchRefInfo, BranchRefKind};
use crate::core::i18n::{self, Locale};

use super::{BranchCompareView, lucide, shared};

pub(super) fn compare_field<T>(
    label: &str,
    control: T,
    label_color: Hsla,
) -> impl IntoElement
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
}

pub(super) fn mode_button(
    id: &'static str,
    label: String,
    selected: bool,
    entity: Entity<BranchCompareView>,
    mode: BranchCompareMode,
) -> impl IntoElement {
    let button = Button::new(id).label(label).compact();
    if selected {
        button.primary().on_click(move |_event, _window, cx| {
            entity.update(cx, |view, cx| view.set_mode(mode, cx));
        })
    } else {
        button.ghost().on_click(move |_event, _window, cx| {
            entity.update(cx, |view, cx| view.set_mode(mode, cx));
        })
    }
}

pub(super) fn format_branch_label(
    locale: Locale,
    reference: &BranchRefInfo,
) -> String {
    let prefix = match reference.kind {
        BranchRefKind::Local => i18n::text(locale, "branch-compare-local"),
        BranchRefKind::Remote => i18n::text(locale, "branch-compare-remote"),
    };
    format!("{prefix} · {}", reference.name)
}

pub(super) fn choose_selection(
    current: &Option<BranchRefInfo>,
    values: &[BranchRefInfo],
    current_branch: &str,
) -> Option<BranchRefInfo> {
    current
        .as_ref()
        .and_then(|value| {
            values.iter().find(|candidate| *candidate == value).cloned()
        })
        .or_else(|| {
            values
                .iter()
                .find(|value| {
                    value.kind == BranchRefKind::Local
                        && value.name == current_branch
                })
                .cloned()
        })
        .or_else(|| values.first().cloned())
}

pub(super) fn choose_target(
    current: &Option<BranchRefInfo>,
    values: &[BranchRefInfo],
    base: Option<&BranchRefInfo>,
) -> Option<BranchRefInfo> {
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
