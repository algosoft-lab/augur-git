//! Dedicated revision comparison view.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::spinner::Spinner;
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, TitleBar, h_flex, v_flex,
};

use crate::core::diff::{DiffDocument, FileChange};
use crate::core::git::{CompareRevision, CompareRevisionKind, RefsInfo};
use crate::core::graph::LogRow;
use crate::core::i18n::{self, Locale};

use super::diff_view::{self, DiffLayoutMode, DiffViewCache};
use super::revision_picker::{
    RevisionPicker, RevisionPickerEvent, RevisionPickerOption,
};
use super::{lucide, shared};

#[path = "branch_compare_helpers.rs"]
mod helpers;
use helpers::{
    choose_selection, choose_target, compare_field, compare_field_action,
    empty_state, first_line, format_commit_revision_label,
    format_revision_label, stat_bar, stat_summary,
};

/// Events emitted by the branch comparison view.
#[derive(Clone, Debug)]
pub enum BranchCompareEvent {
    Cancel,
    Compare {
        request_id: u64,
        base: CompareRevision,
        target: CompareRevision,
    },
}

struct CompareDocument {
    document: DiffDocument,
    cache: Arc<DiffViewCache>,
}

/// Full-screen read-only revision comparison state and renderer.
pub struct BranchCompareView {
    locale: Locale,
    diff_layout: DiffLayoutMode,
    refs: Vec<CompareRevision>,
    commits: Vec<LogRow>,
    current_branch: String,
    revision: u64,
    synced_revision: u64,
    base_picker: Entity<RevisionPicker>,
    target_picker: Entity<RevisionPicker>,
    request_id: u64,
    loading: bool,
    finished: bool,
    files: Vec<FileChange>,
    selected: Option<usize>,
    documents: HashMap<String, CompareDocument>,
    file_errors: HashMap<String, String>,
    request_error: Option<String>,
    show_all: bool,
}

impl EventEmitter<BranchCompareEvent> for BranchCompareView {}

