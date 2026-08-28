//! Bottom commit panel: changed-file list and the native diff viewer.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, h_flex, theme::ThemeColor, v_flex,
};
use std::sync::Arc;

use crate::core::config::{MAX_FILE_LIST_RATIO, MIN_FILE_LIST_RATIO};
use crate::core::diff::{DiffDocument, FileChange, stat_blocks};
use crate::core::i18n::{self, Locale};
use crate::git::diff_view::{self, DiffLayoutMode, DiffViewCache};
use crate::git::{lucide, shared};

#[path = "bottom_panel/working_tree.rs"]
mod working_tree;

fn bottom_empty_state(
    id: &'static str,
    colors: &ThemeColor,
    icon: AnyElement,
    hint: String,
) -> Stateful<Div> {
    v_flex()
        .id(id)
        .size_full()
        .items_center()
        .justify_center()
        .gap_1()
        .child(
            div()
                .size(px(24.))
                .text_color(colors.muted_foreground)
                .child(icon),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(colors.muted_foreground)
                .child(shared(hint)),
        )
}

/// BottomPanel → Workspace event.
#[derive(Clone, Debug)]
pub enum BottomPanelEvent {
    /// Select a file and load its commit diff through the Git worker.
    ShowFileDiff {
        oid: String,
        merge_parent: Option<String>,
        file: FileChange,
    },
    /// Load every changed file when a commit has no explicit file selection.
    ShowAllFileDiffs {
        oid: String,
        merge_parent: Option<String>,
        files: Vec<FileChange>,
    },
    /// Report the global file-list width ratio to the owning repository tab.
    LayoutChanged { file_list_ratio: f32 },
}

#[derive(Clone, Debug)]
pub struct DiffFileListResize;

impl Render for DiffFileListResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

/// Bottom panel containing the changed-file list and commit diff.
pub struct BottomPanel {
    locale: Locale,
    commit: Option<(String, String, String)>,
    merge_parent: Option<String>,
    files: Vec<FileChange>,
    selected: Option<usize>,
    diff: Option<DiffDocument>,
    diff_cache: Option<Arc<DiffViewCache>>,
    all_diffs: Vec<AllDiffDocument>,
    show_all_files: bool,
    all_diff_loading: bool,
    diff_layout: DiffLayoutMode,
    diff_loading: bool,
    working_tree: Option<working_tree::WorkingTreeDiffState>,
    content_width: f32,
    file_list_ratio: f32,
}

struct AllDiffDocument {
    file: FileChange,
    document: DiffDocument,
    cache: Arc<DiffViewCache>,
}

impl EventEmitter<BottomPanelEvent> for BottomPanel {}

impl BottomPanel {
    pub fn new(
        locale: Locale,
        diff_layout: DiffLayoutMode,
        file_list_ratio: f32,
    ) -> Self {
        Self {
            locale,
            commit: None,
            merge_parent: None,
            files: Vec::new(),
            selected: None,
            diff: None,
            diff_cache: None,
            all_diffs: Vec::new(),
            show_all_files: false,
            all_diff_loading: false,
            diff_layout,
            diff_loading: false,
            working_tree: None,
            content_width: f32::INFINITY,
            file_list_ratio: file_list_ratio
                .clamp(MIN_FILE_LIST_RATIO, MAX_FILE_LIST_RATIO),
        }
    }

    /// Apply the persisted diff layout chosen in the settings overlay.
    pub fn set_diff_layout(
        &mut self,
        diff_layout: DiffLayoutMode,
        cx: &mut Context<Self>,
    ) {
        if self.diff_layout != diff_layout {
            self.diff_layout = diff_layout;
            cx.notify();
        }
    }

    pub fn set_file_list_ratio(
        &mut self,
        file_list_ratio: f32,
        cx: &mut Context<Self>,
    ) {
        let file_list_ratio =
            file_list_ratio.clamp(MIN_FILE_LIST_RATIO, MAX_FILE_LIST_RATIO);
        if (self.file_list_ratio - file_list_ratio).abs() > f32::EPSILON {
            self.file_list_ratio = file_list_ratio;
            cx.notify();
        }
    }

    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    pub fn set_commit(
        &mut self,
        oid: &str,
        short: &str,
        subject: &str,
        cx: &mut Context<Self>,
    ) {
        self.commit =
            Some((oid.to_string(), short.to_string(), subject.to_string()));
        self.merge_parent = None;
        self.files.clear();
        self.selected = None;
        self.diff = None;
        self.diff_cache = None;
        self.all_diffs.clear();
        self.show_all_files = false;
        self.all_diff_loading = false;
        self.diff_loading = false;
        self.working_tree = None;
        cx.notify();
    }

