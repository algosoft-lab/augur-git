//! Repository-tab coordination for the dedicated branch comparison view.

use gpui::prelude::*;
use gpui::*;

use crate::core::i18n::Locale;
use crate::git::GitUiEvent;
use crate::git::branch_compare::{BranchCompareEvent, BranchCompareView};

use super::RepoTab;

pub(super) fn new_view(
    window: &mut Window,
    cx: &mut Context<RepoTab>,
    locale: Locale,
    diff_layout: crate::git::diff_view::DiffLayoutMode,
) -> Entity<BranchCompareView> {
    cx.new(|cx| BranchCompareView::new(window, cx, locale, diff_layout))
}

pub(super) fn subscribe(
    compare: &Entity<BranchCompareView>,
    cx: &mut Context<RepoTab>,
) {
    cx.subscribe(compare, |tab, _event, event, cx| match event {
        BranchCompareEvent::Back => close(tab, cx),
        BranchCompareEvent::Cancel => {
            tab.git_view
                .update(cx, |view, _| view.cancel_branch_compare());
        }
        BranchCompareEvent::Compare {
            request_id,
            base,
            target,
            mode,
        } => {
            tab.git_view.update(cx, |view, _| {
                view.branch_compare(
                    *request_id,
                    base.clone(),
                    target.clone(),
                    *mode,
                );
            });
        }
    })
    .detach();
}

/// Handle compare-specific Git events while allowing normal snapshots through.
pub(super) fn handle_git_event(
    tab: &mut RepoTab,
    event: &GitUiEvent,
    cx: &mut Context<RepoTab>,
) -> bool {
    match event {
        GitUiEvent::StatusChanged { branch, .. } => {
            tab.compare.update(cx, |view, cx| {
                view.set_current_branch(branch.clone(), cx);
            });
            false
        }
        GitUiEvent::RefsChanged(refs) => {
            tab.compare.update(cx, |view, cx| {
                view.set_refs(refs.clone(), cx);
            });
            false
        }
        GitUiEvent::BranchCompareFiles { request_id, files } => {
            tab.compare.update(cx, |view, cx| {
                view.set_files(*request_id, files.clone(), cx);
            });
            true
        }
        GitUiEvent::BranchCompareFileDiff {
            request_id,
            file,
            patch,
            old_source,
            new_source,
        } => {
            tab.compare.update(cx, |view, cx| {
                view.set_file_diff(
                    *request_id,
                    file.clone(),
                    patch.clone(),
                    old_source.clone(),
                    new_source.clone(),
                    cx,
                );
            });
            true
        }
        GitUiEvent::BranchCompareError {
            request_id,
            file,
            detail,
        } => {
            tab.compare.update(cx, |view, cx| {
                view.set_error(*request_id, file.clone(), detail.clone(), cx);
            });
            true
        }
        GitUiEvent::BranchCompareFinished { request_id } => {
            tab.compare.update(cx, |view, cx| {
                view.finish(*request_id, cx);
            });
            true
        }
        _ => false,
    }
}

pub(super) fn open(tab: &mut RepoTab, cx: &mut Context<RepoTab>) {
    tab.compare.update(cx, |view, cx| view.open(cx));
    cx.notify();
}

fn close(tab: &mut RepoTab, cx: &mut Context<RepoTab>) {
    tab.git_view
        .update(cx, |view, _| view.cancel_branch_compare());
    tab.compare.update(cx, |view, cx| view.close(cx));
    cx.notify();
}

pub(super) fn set_locale(
    tab: &mut RepoTab,
    locale: Locale,
    cx: &mut Context<RepoTab>,
) {
    tab.compare
        .update(cx, |view, cx| view.set_locale(locale, cx));
}

pub(super) fn set_diff_layout(
    tab: &mut RepoTab,
    diff_layout: crate::git::diff_view::DiffLayoutMode,
    cx: &mut Context<RepoTab>,
) {
    tab.compare.update(cx, |view, cx| {
        view.set_diff_layout(diff_layout, cx);
    });
}

pub(super) fn render(
    tab: &RepoTab,
    window: &mut Window,
    cx: &mut Context<RepoTab>,
) -> AnyElement {
    if tab.compare.read(cx).is_open() {
        tab.compare.clone().into_any_element()
    } else {
        tab.main_content(window, cx).into_any_element()
    }
}
