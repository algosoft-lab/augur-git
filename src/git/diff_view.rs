//! Native GPUI rendering for commit diffs.
//!
//! The view intentionally stays separate from the Git worker. It receives a
//! complete `DiffDocument`, prepares syntax and inline ranges once, and uses a
//! uniform list so large patches do not create one GPUI element per line.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, Div, HighlightStyle, Hsla, ListHorizontalSizingBehavior,
    SharedString, Stateful, StyledText, div, px, relative, uniform_list,
};
use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
use gpui_component::input::Rope;
use gpui_component::{h_flex, theme::ThemeColor};
use similar::{ChangeTag, InlineChangeMode, InlineChangeOptions, TextDiff};

use crate::core::config::DiffLayoutPreference;
use crate::core::diff::{DiffDocument, DiffLineKind, DiffRow, SourceText};

/// Keep syntax parsing bounded for unusually large source files.
pub const MAX_HIGHLIGHT_BYTES: usize = 10 * 1024 * 1024;
const MAX_INLINE_REFINEMENT_BYTES: usize = 64 * 1024;

/// Layout choices for the commit diff viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffLayoutMode {
    #[default]
    Inline,
    SideBySide,
}

impl From<DiffLayoutPreference> for DiffLayoutMode {
    fn from(preference: DiffLayoutPreference) -> Self {
        match preference {
            DiffLayoutPreference::Inline => Self::Inline,
            DiffLayoutPreference::SideBySide => Self::SideBySide,
        }
    }
}

/// Cached syntax and character-level ranges for one diff document.
#[derive(Clone, Debug)]
pub struct DiffViewCache {
    /// Identity of the Git file and language used to build this cache.
    ///
    /// The bottom panel owns one cache at a time, but retaining the key keeps
    /// a future shared cache from accidentally reusing highlights for another
    /// commit or renamed path.
    pub source_key: String,
    pub theme: Arc<HighlightTheme>,
    pub binary: bool,
    pub inline_rows: Arc<Vec<DiffRow>>,
    pub side_rows: Arc<Vec<DiffRow>>,
    pub old_syntax: Vec<Vec<(Range<usize>, HighlightStyle)>>,
    pub new_syntax: Vec<Vec<(Range<usize>, HighlightStyle)>>,
    pub old_inline: Vec<Vec<Range<usize>>>,
    pub new_inline: Vec<Vec<Range<usize>>>,
    pub inline_width_row: usize,
    pub side_width_row: usize,
}

impl DiffViewCache {
    pub fn build_for(
        source_key: impl Into<String>,
        document: &DiffDocument,
        theme: Arc<HighlightTheme>,
    ) -> Self {
        let old_syntax = syntax_for_source(
            document.language.as_deref(),
            document.old_source.as_ref(),
            theme.as_ref(),
        );
        let new_syntax = syntax_for_source(
            document.language.as_deref(),
            document.new_source.as_ref(),
            theme.as_ref(),
        );
        let (old_inline, new_inline) = inline_ranges_for_rows(document);
        let inline_rows = document.rows.clone();
        let side_rows = document.aligned_rows();
        Self {
            source_key: source_key.into(),
            theme,
            binary: document.binary,
            inline_width_row: widest_row_index(&inline_rows, false),
            side_width_row: widest_row_index(&side_rows, true),
            inline_rows: Arc::new(inline_rows),
            side_rows: Arc::new(side_rows),
            old_syntax,
            new_syntax,
            old_inline,
            new_inline,
        }
    }
}

