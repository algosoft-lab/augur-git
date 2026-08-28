//! Working-tree diff state and rendering for the bottom panel.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, h_flex, theme::ThemeColor, v_flex,
};

use crate::core::diff::DiffDocument;
use crate::core::git::{FileStatus, WorkingTreeDiffKind};
use crate::core::i18n::{self, Locale};
use crate::git::diff_view::{self, DiffLayoutMode, DiffViewCache};
use crate::git::shared;

use super::{BottomPanel, bottom_empty_state};

pub(super) struct WorkingTreeDiffState {
    request_id: u64,
    kind: WorkingTreeDiffKind,
    file: FileStatus,
    document: Option<DiffDocument>,
    cache: Option<Arc<DiffViewCache>>,
    loading: bool,
    error: Option<String>,
}

impl WorkingTreeDiffState {
    fn new(
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: FileStatus,
    ) -> Self {
        Self {
            request_id,
            kind,
            file,
            document: None,
            cache: None,
            loading: true,
            error: None,
        }
    }

    pub(super) fn document(&self) -> Option<&DiffDocument> {
        self.document.as_ref()
    }

    fn matches(
        &self,
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: &FileStatus,
    ) -> bool {
        self.request_id == request_id
            && self.kind == kind
            && self.file.path == file.path
            && self.file.old_path == file.old_path
    }

    fn is_staged(&self) -> bool {
        matches!(self.kind, WorkingTreeDiffKind::Staged)
    }

    fn header_label(&self, locale: Locale) -> String {
        let key = if self.is_staged() {
            "diff-working-tree-staged"
        } else {
            "diff-working-tree-changes"
        };
        i18n::text(locale, key)
    }
}

impl BottomPanel {
    /// Switch the bottom panel to a selected working-tree file.
    pub fn set_working_tree_file(
        &mut self,
        request_id: u64,
        staged: bool,
        file: FileStatus,
        cx: &mut Context<Self>,
    ) {
        let kind = if staged {
            WorkingTreeDiffKind::Staged
        } else {
            WorkingTreeDiffKind::Unstaged
        };
        self.commit = None;
        self.merge_parent = None;
        self.files.clear();
        self.selected = None;
        self.diff = None;
        self.diff_cache = None;
        self.all_diffs.clear();
        self.show_all_files = false;
        self.all_diff_loading = false;
        self.diff_loading = false;
        self.working_tree =
            Some(WorkingTreeDiffState::new(request_id, kind, file));
        cx.notify();
    }

    /// Apply a successful working-tree diff if it still matches the selection.
    pub fn set_working_tree_diff(
        &mut self,
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: &FileStatus,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.working_tree.as_ref() else {
            return;
        };
        if !state.matches(request_id, kind, file) {
            return;
        }
        let document = DiffDocument::from_patch(
            file.path.clone(),
            &patch,
            old_source,
            new_source,
        );
        let source_key = format!(
            "working-tree:{request_id}:{}:{}",
            file.path,
            document.language.as_deref().unwrap_or("text")
        );
        let cache = Arc::new(DiffViewCache::build_for(
            source_key,
            &document,
            cx.theme().highlight_theme.clone(),
        ));
        let Some(state) = self.working_tree.as_mut() else {
            return;
        };
        state.document = Some(document);
        state.cache = Some(cache);
        state.loading = false;
        state.error = None;
        cx.notify();
    }

    /// Show a non-fatal error for the current working-tree diff request.
    pub fn set_working_tree_error(
        &mut self,
        request_id: u64,
        kind: WorkingTreeDiffKind,
        file: &FileStatus,
        detail: String,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.working_tree.as_mut() else {
            return;
        };
        if !state.matches(request_id, kind, file) {
            return;
        }
        state.document = None;
        state.cache = None;
        state.loading = false;
        state.error = Some(detail);
        cx.notify();
    }

    /// Drop the selected working-tree diff when a status refresh removes it.
    pub fn sync_working_tree_files(
        &mut self,
        files: &[FileStatus],
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.working_tree.as_ref() else {
            return;
        };
        let staged = state.is_staged();
        let selected_path = state.file.path.clone();
        let selected_old_path = state.file.old_path.clone();
        let still_present = files.iter().any(|file| {
            let in_group = if staged {
                file.has_staged_changes()
            } else {
                file.has_worktree_changes()
            };
            in_group
                && file.path == selected_path
                && file.old_path == selected_old_path
        });
        if !still_present {
            self.working_tree = None;
            cx.notify();
        }
    }