    pub fn set_files(
        &mut self,
        oid: &str,
        merge_parent: Option<String>,
        files: Vec<FileChange>,
        cx: &mut Context<Self>,
    ) {
        if self
            .commit
            .as_ref()
            .is_none_or(|(current, _, _)| current != oid)
        {
            return;
        }
        self.merge_parent = merge_parent.clone();
        self.files = files;
        self.selected = None;
        self.diff = None;
        self.diff_cache = None;
        self.all_diffs.clear();
        self.show_all_files = !self.files.is_empty();
        self.all_diff_loading = self.show_all_files;
        self.diff_loading = false;
        if self.show_all_files {
            cx.emit(BottomPanelEvent::ShowAllFileDiffs {
                oid: oid.to_string(),
                merge_parent,
                files: self.files.clone(),
            });
        }
        cx.notify();
    }

    pub fn set_diff(
        &mut self,
        oid: &str,
        file: &FileChange,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let file_identity = file.identity();
        if self.show_all_files {
            let current_oid_matches = self
                .commit
                .as_ref()
                .is_some_and(|(current, _, _)| current == oid);
            let belongs_to_commit = self
                .files
                .iter()
                .any(|candidate| candidate.identity() == file_identity);
            if !current_oid_matches
                || self.selected.is_some()
                || !belongs_to_commit
                || self
                    .all_diffs
                    .iter()
                    .any(|entry| entry.file.identity() == file_identity)
            {
                return;
            }
            let mut document = DiffDocument::from_patch(
                file.path.clone(),
                &patch,
                old_source,
                new_source,
            );
            document.binary |= file.is_binary();
            let source_key = format!(
                "{oid}:{}:{}",
                file.identity(),
                document.language.as_deref().unwrap_or("text")
            );
            let cache = Arc::new(DiffViewCache::build_for(
                source_key,
                &document,
                cx.theme().highlight_theme.clone(),
            ));
            self.all_diffs.push(AllDiffDocument {
                file: file.clone(),
                document,
                cache,
            });
            self.all_diff_loading = self.all_diffs.len() < self.files.len();
            cx.notify();
            return;
        }

        let selected_identity = self
            .selected
            .and_then(|index| self.files.get(index))
            .map(FileChange::identity);
        let stale = self
            .commit
            .as_ref()
            .is_none_or(|(current, _, _)| current != oid)
            || selected_identity.as_deref() != Some(file_identity.as_str());
        if stale {
            return;
        }
        let mut document = DiffDocument::from_patch(
            file.path.clone(),
            &patch,
            old_source,
            new_source,
        );
        document.binary |= file.is_binary();
        let source_key = format!(
            "{oid}:{}:{}",
            file.identity(),
            document.language.as_deref().unwrap_or("text")
        );
        self.diff_cache = Some(Arc::new(DiffViewCache::build_for(
            source_key,
            &document,
            cx.theme().highlight_theme.clone(),
        )));
        self.diff = Some(document);
        self.diff_loading = false;
        cx.notify();
    }