fn widest_row_index(rows: &[DiffRow], side_by_side: bool) -> usize {
    rows.iter()
        .enumerate()
        .max_by_key(|(_, row)| {
            let old_len = row.old_text.as_deref().map_or(0, str::len);
            let new_len = row.new_text.as_deref().map_or(0, str::len);
            if side_by_side {
                old_len.saturating_add(new_len)
            } else {
                old_len.max(new_len)
            }
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn syntax_for_source(
    language: Option<&str>,
    source: Option<&SourceText>,
    theme: &HighlightTheme,
) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let Some(source) = source else {
        return Vec::new();
    };
    let mut rows = vec![Vec::new(); source.lines.len()];
    let Some(language) = language else {
        return rows;
    };
    if source.text.len() > MAX_HIGHLIGHT_BYTES {
        return rows;
    }

    let rope = Rope::from_str(&source.text);
    let mut highlighter = SyntaxHighlighter::new(language);
    if !highlighter.update(None, &rope, Some(Duration::from_millis(20))) {
        return rows;
    }
    let styles = highlighter.styles(&(0..source.text.len()), theme);
    let mut style_index = 0;
    for (line_index, line) in source.lines.iter().enumerate() {
        let Some(line_range) = source.line_range(Some((line_index + 1) as u32))
        else {
            continue;
        };
        let start = line_range.start;
        let end = line_range.end.min(source.text.len());
        while style_index < styles.len() && styles[style_index].0.end <= start {
            style_index += 1;
        }
        let mut current_style = style_index;
        while current_style < styles.len()
            && styles[current_style].0.start < end
        {
            let (range, style) = &styles[current_style];
            let clipped_start = range.start.max(start);
            let clipped_end = range.end.min(end);
            if clipped_start < clipped_end {
                let local = clipped_start - start..clipped_end - start;
                if line.is_char_boundary(local.start)
                    && line.is_char_boundary(local.end)
                {
                    rows[line_index].push((local, *style));
                }
            }
            current_style += 1;
        }
    }
    rows
}

fn inline_ranges_for_rows(
    document: &DiffDocument,
) -> (Vec<Vec<Range<usize>>>, Vec<Vec<Range<usize>>>) {
    let mut old_ranges = vec![Vec::new(); document.rows.len()];
    let mut new_ranges = vec![Vec::new(); document.rows.len()];
    if document.binary {
        return (old_ranges, new_ranges);
    }

    let mut index = 0;
    while index < document.rows.len() {
        if document.rows[index].kind != DiffLineKind::Del {
            index += 1;
            continue;
        }
        let delete_start = index;
        while index < document.rows.len()
            && document.rows[index].kind == DiffLineKind::Del
        {
            index += 1;
        }
        let delete_end = index;
        let add_start = index;
        while index < document.rows.len()
            && document.rows[index].kind == DiffLineKind::Add
        {
            index += 1;
        }
        let add_end = index;
        for offset in 0..(delete_end - delete_start).min(add_end - add_start) {
            let old_index = delete_start + offset;
            let new_index = add_start + offset;
            let Some(old_text) = document.rows[old_index].old_text.as_deref()
            else {
                continue;
            };
            let Some(new_text) = document.rows[new_index].new_text.as_deref()
            else {
                continue;
            };
            if old_text.len().saturating_add(new_text.len())
                > MAX_INLINE_REFINEMENT_BYTES
            {
                continue;
            }
            let (old, new) = inline_ranges(old_text, new_text);
            old_ranges[old_index] = old;
            new_ranges[new_index] = new;
        }
    }
    (old_ranges, new_ranges)
}

fn inline_ranges(
    old: &str,
    new: &str,
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut options = InlineChangeOptions::new();
    options
        .mode(InlineChangeMode::Chars)
        .min_ratio(0.0)
        .semantic_cleanup(true);
    let diff = TextDiff::from_chars(old, new);
    let mut old_offset = 0;
    let mut new_offset = 0;
    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();

    for change in diff.iter_all_inline_changes_with_options(options) {
        let tag = change.tag();
        for (emphasized, value) in change.iter_strings_lossy() {
            let length = value.len();
            match tag {
                ChangeTag::Delete => {
                    if emphasized && length > 0 {
                        push_inline_range(
                            &mut old_ranges,
                            old_offset..old_offset + length,
                        );
                    }
                    old_offset += length;
                }
                ChangeTag::Insert => {
                    if emphasized && length > 0 {
                        push_inline_range(
                            &mut new_ranges,
                            new_offset..new_offset + length,
                        );
                    }
                    new_offset += length;
                }
                ChangeTag::Equal => {
                    old_offset += length;
                    new_offset += length;
                }
            }
        }
    }
    (old_ranges, new_ranges)
}

fn push_inline_range(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if let Some(last) = ranges.last_mut() {
        if last.end >= range.start {
            last.end = last.end.max(range.end);
            return;
        }
    }
    ranges.push(range);
}

/// Render a document using one virtualized list for either layout.
pub fn render_document(
    cache: &Arc<DiffViewCache>,
    layout: DiffLayoutMode,
    colors: &ThemeColor,
    mono: &SharedString,
) -> AnyElement {
    let rows = if layout == DiffLayoutMode::SideBySide {
        Arc::clone(&cache.side_rows)
    } else {
        Arc::clone(&cache.inline_rows)
    };
    let cache = Arc::clone(cache);
    let colors = colors.clone();
    let mono = mono.clone();
    let row_count = rows.len();
    let width_from_item = if layout == DiffLayoutMode::SideBySide {
        cache.side_width_row
    } else {
        cache.inline_width_row
    };
    let list = uniform_list(
        SharedString::from("commit-diff-rows"),
        row_count,
        move |range, _window, _cx| {
            range
                .filter_map(|index| rows.get(index).map(|row| (index, row)))
                .map(|(index, row)| {
                    render_row(row, index, layout, &cache, &colors, &mono)
                })
                .collect::<Vec<_>>()
        },
    )
    .with_width_from_item(Some(width_from_item))
    .with_horizontal_sizing_behavior(
        ListHorizontalSizingBehavior::Unconstrained,
    )
    .w_full()
    .h_full()
    .flex_1()
    .min_h_0();

    div()
        .id("commit-diff-document")
        .w_full()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .child(list)
        .into_any_element()
}

/// A file section used by the aggregate commit diff view.
#[derive(Clone)]
pub struct DiffViewSection {
    pub path: String,
    pub cache: Arc<DiffViewCache>,
}

#[derive(Clone, Copy)]
enum DiffListItem {
    FileHeader(usize),
    Binary,
    Empty,
    Row { section: usize, row: usize },
}

/// Render multiple file diffs in one virtualized list.
///
/// Every file gets a compact header so the aggregate view remains navigable,
/// while all rows share one list and therefore retain the same large-diff
/// virtualization and horizontal scrolling behavior as the single-file view.
pub fn render_documents(
    sections: Vec<DiffViewSection>,
    layout: DiffLayoutMode,
    colors: &ThemeColor,
    mono: &SharedString,
    binary_label: SharedString,
    empty_label: SharedString,
) -> AnyElement {
    let sections = Arc::new(sections);
    let mut items = Vec::new();
    let mut width_item = 0;
    let mut width_score = 0;
    for (section_index, section) in sections.iter().enumerate() {
        items.push(DiffListItem::FileHeader(section_index));
        let rows = if layout == DiffLayoutMode::SideBySide {
            section.cache.side_rows.as_ref()
        } else {
            section.cache.inline_rows.as_ref()
        };
        if section.cache.binary {
            items.push(DiffListItem::Binary);
            continue;
        }
        if rows.is_empty() {
            items.push(DiffListItem::Empty);
            continue;
        }
        for (row_index, row) in rows.iter().enumerate() {
            let score = row
                .old_text
                .as_deref()
                .map_or(0, str::len)
                .max(row.new_text.as_deref().map_or(0, str::len));
            if score > width_score {
                width_score = score;
                width_item = items.len();
            }
            items.push(DiffListItem::Row {
                section: section_index,
                row: row_index,
            });
        }
    }
    let items = Arc::new(items);
    let colors = colors.clone();
    let mono = mono.clone();
    let binary_label = binary_label.clone();
    let empty_label = empty_label.clone();
    let row_count = items.len();
    let list = uniform_list(
        SharedString::from("commit-diff-documents"),
        row_count,
        move |range, _window, _cx| {
            range
                .filter_map(|index| {
                    items.get(index).copied().map(|item| (index, item))
                })
                .map(|(index, item)| match item {
                    DiffListItem::FileHeader(section) => render_section_header(
                        index,
                        &sections[section].path,
                        &colors,
                        &mono,
                    )
                    .into_any_element(),
                    DiffListItem::Binary => div()
                        .id(SharedString::from(format!(
                            "commit-diff-binary-{index}"
                        )))
                        .min_w_full()
                        .h(px(22.))
                        .flex_shrink_0()
                        .items_center()
                        .px_2()
                        .text_size(px(11.))
                        .text_color(colors.muted_foreground)
                        .child(binary_label.clone())
                        .into_any_element(),
                    DiffListItem::Empty => div()
                        .id(SharedString::from(format!(
                            "commit-diff-empty-{index}"
                        )))
                        .min_w_full()
                        .h(px(22.))
                        .flex_shrink_0()
                        .items_center()
                        .px_2()
                        .text_size(px(11.))
                        .text_color(colors.muted_foreground)
                        .child(empty_label.clone())
                        .into_any_element(),
                    DiffListItem::Row { section, row } => {
                        let cache = &sections[section].cache;
                        let rows = if layout == DiffLayoutMode::SideBySide {
                            &cache.side_rows
                        } else {
                            &cache.inline_rows
                        };
                        rows.get(row)
                            .map(|row| {
                                render_row(
                                    row, index, layout, cache, &colors, &mono,
                                )
                                .into_any_element()
                            })
                            .unwrap_or_else(|| {
                                div().h(px(22.)).into_any_element()
                            })
                    }
                })
                .collect::<Vec<_>>()
        },
    )
    .with_width_from_item(Some(width_item))
    .with_horizontal_sizing_behavior(
        ListHorizontalSizingBehavior::Unconstrained,
    )
    .w_full()
    .h_full()
    .flex_1()
    .min_h_0();

    div()
        .id("commit-diff-documents")
        .w_full()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .child(list)
        .into_any_element()
}

fn render_section_header(
    index: usize,
    path: &str,
    colors: &ThemeColor,
    mono: &SharedString,
) -> Stateful<Div> {
    h_flex()
        .id(SharedString::from(format!("commit-diff-file-{index}")))
        .min_w_full()
        .h(px(22.))
        .flex_shrink_0()
        .items_center()
        .px_2()
        .bg(colors.tab_bar)
        .border_b_1()
        .border_color(colors.border)
        .font_family(mono.clone())
        .text_size(px(11.))
        .text_color(colors.muted_foreground)
        .whitespace_nowrap()
        .child(SharedString::from(path.to_string()))
}

fn render_row(
    row: &DiffRow,
    index: usize,
    layout: DiffLayoutMode,
    cache: &DiffViewCache,
    colors: &ThemeColor,
    mono: &SharedString,
) -> AnyElement {
    if row.kind == DiffLineKind::Hunk {
        return h_flex()
            .id(SharedString::from(format!("commit-diff-hunk-{index}")))
            .min_w_full()
            .h(px(22.))
            .flex_shrink_0()
            .items_center()
            .px_2()
            .bg(colors.blue.opacity(0.10))
            .text_color(colors.blue)
            .font_family(mono.clone())
            .text_size(px(11.))
            .whitespace_nowrap()
            .child(SharedString::from(
                row.hunk_header.clone().unwrap_or_default(),
            ))
            .into_any_element();
    }
    match layout {
        DiffLayoutMode::Inline => {
            render_inline_row(row, index, cache, colors, mono)
                .into_any_element()
        }
        DiffLayoutMode::SideBySide => {
            render_side_by_side_row(row, index, cache, colors, mono)
                .into_any_element()
        }
    }
}

fn render_inline_row(
    row: &DiffRow,
    index: usize,
    cache: &DiffViewCache,
    colors: &ThemeColor,
    mono: &SharedString,
) -> Stateful<Div> {
    let (row_color, marker, marker_color) = match row.kind {
        DiffLineKind::Add => (colors.green, "+", colors.green),
        DiffLineKind::Del => (colors.red, "-", colors.red),
        _ => (colors.foreground, "", colors.muted_foreground),
    };
    let background = match row.kind {
        DiffLineKind::Add => Some(colors.green.opacity(0.12)),
        DiffLineKind::Del => Some(colors.red.opacity(0.12)),
        _ => None,
    };
    let inline_color = match row.kind {
        DiffLineKind::Del => colors.red.opacity(0.30),
        _ => colors.green.opacity(0.30),
    };
    let new_side = row.kind == DiffLineKind::Add
        || (row.kind == DiffLineKind::Context && row.new_text.is_some());
    h_flex()
        .id(SharedString::from(format!("commit-diff-row-{index}")))
        .min_w_full()
        .h(px(22.))
        .flex_shrink_0()
        .items_center()
        .when_some(background, |this, bg| this.bg(bg))
        .child(number_gutter(row.old_no, colors, mono))
        .child(number_gutter(row.new_no, colors, mono))
        .child(
            div()
                .w(px(18.))
                .flex_shrink_0()
                .font_family(mono.clone())
                .text_size(px(12.))
                .text_color(marker_color)
                .text_center()
                .child(SharedString::from(marker)),
        )
        .child(code_cell(
            row.new_text
                .as_deref()
                .or(row.old_text.as_deref())
                .unwrap_or(""),
            syntax_for_row(row, cache, new_side),
            inline_for_row(row, cache, new_side),
            row_color,
            inline_color,
            mono,
        ))
}

fn render_side_by_side_row(
    row: &DiffRow,
    index: usize,
    cache: &DiffViewCache,
    colors: &ThemeColor,
    mono: &SharedString,
) -> Stateful<Div> {
    let changed_pair = row.kind != DiffLineKind::Context
        || row.old_text.as_deref() != row.new_text.as_deref();
    let old_background = if changed_pair
        && row.old_text.is_some()
        && row.kind != DiffLineKind::Add
    {
        if row.new_text.is_some() {
            Some(colors.red.opacity(0.06))
        } else {
            Some(colors.red.opacity(0.12))
        }
    } else {
        None
    };
    let new_background = if changed_pair
        && row.new_text.is_some()
        && row.kind != DiffLineKind::Del
    {
        if row.old_text.is_some() {
            Some(colors.green.opacity(0.06))
        } else {
            Some(colors.green.opacity(0.12))
        }
    } else {
        None
    };
    h_flex()
        .id(SharedString::from(format!("commit-diff-split-row-{index}")))
        .min_w_full()
        .h(px(22.))
        .flex_shrink_0()
        .items_stretch()
        .child(side_cell(
            row.old_text.as_deref(),
            row.old_no,
            row.old_line_index,
            cache,
            false,
            old_background,
            colors,
            mono,
        ))
        .child(div().w(px(1.)).flex_shrink_0().bg(colors.border))
        .child(side_cell(
            row.new_text.as_deref(),
            row.new_no,
            row.new_line_index,
            cache,
            true,
            new_background,
            colors,
            mono,
        ))
}

fn side_cell(
    text: Option<&str>,
    line_number: Option<u32>,
    line_index: Option<usize>,
    cache: &DiffViewCache,
    new_side: bool,
    background: Option<Hsla>,
    colors: &ThemeColor,
    mono: &SharedString,
) -> Div {
    let text = text.unwrap_or("");
    let syntax = if new_side {
        cache.new_syntax.get(line_index.unwrap_or(usize::MAX))
    } else {
        cache.old_syntax.get(line_index.unwrap_or(usize::MAX))
    };
    let inline = if new_side {
        cache.new_inline.get(line_index.unwrap_or(usize::MAX))
    } else {
        cache.old_inline.get(line_index.unwrap_or(usize::MAX))
    };
    let marker = if background.is_some() {
        if new_side { "+" } else { "-" }
    } else {
        ""
    };
    let inline_color = if new_side {
        colors.green.opacity(0.30)
    } else {
        colors.red.opacity(0.30)
    };
    h_flex()
        .w(relative(0.5))
        .min_w_0()
        .flex_shrink_0()
        .items_center()
        .when_some(background, |this, bg| this.bg(bg))
        .child(number_gutter(line_number, colors, mono))
        .child(
            div()
                .w(px(18.))
                .flex_shrink_0()
                .font_family(mono.clone())
                .text_size(px(12.))
                .text_center()
                .text_color(if new_side { colors.green } else { colors.red })
                .child(SharedString::from(marker)),
        )
        .child(code_cell(
            text,
            syntax.map(|value| value.as_slice()),
            inline.map(|value| value.as_slice()),
            colors.foreground,
            inline_color,
            mono,
        ))
}

fn number_gutter(
    number: Option<u32>,
    colors: &ThemeColor,
    mono: &SharedString,
) -> Div {
    div()
        .w(px(42.))
        .flex_shrink_0()
        .px_1()
        .font_family(mono.clone())
        .text_size(px(11.))
        .text_color(colors.muted_foreground.opacity(0.72))
        .text_right()
        .whitespace_nowrap()
        .child(SharedString::from(
            number.map(|number| number.to_string()).unwrap_or_default(),
        ))
}

fn syntax_for_row<'a>(
    row: &DiffRow,
    cache: &'a DiffViewCache,
    new_side: bool,
) -> Option<&'a [(Range<usize>, HighlightStyle)]> {
    let index = if new_side {
        row.new_line_index
    } else {
        row.old_line_index
    }?;
    if new_side {
        cache.new_syntax.get(index).map(Vec::as_slice)
    } else {
        cache.old_syntax.get(index).map(Vec::as_slice)
    }
}

