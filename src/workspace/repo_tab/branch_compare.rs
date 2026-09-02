//! Repository-tab coordination for the dedicated revision comparison view.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{Root, TitleBar};

use crate::core::i18n::Locale;
use crate::git::GitUiEvent;
use crate::git::branch_compare::{
    BranchCompareEvent, BranchCompareView, BranchCompareWindow,
};

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
    window: &mut Window,
    cx: &mut Context<RepoTab>,
) {
    cx.subscribe_in(compare, window, |tab, _event, event, _window, cx| {
        match event {
            BranchCompareEvent::Cancel => {
                tab.git_view
                    .update(cx, |view, _| view.cancel_branch_compare());
            }
            BranchCompareEvent::Compare {
                request_id,
                base,
                target,
            } => {
                tab.git_view.update(cx, |view, _| {
                    view.branch_compare(
                        *request_id,
                        base.clone(),
                        target.clone(),
                    );
                });
            }
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
    if let Some(handle) = tab.compare_window {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        tab.compare_window = None;
        tab.compare_window_closed = None;
    }

    let compare = tab.compare.clone();
    let window_size = size(px(1280.), px(820.));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(window_size, cx)),
        is_resizable: true,
        kind: WindowKind::Normal,
        window_decorations: Some(WindowDecorations::Client),
        window_min_size: Some(size(px(900.), px(560.))),
        ..TitleBar::window_options()
    };
    log::info!("[git_compare] opening standalone comparison window");
    let compare_for_window = compare.clone();
    match cx.open_window(options, move |window, cx| {
        compare_for_window.update(cx, |view, cx| {
            view.attach_window(window, cx);
            view.open(cx);
        });
        let compare_window =
            cx.new(|_| BranchCompareWindow::new(compare_for_window.clone()));
        let root = cx.new(|cx| Root::new(compare_window, window, cx));
        window.activate_window();
        root
    }) {
        Ok(handle) => {
            let window_handle: AnyWindowHandle = handle.into();
            let window_id = window_handle.window_id();
            let weak_tab = cx.entity().downgrade();
            tab.compare_window = Some(window_handle);
            log::info!(
                "[git_compare] standalone comparison window created: id={}",
                window_id.as_u64()
            );
            tab.compare_window_closed =
                Some(cx.on_window_closed(move |cx, closed_window_id| {
                    if closed_window_id != window_id {
                        return;
                    }
                    log::info!(
                        "[git_compare] standalone comparison window closed: id={}",
                        window_id.as_u64()
                    );
                    let Some(tab) = weak_tab.upgrade() else {
                        return;
                    };
                    let _ = tab.update(cx, |tab, cx| {
                        if tab.compare_window.is_none_or(|handle| {
                            handle.window_id() != window_id
                        }) {
                            return;
                        }
                        tab.compare_window = None;
                        tab.compare_window_closed = None;
                        tab.git_view.update(cx, |view, _| {
                            view.cancel_branch_compare();
                        });
                        tab.compare.update(cx, |view, cx| view.close(cx));
                        cx.notify();
                    });
                }));
            cx.notify();
        }
        Err(error) => {
            log::error!(
                "[git_compare] failed to open standalone comparison window: {error}"
            );
            compare.update(cx, |view, cx| view.close(cx));
        }
    }
}

pub(super) fn close(tab: &mut RepoTab, cx: &mut Context<RepoTab>) {
    tab.compare_window_closed = None;
    if let Some(handle) = tab.compare_window.take() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
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
    tab.main_content(window, cx).into_any_element()
}