    fn select_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.selected == Some(index) {
            return;
        }
        let Some((oid, _, _)) = self.commit.clone() else {
            return;
        };
        let Some(file) = self.files.get(index) else {
            return;
        };
        self.selected = Some(index);
        self.diff = None;
        self.diff_cache = None;
        self.all_diffs.clear();
        self.show_all_files = false;
        self.all_diff_loading = false;
        self.diff_loading = true;
        cx.emit(BottomPanelEvent::ShowFileDiff {
            oid,
            merge_parent: self.merge_parent.clone(),
            file: file.clone(),
        });
        cx.notify();
    }

    pub(super) fn copy_diff(&self, cx: &mut Context<Self>) {
        let text = if let Some(document) = self.diff.as_ref() {
            document.copy_text()
        } else if let Some(document) = self
            .working_tree
            .as_ref()
            .and_then(working_tree::WorkingTreeDiffState::document)
        {
            document.copy_text()
        } else if self.show_all_files {
            self.all_diffs
                .iter()
                .map(|entry| {
                    let mut text = format!("diff -- {}\n", entry.document.path);
                    text.push_str(&entry.document.copy_text());
                    text
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            return;
        };
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        log::debug!(
            "[git_diff] copied current document: files={}, rows={}",
            if self.show_all_files {
                self.all_diffs.len()
            } else {
                1
            },
            self.diff
                .as_ref()
                .map(|document| document.rows.len())
                .unwrap_or_else(|| {
                    self.all_diffs
                        .iter()
                        .map(|entry| entry.document.rows.len())
                        .sum()
                })
        );
    }

    fn all_diff_view(
        &mut self,
        colors: &ThemeColor,
        cx: &Context<Self>,
        width: f32,
    ) -> AnyElement {
        if self.all_diffs.is_empty() {
            let body = if self.all_diff_loading {
                div()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .child(shared("…"))
                    .into_any_element()
            } else {
                bottom_empty_state(
                    "bottom-diff-all-empty",
                    colors,
                    Icon::new(IconName::File).into_any_element(),
                    i18n::text(self.locale, "diff-no-output"),
                )
                .into_any_element()
            };
            return div()
                .id("bottom-diff")
                .flex_1()
                .min_w_0()
                .h_full()
                .child(body)
                .into_any_element();
        }

        let theme = cx.theme().highlight_theme.clone();
        for entry in &mut self.all_diffs {
            if entry.cache.theme.as_ref() != theme.as_ref() {
                entry.cache = Arc::new(DiffViewCache::build_for(
                    entry.cache.source_key.clone(),
                    &entry.document,
                    theme.clone(),
                ));
            }
        }
        let layout = if width < 900. {
            DiffLayoutMode::Inline
        } else {
            self.diff_layout
        };
        let sections = self
            .all_diffs
            .iter()
            .map(|entry| diff_view::DiffViewSection {
                path: entry.document.path.clone(),
                cache: Arc::clone(&entry.cache),
            })
            .collect();
        let document_view = diff_view::render_documents(
            sections,
            layout,
            colors,
            &cx.theme().mono_font_family,
            shared(i18n::text(self.locale, "bottom-bin")),
            shared(i18n::text(self.locale, "diff-no-output")),
        );
        let copy_entity = cx.entity();
        let key_entity = cx.entity();
        let file_header = h_flex()
            .id("bottom-diff-all-file-header")
            .w_full()
            .h(px(22.))
            .flex_shrink_0()
            .px_2()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(11.))
            .text_color(colors.muted_foreground)
            .child(
                div()
                    .flex_1()
                    .child(shared(i18n::text(self.locale, "diff-all-files"))),
            )
            .when(self.merge_parent.is_some(), |header| {
                header.child(
                    div()
                        .mr_1()
                        .text_size(px(10.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "diff-merge-first-parent",
                        ))),
                )
            })
            .child(
                div()
                    .id("bottom-diff-all-copy")
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
        div()
            .id("bottom-diff")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .focusable()
            .on_key_down(move |event, _window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("c")
                    && event.keystroke.modifiers.secondary()
                {
                    key_entity.update(cx, |panel, cx| panel.copy_diff(cx));
                }
            })
            .child(file_header)
            .child(document_view)
            .into_any_element()
    }

    fn no_changes_message(&self) -> String {
        let key = if self.merge_parent.is_some() {
            "bottom-merge-empty"
        } else {
            "bottom-no-changes"
        };
        i18n::text(self.locale, key)
    }

    fn file_list(
        &self,
        colors: &ThemeColor,
        cx: &Context<Self>,
        width_ratio: f32,
    ) -> AnyElement {
        let mono = cx.theme().mono_font_family.clone();
        if self.files.is_empty() {
            return bottom_empty_state(
                "bottom-files-empty",
                colors,
                Icon::new(IconName::Info).into_any_element(),
                self.no_changes_message(),
            )
            .w(relative(width_ratio))
            .flex_shrink_0()
            .into_any_element();
        }

        let rows = self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let this = cx.entity();
                let selected = self.selected == Some(index);
                let stat = if file.is_binary() {
                    div()
                        .text_size(px(10.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(self.locale, "bottom-bin")))
                        .into_any_element()
                } else {
                    stat_bar(
                        colors,
                        file.added.unwrap_or(0),
                        file.deleted.unwrap_or(0),
                    )
                    .into_any_element()
                };
                h_flex()
                    .id(SharedString::from(format!("bottom-file-{index}")))
                    .w_full()
                    .h(px(22.))
                    .flex_shrink_0()
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .bg(if selected {
                        colors.list_active
                    } else {
                        colors.background
                    })
                    .hover(|this| {
                        if !selected {
                            this.bg(colors.list_hover)
                        } else {
                            this
                        }
                    })
                    .on_click(move |_event, _window, cx| {
                        this.update(cx, |panel, cx| {
                            panel.select_file(index, cx)
                        });
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(mono.clone())
                            .text_size(px(11.))
                            .text_color(colors.foreground)
                            .truncate()
                            .child(shared(file.path.clone())),
                    )
                    .child(stat)
            })
            .collect::<Vec<_>>();

        div()
            .id("bottom-files")
            .w(relative(width_ratio))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .py_1()
            .children(rows)
            .into_any_element()
    }

    fn diff_view(
        &mut self,
        colors: &ThemeColor,
        cx: &Context<Self>,
        width: f32,
    ) -> AnyElement {
        if self.show_all_files {
            return self.all_diff_view(colors, cx, width);
        }
        if self.files.is_empty() {
            return bottom_empty_state(
                "bottom-diff-no-changes",
                colors,
                Icon::new(IconName::File).into_any_element(),
                self.no_changes_message(),
            )
            .into_any_element();
        }
        let Some(document) = self.diff.as_ref() else {
            let body = if self.selected.is_some() && self.diff_loading {
                div()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .child(shared("…"))
                    .into_any_element()
            } else if self.selected.is_some() {
                bottom_empty_state(
                    "bottom-diff-empty",
                    colors,
                    Icon::new(IconName::File).into_any_element(),
                    i18n::text(self.locale, "diff-no-output"),
                )
                .into_any_element()
            } else {
                bottom_empty_state(
                    "bottom-diff-none",
                    colors,
                    Icon::new(IconName::File).into_any_element(),
                    i18n::text(self.locale, "bottom-no-file"),
                )
                .into_any_element()
            };
            return div()
                .id("bottom-diff")
                .flex_1()
                .min_w_0()
                .h_full()
                .child(body)
                .into_any_element();
        };

        let theme = cx.theme().highlight_theme.clone();
        if self
            .diff_cache
            .as_ref()
            .is_none_or(|cache| cache.theme.as_ref() != theme.as_ref())
        {
            let source_key = self
                .diff_cache
                .as_ref()
                .map(|cache| cache.source_key.clone())
                .unwrap_or_default();
            self.diff_cache = Some(Arc::new(DiffViewCache::build_for(
                source_key, document, theme,
            )));
        }
        if document.binary {
            return div()
                .id("bottom-diff")
                .flex_1()
                .min_w_0()
                .h_full()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(colors.muted_foreground)
                .child(shared(i18n::text(self.locale, "bottom-bin")))
                .into_any_element();
        }
        if document.rows.is_empty() {
            return bottom_empty_state(
                "bottom-diff-empty",
                colors,
                Icon::new(IconName::File).into_any_element(),
                i18n::text(self.locale, "diff-no-output"),
            )
            .into_any_element();
        }
        let Some(cache) = self.diff_cache.as_ref() else {
            return div().id("bottom-diff").flex_1().into_any_element();
        };
        let layout = if width < 900. {
            DiffLayoutMode::Inline
        } else {
            self.diff_layout
        };
        let document_view = diff_view::render_document(
            cache,
            layout,
            colors,
            &cx.theme().mono_font_family,
        );
        let file_header = h_flex()
            .id("bottom-diff-file-header")
            .w_full()
            .h(px(22.))
            .flex_shrink_0()
            .px_2()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(11.))
            .text_color(colors.muted_foreground)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child(shared(document.path.clone())),
            )
            .child({
                let copy_entity = cx.entity();
                div()
                    .id("bottom-diff-copy")
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
                    })
            });
        let this = cx.entity();
        div()
            .id("bottom-diff")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .focusable()
            .on_key_down(move |event, _window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("c")
                    && event.keystroke.modifiers.secondary()
                {
                    this.update(cx, |panel, cx| panel.copy_diff(cx));
                }
            })
            .child(file_header)
            .child(document_view)
            .into_any_element()
    }
}