    pub(super) fn working_tree_view(
        &mut self,
        colors: &ThemeColor,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let locale = self.locale;
        let diff_layout = self.diff_layout;
        let content_width = self.content_width;
        let mono_font = cx.theme().mono_font_family.clone();
        let Some(state) = self.working_tree.as_mut() else {
            return bottom_empty_state(
                "bottom-no-commit",
                colors,
                Icon::new(IconName::Info).into_any_element(),
                i18n::text(locale, "bottom-no-commit"),
            )
            .into_any_element();
        };

        let width = if content_width.is_finite() {
            content_width
        } else {
            f32::from(window.bounds().size.width)
        };
        let body = if state.loading {
            bottom_empty_state(
                "bottom-working-diff-loading",
                colors,
                Icon::new(IconName::Info).into_any_element(),
                i18n::text(locale, "diff-working-tree-loading"),
            )
            .into_any_element()
        } else if let Some(error) = state.error.as_deref() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .child(div().text_size(px(11.)).text_color(colors.red).child(
                    shared(i18n::text(locale, "diff-working-tree-error")),
                ))
                .child(
                    div()
                        .max_w(px(600.))
                        .text_size(px(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared(error.to_string())),
                )
                .into_any_element()
        } else {
            let Some(document) = state.document.as_ref() else {
                return bottom_empty_state(
                    "bottom-working-diff-empty",
                    colors,
                    Icon::new(IconName::File).into_any_element(),
                    i18n::text(locale, "diff-no-output"),
                )
                .into_any_element();
            };
            let theme = cx.theme().highlight_theme.clone();
            if state
                .cache
                .as_ref()
                .is_none_or(|cache| cache.theme.as_ref() != theme.as_ref())
            {
                let source_key = state
                    .cache
                    .as_ref()
                    .map(|cache| cache.source_key.clone())
                    .unwrap_or_default();
                state.cache = Some(Arc::new(DiffViewCache::build_for(
                    source_key, document, theme,
                )));
            }
            if document.binary {
                div()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .child(shared(i18n::text(locale, "bottom-bin")))
                    .into_any_element()
            } else if document.rows.is_empty() {
                bottom_empty_state(
                    "bottom-working-diff-empty",
                    colors,
                    Icon::new(IconName::File).into_any_element(),
                    i18n::text(locale, "diff-no-output"),
                )
                .into_any_element()
            } else if let Some(cache) = state.cache.as_ref() {
                let layout = if width < 900. {
                    DiffLayoutMode::Inline
                } else {
                    diff_layout
                };
                diff_view::render_document(cache, layout, colors, &mono_font)
            } else {
                bottom_empty_state(
                    "bottom-working-diff-empty",
                    colors,
                    Icon::new(IconName::File).into_any_element(),
                    i18n::text(locale, "diff-no-output"),
                )
                .into_any_element()
            }
        };

        let copy_entity = cx.entity();
        let header = h_flex()
            .id("bottom-working-diff-header")
            .w_full()
            .h(px(22.))
            .flex_shrink_0()
            .px_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(11.))
            .text_color(colors.muted_foreground)
            .child(
                div()
                    .text_color(colors.accent)
                    .child(shared(state.header_label(locale))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(shared(state.file.path.clone())),
            )
            .child(
                div()
                    .id("bottom-working-diff-copy")
                    .w(px(22.))
                    .h(px(20.))
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_color(colors.muted_foreground)
                    .hover(|this| this.bg(colors.list_hover))
                    .child(Icon::new(IconName::Copy).size(px(13.)))
                    .on_click(move |_event, _window, cx| {
                        copy_entity.update(cx, |panel, cx| panel.copy_diff(cx));
                    }),
            );
        let key_entity = cx.entity();
        v_flex()
            .id("bottom-working-diff")
            .size_full()
            .bg(colors.background)
            .focusable()
            .on_key_down(move |event, _window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("c")
                    && event.keystroke.modifiers.secondary()
                {
                    key_entity.update(cx, |panel, cx| panel.copy_diff(cx));
                }
            })
            .child(super::measure_width_canvas(cx.entity()))
            .child(header)
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkingTreeDiffKind, WorkingTreeDiffState};
    use crate::core::git::FileStatus;

    fn status(index: char, worktree: char, path: &str) -> FileStatus {
        FileStatus {
            index,
            worktree,
            path: path.to_string(),
            old_path: None,
        }
    }

    #[test]
    fn working_tree_diff_state_rejects_stale_requests() {
        let selected = status('M', 'M', "src/main.rs");
        let state = WorkingTreeDiffState::new(
            7,
            WorkingTreeDiffKind::Unstaged,
            selected.clone(),
        );
        assert!(state.matches(7, WorkingTreeDiffKind::Unstaged, &selected));
        assert!(!state.matches(8, WorkingTreeDiffKind::Unstaged, &selected));
        assert!(!state.matches(7, WorkingTreeDiffKind::Staged, &selected));
        assert!(!state.matches(
            7,
            WorkingTreeDiffKind::Unstaged,
            &status('M', 'M', "src/other.rs")
        ));
    }
}