impl BranchCompareView {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        locale: Locale,
        diff_layout: DiffLayoutMode,
    ) -> Self {
        let base_picker =
            cx.new(|cx| RevisionPicker::new("base", window, cx, locale));
        let target_picker =
            cx.new(|cx| RevisionPicker::new("target", window, cx, locale));

        let base_entity = base_picker.clone();
        cx.subscribe(&base_entity, |view, _, _: &RevisionPickerEvent, cx| {
            view.revision = view.revision.wrapping_add(1).max(1);
            view.invalidate_compare(cx);
            cx.notify();
        })
        .detach();
        let target_entity = target_picker.clone();
        cx.subscribe(&target_entity, |view, _, _: &RevisionPickerEvent, cx| {
            view.revision = view.revision.wrapping_add(1).max(1);
            view.invalidate_compare(cx);
            cx.notify();
        })
        .detach();

        Self {
            locale,
            diff_layout,
            refs: Vec::new(),
            commits: Vec::new(),
            current_branch: String::new(),
            revision: 1,
            synced_revision: 0,
            base_picker,
            target_picker,
            request_id: 0,
            loading: false,
            finished: false,
            files: Vec::new(),
            selected: None,
            documents: HashMap::new(),
            file_errors: HashMap::new(),
            request_error: None,
            show_all: true,
        }
    }

    /// Rebind window-scoped picker subscriptions when the view moves to a
    /// standalone native window.
    pub fn attach_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.base_picker.update(cx, |picker, cx| {
            picker.attach_window(window, cx);
        });
        self.target_picker.update(cx, |picker, cx| {
            picker.attach_window(window, cx);
        });
    }

    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        if self.locale != locale {
            self.locale = locale;
            self.revision = self.revision.wrapping_add(1).max(1);
            cx.notify();
        }
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.show_all = true;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.invalidate_request_id();
        self.loading = false;
        cx.notify();
    }

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

    pub fn set_refs(&mut self, refs: RefsInfo, cx: &mut Context<Self>) {
        if self.refs != refs.comparison_revisions {
            self.refs = refs.comparison_revisions;
            self.revision = self.revision.wrapping_add(1).max(1);
            cx.notify();
        }
    }

    /// Update the commit options shown by the revision selectors.
    pub fn set_log_rows(&mut self, rows: Vec<LogRow>, cx: &mut Context<Self>) {
        self.commits = rows;
        self.revision = self.revision.wrapping_add(1).max(1);
        cx.notify();
    }

    pub fn set_current_branch(
        &mut self,
        branch: String,
        cx: &mut Context<Self>,
    ) {
        if self.current_branch != branch {
            self.current_branch = branch;
            self.revision = self.revision.wrapping_add(1).max(1);
            cx.notify();
        }
    }

    pub fn set_files(
        &mut self,
        request_id: u64,
        files: Vec<FileChange>,
        cx: &mut Context<Self>,
    ) {
        if request_id != self.request_id {
            log::warn!(
                "[git_compare] UI dropped metadata: event_request_id={}, current_request_id={}",
                request_id,
                self.request_id
            );
            return;
        }
        log::info!(
            "[git_compare] UI accepted metadata: request_id={}, files={}",
            request_id,
            files.len()
        );
        self.files = files;
        self.documents.clear();
        self.file_errors.clear();
        self.request_error = None;
        self.finished = false;
        self.show_all = true;
        self.selected = None;
        cx.notify();
    }

    pub fn set_file_diff(
        &mut self,
        request_id: u64,
        file: FileChange,
        patch: String,
        old_source: Option<String>,
        new_source: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if request_id != self.request_id {
            log::warn!(
                "[git_compare] UI dropped file event: event_request_id={}, current_request_id={}, path={}",
                request_id,
                self.request_id,
                file.path
            );
            return;
        }
        if !self
            .files
            .iter()
            .any(|candidate| candidate.identity() == file.identity())
        {
            log::warn!(
                "[git_compare] UI dropped unknown file event: request_id={}, path={}",
                request_id,
                file.path
            );
            return;
        }
        let identity = file.identity();
        let old_source_bytes = old_source.as_ref().map_or(0, String::len);
        let new_source_bytes = new_source.as_ref().map_or(0, String::len);
        let mut document = DiffDocument::from_patch(
            file.path.clone(),
            &patch,
            old_source,
            new_source,
        );
        document.binary |= file.is_binary();
        let row_count = document.rows.len();
        let document_binary = document.binary;
        let source_key = format!(
            "compare:{request_id}:{}:{}",
            identity,
            document.language.as_deref().unwrap_or("text")
        );
        let cache = Arc::new(DiffViewCache::build_for(
            source_key,
            &document,
            cx.theme().highlight_theme.clone(),
        ));
        self.documents
            .insert(identity, CompareDocument { document, cache });
        log::info!(
            "[git_compare] UI accepted file: request_id={}, path={}, patch_bytes={}, rows={}, old_source_bytes={}, new_source_bytes={}, binary={}",
            request_id,
            file.path,
            patch.len(),
            row_count,
            old_source_bytes,
            new_source_bytes,
            document_binary
        );
        cx.notify();
    }

    pub fn set_error(
        &mut self,
        request_id: u64,
        file: Option<FileChange>,
        detail: String,
        cx: &mut Context<Self>,
    ) {
        if request_id != self.request_id {
            return;
        }
        if let Some(file) = file {
            self.file_errors.insert(file.identity(), detail);
        } else {
            self.request_error = Some(detail);
            self.finished = true;
            self.loading = false;
        }
        cx.notify();
    }

    pub fn finish(&mut self, request_id: u64, cx: &mut Context<Self>) {
        if request_id == self.request_id {
            self.loading = false;
            self.finished = true;
            log::info!(
                "[git_compare] UI finished: request_id={}, files={}, documents={}, file_errors={}",
                request_id,
                self.files.len(),
                self.documents.len(),
                self.file_errors.len()
            );
            cx.notify();
        }
    }

    pub fn start_compare(&mut self, cx: &mut Context<Self>) {
        let (Some(base), Some(target)) = (
            self.base_picker.read(cx).candidate().revision(),
            self.target_picker.read(cx).candidate().revision(),
        ) else {
            return;
        };
        if base.full_name == target.full_name {
            return;
        }
        self.request_id = self.request_id.wrapping_add(1).max(1);
        self.loading = true;
        self.finished = false;
        self.files.clear();
        self.documents.clear();
        self.file_errors.clear();
        self.request_error = None;
        self.show_all = true;
        self.selected = None;
        cx.emit(BranchCompareEvent::Compare {
            request_id: self.request_id,
            base,
            target,
        });
        cx.notify();
    }

    fn invalidate_request_id(&mut self) {
        self.request_id = self.request_id.wrapping_add(1).max(1);
    }

    fn invalidate_compare(&mut self, cx: &mut Context<Self>) {
        self.invalidate_request_id();
        self.loading = false;
        self.request_error = None;
        self.files.clear();
        self.documents.clear();
        self.file_errors.clear();
        self.finished = false;
        self.selected = None;
        cx.emit(BranchCompareEvent::Cancel);
    }

    fn swap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let base = self.base_picker.read(cx).value();
        let target = self.target_picker.read(cx).value();
        self.revision = self.revision.wrapping_add(1).max(1);
        self.invalidate_compare(cx);
        self.base_picker.update(cx, |picker, cx| {
            picker.set_value(target.clone(), window, cx);
        });
        self.target_picker.update(cx, |picker, cx| {
            picker.set_value(base, window, cx);
        });
        cx.notify();
    }

    fn select_file(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        self.show_all = index.is_none();
        self.selected = index;
        cx.notify();
    }

    fn copy_diff(&self, cx: &mut Context<Self>) {
        let text = if self.show_all {
            self.files
                .iter()
                .filter_map(|file| self.documents.get(&file.identity()))
                .map(|entry| {
                    let mut text = format!("diff -- {}\n", entry.document.path);
                    text.push_str(&entry.document.copy_text());
                    text
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            self.selected_identity()
                .and_then(|identity| self.documents.get(&identity))
                .map(|entry| entry.document.copy_text())
                .unwrap_or_default()
        };
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn sync_selectors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.revision == self.synced_revision {
            return;
        }
        let mut options = Vec::with_capacity(
            self.refs.len().saturating_add(self.commits.len()),
        );
        options.extend(self.refs.iter().cloned().map(|value| {
            RevisionPickerOption::new(
                value.clone(),
                format_revision_label(self.locale, &value, None),
            )
        }));
        let mut seen_commits = HashMap::new();
        for row in &self.commits {
            if seen_commits.insert(row.oid.clone(), ()).is_none() {
                let value = CompareRevision {
                    name: row.short.clone(),
                    full_name: row.oid.clone(),
                    kind: CompareRevisionKind::Commit,
                };
                options.push(RevisionPickerOption::new(
                    value.clone(),
                    format_commit_revision_label(self.locale, &value, row),
                ));
            }
        }
        let values = options
            .iter()
            .map(|option| option.value.clone())
            .collect::<Vec<_>>();
        let current_base = self.base_picker.read(cx).selected();
        let current_target = self.target_picker.read(cx).selected();
        let base =
            choose_selection(&current_base, &values, &self.current_branch);
        let target = choose_target(&current_target, &values, base.as_ref());
        let picker_options = options;
        self.base_picker.update(cx, |picker, cx| {
            picker.set_locale(self.locale, window, cx);
            picker.set_options(
                picker_options.clone(),
                base.clone(),
                window,
                cx,
            );
        });
        self.target_picker.update(cx, |picker, cx| {
            picker.set_locale(self.locale, window, cx);
            picker.set_options(picker_options, target.clone(), window, cx);
        });
        self.synced_revision = self.revision;
    }

    fn can_compare(&self, cx: &App) -> bool {
        self.base_picker
            .read(cx)
            .candidate()
            .revision()
            .zip(self.target_picker.read(cx).candidate().revision())
            .is_some_and(|(base, target)| base.full_name != target.full_name)
    }

    fn header(
        &self,
        colors: &gpui_component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> AnyElement {
        let this = cx.entity();
        let compare_enabled = self.can_compare(cx);
        let base_label = i18n::text(self.locale, "branch-compare-base");
        let target_label = i18n::text(self.locale, "branch-compare-target");
        let run_label = if self.finished {
            i18n::text(self.locale, "branch-compare-refresh")
        } else {
            i18n::text(self.locale, "branch-compare-run")
        };
        let (total_added, total_deleted) =
            self.files.iter().fold((0, 0), |(added, deleted), file| {
                (
                    added + file.added.unwrap_or(0),
                    deleted + file.deleted.unwrap_or(0),
                )
            });

        v_flex()
            .id("branch-compare-header")
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .p_3()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .when(self.loading || !self.files.is_empty(), |header| {
                header.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1())
                        .when(self.loading, |row| {
                            row.child(
                                Spinner::new()
                                    .with_size(px(14.))
                                    .color(colors.blue),
                            )
                            .child(shared(
                                format!(
                                    "{} / {}",
                                    self.documents.len(),
                                    self.files.len()
                                ),
                            ))
                        })
                        .when(!self.files.is_empty(), |row| {
                            row.child(stat_summary(
                                colors,
                                total_added,
                                total_deleted,
                            ))
                        }),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_2()
                    .child(compare_field(
                        &base_label,
                        self.base_picker.clone(),
                        colors.muted_foreground,
                    ))
                    .child(compare_field_action(
                        Button::new("branch-compare-swap")
                            .icon(lucide("refresh-cw"))
                            .ghost()
                            .compact()
                            .flex_shrink_0()
                            .disabled(
                                self.base_picker
                                    .read(cx)
                                    .candidate()
                                    .revision()
                                    .is_none()
                                    || self
                                        .target_picker
                                        .read(cx)
                                        .candidate()
                                        .revision()
                                        .is_none(),
                            )
                            .on_click({
                                let this = this.clone();
                                move |_event, window, cx| {
                                    this.update(cx, |view, cx| {
                                        view.swap(window, cx)
                                    });
                                }
                            }),
                    ))
                    .child(compare_field(
                        &target_label,
                        self.target_picker.clone(),
                        colors.muted_foreground,
                    ))
                    .child(compare_field_action(
                        Button::new("branch-compare-run")
                            .label(run_label)
                            .primary()
                            .compact()
                            .flex_shrink_0()
                            .disabled(!compare_enabled)
                            .on_click(move |_event, _window, cx| {
                                this.update(cx, |view, cx| {
                                    view.start_compare(cx)
                                });
                            }),
                    )),
            )
            .when_some(self.request_error.clone(), |header, error| {
                header.child(
                    div()
                        .w_full()
                        .text_size(px(11.))
                        .text_color(colors.red)
                        .child(shared(error)),
                )
            })
            .into_any_element()
    }

    fn file_list(
        &self,
        colors: &gpui_component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let this = cx.entity();
        let all_selected = self.show_all;
        let all_row = h_flex()
            .id("branch-compare-all-files")
            .w_full()
            .h(px(26.))
            .items_center()
            .px_2()
            .gap_2()
            .rounded_sm()
            .bg(if all_selected {
                colors.list_active
            } else {
                colors.background
            })
            .hover(|row| row.bg(colors.list_hover))
            .on_click({
                let this = this.clone();
                move |_event, _window, cx| {
                    this.update(cx, |view, cx| view.select_file(None, cx));
                }
            })
            .child(lucide("file"))
            .child(
                div()
                    .flex_1()
                    .text_size(px(11.))
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(
                        self.locale,
                        "branch-compare-all-files",
                    ))),
            )
            .child(shared(self.files.len().to_string()));
        let rows = self.files.iter().enumerate().map(|(index, file)| {
            let selected = !self.show_all
                && self.selected_identity().as_deref()
                    == Some(file.identity().as_str());
            let identity = file.identity();
            let error = self.file_errors.get(&identity).cloned();
            let has_error = error.is_some();
            let this = this.clone();
            h_flex()
                .id(SharedString::from(format!("branch-compare-file-{index}")))
                .w_full()
                .h(px(24.))
                .items_center()
                .px_2()
                .gap_2()
                .rounded_sm()
                .bg(if selected {
                    colors.list_active
                } else {
                    colors.background
                })
                .hover(|row| row.bg(colors.list_hover))
                .on_click(move |_event, _window, cx| {
                    this.update(cx, |view, cx| {
                        view.select_file(Some(index), cx)
                    });
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(11.))
                        .text_color(colors.foreground)
                        .truncate()
                        .child(shared(file.path.clone())),
                )
                .when_some(error, |row, error| {
                    row.child(
                        div()
                            .text_size(px(10.))
                            .text_color(colors.red)
                            .child(shared(first_line(&error).to_string())),
                    )
                })
                .when(!has_error, |row| {
                    row.child(stat_bar(
                        colors,
                        file.added.unwrap_or(0),
                        file.deleted.unwrap_or(0),
                    ))
                })
        });
        v_flex()
            .id("branch-compare-files")
            .w(px(300.))
            .h_full()
            .flex_shrink_0()
            .gap_1()
            .p_2()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(colors.border)
            .child(all_row)
            .children(rows)
    }

    fn selected_identity(&self) -> Option<String> {
        if self.show_all {
            return None;
        }
        self.selected
            .and_then(|index| self.files.get(index))
            .map(FileChange::identity)
    }

    fn diff_view(
        &mut self,
        colors: &gpui_component::theme::ThemeColor,
        layout: DiffLayoutMode,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().highlight_theme.clone();
        for entry in self.documents.values_mut() {
            if entry.cache.theme.as_ref() != theme.as_ref() {
                entry.cache = Arc::new(DiffViewCache::build_for(
                    entry.cache.source_key.clone(),
                    &entry.document,
                    theme.clone(),
                ));
            }
        }
        if self.request_error.is_some() {
            return empty_state(
                "branch-compare-error",
                colors,
                i18n::text(self.locale, "branch-compare-error"),
            )
            .into_any_element();
        }
        if self.files.is_empty() {
            let message = if self.loading {
                i18n::text(self.locale, "branch-compare-loading")
            } else if self.finished {
                i18n::text(self.locale, "branch-compare-no-changes")
            } else {
                i18n::text(self.locale, "branch-compare-select-hint")
            };
            return empty_state("branch-compare-empty", colors, message)
                .into_any_element();
        }
        if self.show_all {
            let sections = self
                .files
                .iter()
                .filter_map(|file| self.documents.get(&file.identity()))
                .map(|entry| diff_view::DiffViewSection {
                    path: entry.document.path.clone(),
                    cache: Arc::clone(&entry.cache),
                })
                .collect::<Vec<_>>();
            let total_rows = sections
                .iter()
                .map(|section| {
                    if layout == DiffLayoutMode::SideBySide {
                        section.cache.side_rows.len()
                    } else {
                        section.cache.inline_rows.len()
                    }
                })
                .sum::<usize>();
            log::info!(
                "[git_compare] render aggregate: request_id={}, layout={layout:?}, sections={}, documents={}, total_rows={}",
                self.request_id,
                sections.len(),
                self.documents.len(),
                total_rows
            );
            let body = if sections.is_empty() && self.loading {
                empty_state(
                    "branch-compare-loading-body",
                    colors,
                    i18n::text(self.locale, "branch-compare-loading"),
                )
                .into_any_element()
            } else {
                diff_view::render_documents(
                    sections,
                    layout,
                    colors,
                    &cx.theme().mono_font_family,
                    shared(i18n::text(self.locale, "bottom-bin")),
                    shared(i18n::text(self.locale, "diff-no-output")),
                )
            };
            let copy_entity = cx.entity();
            return v_flex()
                .id("branch-compare-diff")
                .size_full()
                .child(
                    h_flex()
                        .id("branch-compare-diff-header")
                        .w_full()
                        .h(px(26.))
                        .flex_shrink_0()
                        .items_center()
                        .px_2()
                        .gap_2()
                        .border_b_1()
                        .border_color(colors.border)
                        .child(shared(i18n::text(
                            self.locale,
                            "branch-compare-all-files",
                        )))
                        .when(self.loading, |row| {
                            row.child(shared(format!(
                                "{} / {}",
                                self.documents.len(),
                                self.files.len()
                            )))
                        })
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("branch-compare-copy")
                                .px_1()
                                .rounded_sm()
                                .hover(|button| button.bg(colors.list_hover))
                                .child(Icon::new(IconName::Copy).size(px(13.)))
                                .on_click(move |_event, _window, cx| {
                                    copy_entity.update(cx, |view, cx| {
                                        view.copy_diff(cx)
                                    });
                                }),
                        ),
                )
                .child(body)
                .into_any_element();
        }

        let Some(identity) = self.selected_identity() else {
            return empty_state(
                "branch-compare-no-file",
                colors,
                i18n::text(self.locale, "branch-compare-select-file"),
            )
            .into_any_element();
        };
        let Some(entry) = self.documents.get(&identity) else {
            let message =
                self.file_errors.get(&identity).cloned().unwrap_or_else(|| {
                    i18n::text(self.locale, "branch-compare-loading")
                });
            return empty_state("branch-compare-file-loading", colors, message)
                .into_any_element();
        };
        if entry.document.binary {
            return empty_state(
                "branch-compare-binary",
                colors,
                i18n::text(self.locale, "bottom-bin"),
            )
            .into_any_element();
        }
        let copy_entity = cx.entity();
        v_flex()
            .id("branch-compare-single-diff")
            .size_full()
            .child(
                h_flex()
                    .id("branch-compare-single-header")
                    .w_full()
                    .h(px(26.))
                    .flex_shrink_0()
                    .items_center()
                    .px_2()
                    .child(shared(entry.document.path.clone()))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("branch-compare-single-copy")
                            .px_1()
                            .child(Icon::new(IconName::Copy).size(px(13.)))
                            .on_click(move |_event, _window, cx| {
                                copy_entity
                                    .update(cx, |view, cx| view.copy_diff(cx));
                            }),
                    ),
            )
            .child(diff_view::render_document(
                &entry.cache,
                layout,
                colors,
                &cx.theme().mono_font_family,
            ))
            .into_any_element()
    }
}