impl Render for BottomPanel {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        if self.working_tree.is_some() {
            return self.working_tree_view(&colors, window, cx);
        }
        let Some((_, short, subject)) = &self.commit else {
            return bottom_empty_state(
                "bottom-no-commit",
                &colors,
                lucide("git-commit-horizontal").into_any_element(),
                i18n::text(self.locale, "bottom-no-commit"),
            )
            .into_any_element();
        };

        let (total_add, total_del) =
            self.files.iter().fold((0, 0), |(added, deleted), file| {
                (
                    added + file.added.unwrap_or(0),
                    deleted + file.deleted.unwrap_or(0),
                )
            });
        let window_width = f32::from(window.bounds().size.width);
        let panel_width = if self.content_width.is_finite() {
            self.content_width
        } else {
            window_width
        };
        let diff_width = panel_width * (1.0 - self.file_list_ratio);
        let header = h_flex()
            .id("bottom-header")
            .w_full()
            .h(px(24.))
            .flex_shrink_0()
            .px_2()
            .gap_2()
            .items_center()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .px_2()
                    .rounded_sm()
                    .bg(colors.input)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(11.))
                    .text_color(colors.accent)
                    .child(shared(short.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .truncate()
                    .child(shared(subject.clone())),
            )
            .child(stat_bar(&colors, total_add, total_del));

        let resize_entity = cx.entity();
        v_flex()
            .id("bottom-panel")
            .size_full()
            .bg(colors.background)
            .on_drag_move::<DiffFileListResize>(move |event, _window, cx| {
                let width = f32::from(event.bounds.size.width);
                if width <= 0.0 {
                    return;
                }
                let x = f32::from(event.event.position.x)
                    - f32::from(event.bounds.origin.x);
                resize_entity.update(cx, |panel, cx| {
                    let ratio = (x / width)
                        .clamp(MIN_FILE_LIST_RATIO, MAX_FILE_LIST_RATIO);
                    if (panel.file_list_ratio - ratio).abs() > f32::EPSILON {
                        panel.file_list_ratio = ratio;
                        cx.emit(BottomPanelEvent::LayoutChanged {
                            file_list_ratio: ratio,
                        });
                        cx.notify();
                    }
                });
            })
            .child(measure_width_canvas(cx.entity()))
            .child(header)
            .child(
                div()
                    .id("bottom-body")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(self.file_list(&colors, cx, self.file_list_ratio))
                    .child(
                        div()
                            .id("bottom-file-list-resize-handle")
                            .w(px(3.))
                            .flex_shrink_0()
                            .h_full()
                            .bg(colors.border)
                            .cursor_col_resize()
                            .hover(|this| this.bg(colors.drag_border))
                            .on_drag(DiffFileListResize, |value, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| value.clone())
                            }),
                    )
                    .child(self.diff_view(&colors, cx, diff_width)),
            )
            .into_any_element()
    }
}

fn measure_width_canvas(entity: Entity<BottomPanel>) -> impl IntoElement {
    canvas(
        move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
            let width = f32::from(bounds.size.width);
            if width > 0.0 && width.is_finite() {
                cx.defer(move |cx: &mut App| {
                    entity.update(cx, |panel, cx| {
                        if panel.content_width != width {
                            panel.content_width = width;
                            cx.notify();
                        }
                    });
                });
            }
        },
        |_bounds: Bounds<Pixels>,
         _state: (),
         _window: &mut Window,
         _cx: &mut App| {},
    )
    .w_full()
    .h(px(0.))
}

fn stat_bar(colors: &ThemeColor, added: usize, deleted: usize) -> Div {
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
        bar = bar.child(div().w(px(4.)).h(px(10.)).rounded(px(1.)).bg(color));
    }
    h_flex()
        .gap_1()
        .items_center()
        .flex_shrink_0()
        .child(
            div()
                .text_size(px(10.))
                .text_color(colors.green)
                .child(shared(format!("+{added}"))),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(colors.red)
                .child(shared(format!("-{deleted}"))),
        )
        .child(bar)
}