fn inline_for_row<'a>(
    row: &DiffRow,
    cache: &'a DiffViewCache,
    new_side: bool,
) -> Option<&'a [Range<usize>]> {
    let index = if new_side {
        row.new_line_index
    } else {
        row.old_line_index
    }?;
    if new_side {
        cache.new_inline.get(index).map(Vec::as_slice)
    } else {
        cache.old_inline.get(index).map(Vec::as_slice)
    }
}

fn code_cell(
    text: &str,
    syntax: Option<&[(Range<usize>, HighlightStyle)]>,
    inline: Option<&[Range<usize>]>,
    row_color: Hsla,
    inline_color: Hsla,
    mono: &SharedString,
) -> Div {
    let text = if text.is_empty() { " " } else { text };
    let highlights = merged_highlights(
        text,
        syntax.unwrap_or(&[]),
        inline.unwrap_or(&[]),
        inline_color,
    );
    let styled_text = StyledText::new(SharedString::from(text.to_string()))
        .with_highlights(highlights);
    div()
        .min_w_0()
        .flex_1()
        .flex_shrink_0()
        .font_family(mono.clone())
        .text_size(px(12.))
        .text_color(row_color)
        .whitespace_nowrap()
        .child(styled_text)
}

fn merged_highlights(
    text: &str,
    syntax: &[(Range<usize>, HighlightStyle)],
    inline: &[Range<usize>],
    inline_color: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut boundaries = vec![0, text.len()];
    for (range, _) in syntax {
        boundaries.push(range.start.min(text.len()));
        boundaries.push(range.end.min(text.len()));
    }
    for range in inline {
        boundaries.push(range.start.min(text.len()));
        boundaries.push(range.end.min(text.len()));
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
        .windows(2)
        .filter_map(|window| {
            let start = window[0];
            let end = window[1];
            if start >= end
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return None;
            }
            let mut style = syntax
                .iter()
                .find(|(range, _)| range.start <= start && start < range.end)
                .map(|(_, style)| *style)
                .unwrap_or_default();
            if inline
                .iter()
                .any(|range| range.start <= start && start < range.end)
            {
                style.background_color = Some(inline_color);
            }
            Some((start..end, style))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::inline_ranges;

    #[test]
    fn inline_ranges_mark_changed_utf8_characters() {
        let old = "let value = 旧;";
        let new = "let value = 新;";
        let (old_ranges, new_ranges) = inline_ranges(old, new);
        assert!(!old_ranges.is_empty());
        assert!(!new_ranges.is_empty());
        for range in old_ranges {
            assert!(old.is_char_boundary(range.start));
            assert!(old.is_char_boundary(range.end));
        }
        for range in new_ranges {
            assert!(new.is_char_boundary(range.start));
            assert!(new.is_char_boundary(range.end));
        }
    }

    #[test]
    fn inline_ranges_skip_equal_prefix_and_suffix() {
        let (old_ranges, new_ranges) =
            inline_ranges("return old_value;", "return new_value;");
        assert_eq!(old_ranges, vec![7..10]);
        assert_eq!(new_ranges, vec![7..10]);
    }
}
