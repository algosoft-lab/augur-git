//! Character-level change ranges for inline diff highlighting.
//!
//! Git reports replacements as whole lines, so an inline view needs to know
//! which characters inside a paired line actually changed. The computation is
//! bounded: a pair whose combined length exceeds [`MAX_INLINE_REFINEMENT_BYTES`]
//! is left unmarked, which keeps pathological minified files from stalling the
//! interface.

use std::ops::Range;

use similar::{ChangeTag, InlineChangeMode, InlineChangeOptions, TextDiff};

use crate::diff::{DiffDocument, DiffLineKind};

/// Combined-length ceiling for one refined line pair.
pub const MAX_INLINE_REFINEMENT_BYTES: usize = 64 * 1024;

/// Character-level changed ranges per line, keyed by source line index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineRanges {
    /// Ranges into the old source, indexed by old line index.
    pub old: Vec<Vec<Range<usize>>>,
    /// Ranges into the new source, indexed by new line index.
    pub new: Vec<Vec<Range<usize>>>,
}

impl InlineRanges {
    /// Ranges for one old line, empty when the line is unchanged.
    pub fn old_for(&self, line_index: Option<usize>) -> &[Range<usize>] {
        line_index
            .and_then(|index| self.old.get(index))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Ranges for one new line, empty when the line is unchanged.
    pub fn new_for(&self, line_index: Option<usize>) -> &[Range<usize>] {
        line_index
            .and_then(|index| self.new.get(index))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Compute the changed character ranges for every paired line in a document.
pub fn inline_ranges(document: &DiffDocument) -> InlineRanges {
    let fallback_size = document.rows.len();
    let mut old_ranges = vec![
        Vec::new();
        document
            .old_source
            .as_ref()
            .map_or(fallback_size, |source| source.lines.len())
    ];
    let mut new_ranges = vec![
        Vec::new();
        document
            .new_source
            .as_ref()
            .map_or(fallback_size, |source| source.lines.len())
    ];
    if document.binary {
        return InlineRanges {
            old: old_ranges,
            new: new_ranges,
        };
    }

    let mut index = 0;
    while index < document.rows.len() {
        if document.rows[index].kind != DiffLineKind::Del {
            index += 1;
            continue;
        }
        let delete_start = index;
        while index < document.rows.len() && document.rows[index].kind == DiffLineKind::Del {
            index += 1;
        }
        let delete_end = index;
        let add_start = index;
        while index < document.rows.len() && document.rows[index].kind == DiffLineKind::Add {
            index += 1;
        }
        let add_end = index;

        for offset in 0..(delete_end - delete_start).min(add_end - add_start) {
            let old_index = delete_start + offset;
            let new_index = add_start + offset;
            let Some(old_text) = document.rows[old_index].old_text.as_deref() else {
                continue;
            };
            let Some(new_text) = document.rows[new_index].new_text.as_deref() else {
                continue;
            };
            if old_text.len().saturating_add(new_text.len()) > MAX_INLINE_REFINEMENT_BYTES {
                continue;
            }
            let (old, new) = inline_ranges_for_pair(old_text, new_text);
            if let Some(slot_index) = document.rows[old_index].old_line_index {
                if let Some(slot) = old_ranges.get_mut(slot_index) {
                    *slot = old;
                }
            }
            if let Some(slot_index) = document.rows[new_index].new_line_index {
                if let Some(slot) = new_ranges.get_mut(slot_index) {
                    *slot = new;
                }
            }
        }
    }

    InlineRanges {
        old: old_ranges,
        new: new_ranges,
    }
}

/// Changed byte ranges inside one deleted/added line pair.
pub fn inline_ranges_for_pair(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
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
                        push_range(&mut old_ranges, old_offset..old_offset + length);
                    }
                    old_offset += length;
                }
                ChangeTag::Insert => {
                    if emphasized && length > 0 {
                        push_range(&mut new_ranges, new_offset..new_offset + length);
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

/// Append a range, merging it into the previous one when they touch.
fn push_range(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if let Some(last) = ranges.last_mut()
        && last.end >= range.start
    {
        last.end = last.end.max(range.end);
        return;
    }
    ranges.push(range);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(patch: &str, old: Option<&str>, new: Option<&str>) -> DiffDocument {
        DiffDocument::from_patch(
            "src/example.rs",
            patch,
            old.map(str::to_string),
            new.map(str::to_string),
        )
    }

    #[test]
    fn a_changed_word_is_isolated_inside_the_line() {
        let (old, new) = inline_ranges_for_pair("let value = 1;", "let value = 2;");
        assert_eq!(old.len(), 1);
        assert_eq!(new.len(), 1);
        assert_eq!(&"let value = 1;"[old[0].clone()], "1");
        assert_eq!(&"let value = 2;"[new[0].clone()], "2");
    }

    #[test]
    fn identical_lines_produce_no_ranges() {
        let (old, new) = inline_ranges_for_pair("same", "same");
        assert!(old.is_empty());
        assert!(new.is_empty());
    }

    #[test]
    fn touching_ranges_are_merged() {
        let (old, _) = inline_ranges_for_pair("abcdef", "abXYef");
        assert_eq!(old.len(), 1);
        assert_eq!(old[0], 2..4);
    }

    #[test]
    fn document_ranges_are_keyed_by_source_line_index() {
        let doc = document(
            "@@ -1,2 +1,2 @@\n context\n-old one\n+new one\n",
            Some("context\nold one\n"),
            Some("context\nnew one\n"),
        );
        let ranges = inline_ranges(&doc);
        assert!(ranges.old_for(Some(0)).is_empty());
        assert!(!ranges.old_for(Some(1)).is_empty());
        assert!(!ranges.new_for(Some(1)).is_empty());
    }

    #[test]
    fn binary_documents_carry_no_ranges() {
        let doc = document(
            "diff --git a/x.png b/x.png\nBinary files a/x.png and b/x.png differ\n",
            None,
            None,
        );
        assert!(doc.binary);
        let ranges = inline_ranges(&doc);
        assert!(ranges.old.iter().all(Vec::is_empty));
        assert!(ranges.new.iter().all(Vec::is_empty));
    }

    #[test]
    fn an_out_of_bounds_line_index_is_not_an_error() {
        let ranges = InlineRanges::default();
        assert!(ranges.old_for(None).is_empty());
        assert!(ranges.old_for(Some(9)).is_empty());
        assert!(ranges.new_for(Some(9)).is_empty());
    }
}