/// Native-window root for a revision comparison.
pub struct BranchCompareWindow {
    compare: Entity<BranchCompareView>,
}

impl BranchCompareWindow {
    pub fn new(compare: Entity<BranchCompareView>) -> Self {
        Self { compare }
    }
}

impl Render for BranchCompareWindow {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let locale = self.compare.read(cx).locale;
        v_flex()
            .id("branch-compare-window")
            .size_full()
            .min_h_0()
            .bg(colors.background)
            .child(
                TitleBar::new().child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(
                            locale,
                            "branch-compare-title",
                        ))),
                ),
            )
            .child(div().flex_1().min_h_0().child(self.compare.clone()))
    }
}

impl Render for BranchCompareView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.sync_selectors(window, cx);
        let colors = cx.theme().colors.clone();
        let layout = if self.diff_layout == DiffLayoutMode::SideBySide
            && f32::from(window.bounds().size.width) < 900.0
        {
            DiffLayoutMode::Inline
        } else {
            self.diff_layout
        };
        h_flex()
            .id("branch-compare-view")
            .size_full()
            .min_h_0()
            .bg(colors.background)
            .child(
                v_flex()
                    .size_full()
                    .min_w_0()
                    .child(self.header(&colors, cx))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .child(self.file_list(&colors, cx))
                            .child(
                                div()
                                    .flex_1()
                                    // h_flex defaults to items_center, so the
                                    // diff pane needs an explicit cross-axis
                                    // height to keep its virtualized list visible.
                                    .h_full()
                                    .min_w_0()
                                    .min_h_0()
                                    .child(self.diff_view(&colors, layout, cx)),
                            ),
                    ),
            )
    }
}
