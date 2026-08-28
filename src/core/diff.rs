//! Pure data models and parsers used by the commit diff viewer.

use std::collections::HashMap;
use std::ops::Range;

/// The kind of change represented by a file entry in a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChangeStatus {
    Added,
    Copied,
    Deleted,
    Modified,
    Renamed,
    TypeChanged,
    Unmerged,
    Unknown,
}

impl FileChangeStatus {
    pub fn from_status(status: char) -> Self {
        match status {
            'A' => Self::Added,
            'C' => Self::Copied,
            'D' => Self::Deleted,
            'M' => Self::Modified,
            'R' => Self::Renamed,
            'T' => Self::TypeChanged,
            'U' => Self::Unmerged,
            _ => Self::Unknown,
        }
    }

    pub fn is_rename(self) -> bool {
        matches!(self, Self::Renamed | Self::Copied)
    }
}

/// A file changed by a commit.
///
/// `path` is retained as the display label used by the existing file list.
/// Git operations must use `old_path`/`new_path` instead of this label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub new_path: String,
    pub status: FileChangeStatus,
    pub old_blob: Option<String>,
    pub new_blob: Option<String>,
    pub added: Option<usize>,
    pub deleted: Option<usize>,
}

impl FileChange {
    pub fn is_binary(&self) -> bool {
        self.added.is_none() || self.deleted.is_none()
    }

    pub fn identity(&self) -> String {
        format!(
            "{}:{}:{}",
            self.old_path.as_deref().unwrap_or(""),
            self.new_path,
            self.status_code()
        )
    }

    pub fn status_code(&self) -> char {
        match self.status {
            FileChangeStatus::Added => 'A',
            FileChangeStatus::Copied => 'C',
            FileChangeStatus::Deleted => 'D',
            FileChangeStatus::Modified => 'M',
            FileChangeStatus::Renamed => 'R',
            FileChangeStatus::TypeChanged => 'T',
            FileChangeStatus::Unmerged => 'U',
            FileChangeStatus::Unknown => '?',
        }
    }
}

/// Parse the human-readable numstat form emitted by `git show`.
pub(crate) fn parse_numstat(text: &str) -> Vec<FileChange> {
    text.lines()
        .filter_map(|line| {
            let (added, rest) = line.split_once('\t')?;
            let (deleted, encoded_path) = rest.split_once('\t')?;
            let path = decode_git_path(encoded_path);
            if path.is_empty() {
                return None;
            }

            let (old_path, new_path, status) = split_rename_label(&path);
            let display_path = old_path
                .as_ref()
                .map(|old| format!("{old} => {new_path}"))
                .unwrap_or_else(|| new_path.clone());
            Some(FileChange {
                path: display_path,
                old_path,
                new_path,
                status,
                old_blob: None,
                new_blob: None,
                added: added.parse().ok(),
                deleted: deleted.parse().ok(),
            })
        })
        .collect()
}

fn decode_git_path(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() < 2
        || bytes.first() != Some(&b'"')
        || bytes.last() != Some(&b'"')
    {
        return path.to_string();
    }

    let mut decoded = Vec::with_capacity(bytes.len().saturating_sub(2));
    let mut index = 1;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if byte != b'\\' {
            decoded.push(byte);
            continue;
        }
        let Some(&escaped) = bytes.get(index) else {
            return path.to_string();
        };
        index += 1;
        match escaped {
            b'a' => decoded.push(0x07),
            b'b' => decoded.push(0x08),
            b't' => decoded.push(b'\t'),
            b'n' => decoded.push(b'\n'),
            b'v' => decoded.push(0x0b),
            b'f' => decoded.push(0x0c),
            b'r' => decoded.push(b'\r'),
            b'\\' => decoded.push(b'\\'),
            b'"' => decoded.push(b'"'),
            b'0'..=b'7' => {
                let mut value = u16::from(escaped - b'0');
                for _ in 0..2 {
                    let Some(next @ b'0'..=b'7') = bytes.get(index).copied()
                    else {
                        break;
                    };
                    value = value * 8 + u16::from(next - b'0');
                    index += 1;
                }
                decoded.push(value as u8);
            }
            _ => {
                decoded.push(b'\\');
                decoded.push(escaped);
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn split_rename_label(
    path: &str,
) -> (Option<String>, String, FileChangeStatus) {
    let Some(arrow) = path.find(" => ") else {
        return (None, path.to_string(), FileChangeStatus::Modified);
    };
    let arrow_end = arrow + " => ".len();

    // Git collapses common directory/name prefixes into a brace expression,
    // e.g. src/{old => new}.rs. Expand it so raw records and numstat rows
    // use the same canonical old/new paths for matching.
    let brace_open = path[..arrow].rfind('{');
    let brace_close =
        path[arrow_end..].find('}').map(|offset| arrow_end + offset);
    if let (Some(open), Some(close)) = (brace_open, brace_close) {
        let prefix = &path[..open];
        let old_name = &path[open + 1..arrow];
        let new_name = &path[arrow_end..close];
        let suffix = &path[close + 1..];
        let old = format!("{prefix}{old_name}{suffix}");
        let new = format!("{prefix}{new_name}{suffix}");
        return (Some(old), new, FileChangeStatus::Renamed);
    }

    let old = path[..arrow].to_string();
    let new = path[arrow_end..].to_string();
    if old.is_empty() || new.is_empty() {
        return (None, path.to_string(), FileChangeStatus::Modified);
    }
    (Some(old), new, FileChangeStatus::Renamed)
}

/// Parse NUL-delimited `git diff-tree --raw -z` records.
///
/// A normal record is `header\0path\0`; rename/copy records are
/// `header\0old-path\0new-path\0`.
pub(crate) fn parse_raw_records(data: &[u8]) -> Vec<FileChange> {
    let fields: Vec<&[u8]> = data.split(|byte| *byte == 0).collect();
    let mut index = 0;
    let mut changes = Vec::new();

    while index < fields.len() {
        let header = fields[index];
        index += 1;
        if header.is_empty() {
            continue;
        }
        let header = String::from_utf8_lossy(header);
        let mut parts = header.split_whitespace();
        let _old_mode = parts.next();
        let _new_mode = parts.next();
        let old_blob = parts.next().and_then(valid_blob);
        let new_blob = parts.next().and_then(valid_blob);
        let status_field = parts.next().unwrap_or("?");
        let status_char = status_field.chars().next().unwrap_or('?');
        let status = FileChangeStatus::from_status(status_char);

        let Some(first_path) = fields.get(index) else {
            break;
        };
        index += 1;
        let first_path = String::from_utf8_lossy(first_path).into_owned();
        let (old_path, new_path) = if status.is_rename() {
            let Some(second_path) = fields.get(index) else {
                break;
            };
            index += 1;
            (
                Some(first_path),
                String::from_utf8_lossy(second_path).into_owned(),
            )
        } else {
            let old_path = if matches!(status, FileChangeStatus::Added) {
                None
            } else {
                Some(first_path.clone())
            };
            let new_path = if matches!(status, FileChangeStatus::Deleted) {
                first_path.clone()
            } else {
                first_path
            };
            (old_path, new_path)
        };

        let path = old_path
            .as_ref()
            .filter(|_| status.is_rename())
            .map(|old| format!("{old} => {new_path}"))
            .unwrap_or_else(|| new_path.clone());
        changes.push(FileChange {
            path,
            old_path,
            new_path,
            status,
            old_blob,
            new_blob,
            added: None,
            deleted: None,
        });
    }

    changes
}

fn valid_blob(value: &str) -> Option<String> {
    (matches!(value.len(), 40 | 64)
        && value.bytes().any(|byte| byte != b'0')
        && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| value.to_string())
}

/// Merge numstat counts into raw file metadata, matching by new and old path.
pub(crate) fn merge_numstat(
    mut raw: Vec<FileChange>,
    stats: Vec<FileChange>,
) -> Vec<FileChange> {
    let mut by_path = HashMap::new();
    for stat in stats {
        by_path.insert(stat.path.clone(), stat);
    }
    for change in &mut raw {
        let candidates = [
            change.path.as_str(),
            change.new_path.as_str(),
            change.old_path.as_deref().unwrap_or(""),
        ];
        if let Some(stat) =
            candidates.iter().find_map(|path| by_path.remove(*path))
        {
            change.added = stat.added;
            change.deleted = stat.deleted;
        }
    }
    raw
}

/// A source file and byte offsets for each logical line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceText {
    pub text: String,
    pub line_starts: Vec<usize>,
    pub lines: Vec<String>,
}

impl SourceText {
    pub fn new(text: String) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        let mut lines: Vec<String> = text
            .split_terminator('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect();
        if !text.is_empty() && !text.ends_with('\n') {
            if lines.is_empty() {
                lines.push(text.clone());
            }
        }
        Self {
            text,
            line_starts,
            lines,
        }
    }

    pub fn line(&self, line_number: Option<u32>) -> Option<&str> {
        line_number
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| self.lines.get(index as usize))
            .map(String::as_str)
    }

    pub fn line_start(&self, line_number: Option<u32>) -> Option<usize> {
        line_number
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| self.line_starts.get(index as usize))
            .copied()
    }

    pub fn line_range(&self, line_number: Option<u32>) -> Option<Range<usize>> {
        let line_index =
            line_number.and_then(|number| number.checked_sub(1))? as usize;
        let start = self.line_start(line_number)?;
        let line = self.lines.get(line_index)?;
        Some(start..start.saturating_add(line.len()))
    }
}

/// A line-level change with clean old/new source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: DiffLineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    pub old_line_index: Option<usize>,
    pub new_line_index: Option<usize>,
    pub hunk_header: Option<String>,
}

/// A parsed single-file commit diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffDocument {
    pub path: String,
    pub language: Option<String>,
    pub rows: Vec<DiffRow>,
    pub old_source: Option<SourceText>,
    pub new_source: Option<SourceText>,
    pub binary: bool,
}

impl DiffDocument {
    pub fn from_patch(
        path: impl Into<String>,
        patch: &str,
        old_source: Option<String>,
        new_source: Option<String>,
    ) -> Self {
        let path = path.into();
        let language = language_for_path(&path);
        let old_source = old_source.map(SourceText::new);
        let new_source = new_source.map(SourceText::new);
        let binary = is_binary_patch(patch);
        let rows = parse_diff(patch)
            .into_iter()
            .filter_map(|line| match line.kind {
                DiffLineKind::Hunk => Some(DiffRow {
                    kind: line.kind,
                    old_no: None,
                    new_no: None,
                    old_text: None,
                    new_text: None,
                    old_line_index: None,
                    new_line_index: None,
                    hunk_header: Some(line.text),
                }),
                DiffLineKind::Add => Some(row_from_line(
                    line,
                    old_source.as_ref(),
                    new_source.as_ref(),
                )),
                DiffLineKind::Del | DiffLineKind::Context => {
                    Some(row_from_line(
                        line,
                        old_source.as_ref(),
                        new_source.as_ref(),
                    ))
                }
                DiffLineKind::Meta => None,
            })
            .collect();
        Self {
            path,
            language,
            rows,
            old_source,
            new_source,
            binary,
        }
    }

    /// Return rows aligned for a side-by-side view.
    ///
    /// Git emits a replacement as a run of deleted rows followed by a run of
    /// added rows. Pairing those runs keeps both panes at the same height while
    /// preserving unmatched rows as blank cells on the other side.
    pub fn aligned_rows(&self) -> Vec<DiffRow> {
        let mut aligned = Vec::with_capacity(self.rows.len());
        let mut index = 0;
        while index < self.rows.len() {
            let row = &self.rows[index];
            if row.kind != DiffLineKind::Del {
                aligned.push(row.clone());
                index += 1;
                continue;
            }

            let delete_start = index;
            while index < self.rows.len()
                && self.rows[index].kind == DiffLineKind::Del
            {
                index += 1;
            }
            let delete_end = index;
            let add_start = index;
            while index < self.rows.len()
                && self.rows[index].kind == DiffLineKind::Add
            {
                index += 1;
            }
            let add_end = index;
            let delete_count = delete_end - delete_start;
            let add_count = add_end - add_start;
            let count = delete_count.max(add_count);
            for offset in 0..count {
                let old = self.rows.get(delete_start + offset);
                let new = self.rows.get(add_start + offset);
                let Some(template) = old.or(new) else {
                    continue;
                };
                aligned.push(DiffRow {
                    kind: match (old, new) {
                        (Some(_), Some(_)) => DiffLineKind::Context,
                        (Some(_), None) => DiffLineKind::Del,
                        (None, Some(_)) => DiffLineKind::Add,
                        (None, None) => DiffLineKind::Meta,
                    },
                    old_no: old.and_then(|row| row.old_no),
                    new_no: new.and_then(|row| row.new_no),
                    old_text: old.and_then(|row| row.old_text.clone()),
                    new_text: new.and_then(|row| row.new_text.clone()),
                    old_line_index: old.and_then(|row| row.old_line_index),
                    new_line_index: new.and_then(|row| row.new_line_index),
                    hunk_header: template.hunk_header.clone(),
                });
            }
        }
        aligned
    }

    /// Reconstruct a compact unified representation for clipboard copying.
    pub fn copy_text(&self) -> String {
        let mut text = String::new();
        for row in &self.rows {
            match row.kind {
                DiffLineKind::Hunk => {
                    if let Some(header) = &row.hunk_header {
                        text.push_str(header);
                        text.push('\n');
                    }
                }
                DiffLineKind::Add => {
                    text.push('+');
                    text.push_str(row.new_text.as_deref().unwrap_or_default());
                    text.push('\n');
                }
                DiffLineKind::Del => {
                    text.push('-');
                    text.push_str(row.old_text.as_deref().unwrap_or_default());
                    text.push('\n');
                }
                DiffLineKind::Context => {
                    text.push(' ');
                    text.push_str(
                        row.new_text
                            .as_deref()
                            .or(row.old_text.as_deref())
                            .unwrap_or_default(),
                    );
                    text.push('\n');
                }
                DiffLineKind::Meta => {}
            }
        }
        text
    }
}

/// Detect Git's binary-diff metadata without inspecting the changed content.
///
/// Searching the whole patch is incorrect because a text file can contain
/// strings such as `Binary files ` or `GIT binary patch` in its own source.
pub fn is_binary_patch(patch: &str) -> bool {
    patch.lines().any(|line| {
        line.starts_with("Binary files ") || line == "GIT binary patch"
    })
}

fn row_from_line(
    line: DiffLine,
    old_source: Option<&SourceText>,
    new_source: Option<&SourceText>,
) -> DiffRow {
    let old_text = match line.kind {
        DiffLineKind::Add => None,
        DiffLineKind::Del | DiffLineKind::Context => Some(
            old_source
                .and_then(|source| source.line(line.old_no))
                .map(str::to_string)
                .unwrap_or_else(|| clean_patch_text(&line.text)),
        ),
        _ => None,
    };
    let new_text = match line.kind {
        DiffLineKind::Del => None,
        DiffLineKind::Add | DiffLineKind::Context => Some(
            new_source
                .and_then(|source| source.line(line.new_no))
                .map(str::to_string)
                .unwrap_or_else(|| clean_patch_text(&line.text)),
        ),
        _ => None,
    };
    DiffRow {
        kind: line.kind,
        old_no: line.old_no,
        new_no: line.new_no,
        old_text,
        new_text,
        old_line_index: line.old_no.and_then(|number| {
            number.checked_sub(1).map(|number| number as usize)
        }),
        new_line_index: line.new_no.and_then(|number| {
            number.checked_sub(1).map(|number| number as usize)
        }),
        hunk_header: None,
    }
}

fn clean_patch_text(text: &str) -> String {
    text.strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .or_else(|| text.strip_prefix(' '))
        .unwrap_or(text)
        .to_string()
}

/// A unified diff line category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Meta,
    Hunk,
    Add,
    Del,
    Context,
}

/// The legacy parser representation retained for focused parser tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

/// Parse a unified diff and track old/new line numbers.
pub fn parse_diff(text: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let (mut old, mut new): (u32, u32) = (0, 0);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            let old_start = rest
                .split_whitespace()
                .next()
                .and_then(|token| token.strip_prefix('-'))
                .and_then(|value| value.split(',').next())
                .and_then(|value| value.parse().ok());
            let new_start = rest
                .split_whitespace()
                .nth(1)
                .and_then(|token| token.strip_prefix('+'))
                .and_then(|value| value.split(',').next())
                .and_then(|value| value.parse().ok());
            if let (Some(old_start), Some(new_start)) = (old_start, new_start) {
                old = old_start;
                new = new_start;
                out.push(DiffLine {
                    kind: DiffLineKind::Hunk,
                    old_no: None,
                    new_no: None,
                    text: line.to_string(),
                });
                continue;
            }
        }
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            out.push(DiffLine {
                kind: DiffLineKind::Meta,
                old_no: None,
                new_no: None,
                text: line.to_string(),
            });
            continue;
        }
        match line.chars().next() {
            Some('+') => {
                out.push(DiffLine {
                    kind: DiffLineKind::Add,
                    old_no: None,
                    new_no: Some(new),
                    text: line.to_string(),
                });
                new += 1;
            }
            Some('-') => {
                out.push(DiffLine {
                    kind: DiffLineKind::Del,
                    old_no: Some(old),
                    new_no: None,
                    text: line.to_string(),
                });
                old += 1;
            }
            Some(' ') | None => {
                out.push(DiffLine {
                    kind: DiffLineKind::Context,
                    old_no: Some(old),
                    new_no: Some(new),
                    text: line.to_string(),
                });
                old += 1;
                new += 1;
            }
            _ => out.push(DiffLine {
                kind: DiffLineKind::Meta,
                old_no: None,
                new_no: None,
                text: line.to_string(),
            }),
        }
    }
    out
}

/// Calculate the five-segment add/delete bar used by the file list.
pub fn stat_blocks(added: usize, deleted: usize) -> (usize, usize) {
    const TOTAL_BLOCKS: usize = 5;
    let total = added + deleted;
    if total == 0 {
        return (0, 0);
    }
    let green = (added * TOTAL_BLOCKS).div_ceil(total).min(TOTAL_BLOCKS);
    (green, TOTAL_BLOCKS - green)
}

/// Resolve a Tree-sitter language name from a path extension.
pub fn language_for_path(path: &str) -> Option<String> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower_name = file_name.to_ascii_lowercase();
    if matches!(
        lower_name.as_str(),
        "dockerfile" | "makefile" | "gnumakefile" | "cmakelists.txt"
    ) {
        return Some(match lower_name.as_str() {
            "dockerfile" => "bash".to_string(),
            "cmakelists.txt" => "cmake".to_string(),
            _ => "make".to_string(),
        });
    }
    let extension = lower_name.rsplit_once('.')?.1;
    let language = match extension {
        "astro" => "astro",
        "rs" => "rust",
        "c" | "h" => "c",
        "cc" | "cp" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "cpp",
        "cs" => "csharp",
        "css" | "scss" => "css",
        "cmake" => "cmake",
        "diff" | "patch" => "diff",
        "ejs" => "ejs",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "ex" | "exs" => "elixir",
        "erb" => "erb",
        "go" => "go",
        "graphql" | "gql" => "graphql",
        "htm" | "html" => "html",
        "java" => "java",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "jsdoc" => "jsdoc",
        "json" | "jsonc" => "json",
        "kt" | "kts" | "ktm" => "kotlin",
        "lua" => "lua",
        "md" | "markdown" | "mdx" => "markdown",
        "php" => "php",
        "proto" | "protobuf" => "proto",
        "py" | "pyi" => "python",
        "rb" | "rake" => "ruby",
        "scala" => "scala",
        "sql" => "sql",
        "svelte" => "svelte",
        "swift" => "swift",
        "toml" => "toml",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "yaml" | "yml" => "yaml",
        "zig" => "zig",
        _ => return None,
    };
    Some(language.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_strips_patch_markers_and_metadata() {
        let patch = "diff --git a/src/main.rs b/src/main.rs\nindex 1..2\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,2 +1,2 @@\n-fn old() {}\n+fn new() {}\n";
        let document = DiffDocument::from_patch(
            "src/main.rs",
            patch,
            Some("fn old() {}\n".to_string()),
            Some("fn new() {}\n".to_string()),
        );
        assert_eq!(document.language.as_deref(), Some("rust"));
        assert_eq!(document.rows.len(), 3);
        assert_eq!(document.rows[1].old_text.as_deref(), Some("fn old() {}"));
        assert_eq!(document.rows[2].new_text.as_deref(), Some("fn new() {}"));
    }

    #[test]
    fn raw_records_preserve_rename_paths_and_blobs() {
        let data = b":100644 100644 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb R100\0old.rs\0new.rs\0";
        let records = parse_raw_records(data);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].old_path.as_deref(), Some("old.rs"));
        assert_eq!(records[0].new_path, "new.rs");
        assert_eq!(
            records[0].old_blob.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn source_text_handles_trailing_newlines() {
        let source = SourceText::new("a\n\nb".to_string());
        assert_eq!(source.lines, vec!["a", "", "b"]);
        assert_eq!(source.line(Some(2)), Some(""));
        assert_eq!(source.line_start(Some(3)), Some(3));
    }

    #[test]
    fn source_text_maps_utf8_lines_to_byte_ranges() {
        let source = SourceText::new("猫\nvalue".to_string());
        assert_eq!(source.line_range(Some(1)), Some(0..3));
        assert_eq!(source.line_range(Some(2)), Some(4..9));
    }

    #[test]
    fn language_mapping_has_plain_fallback() {
        assert_eq!(language_for_path("src/main.tsx").as_deref(), Some("tsx"));
        assert_eq!(language_for_path("Cargo.toml").as_deref(), Some("toml"));
        assert_eq!(
            language_for_path("src/App.astro").as_deref(),
            Some("astro")
        );
        assert_eq!(
            language_for_path("CMakeLists.txt").as_deref(),
            Some("cmake")
        );
        assert_eq!(language_for_path("README").as_deref(), None);
    }

    #[test]
    fn aligned_rows_pair_replacements_and_preserve_unmatched_lines() {
        let patch = "@@ -1,3 +1,4 @@\n-old one\n-old two\n+new one\n+new two\n+new three\n context\n";
        let document = DiffDocument::from_patch(
            "src/example.rs",
            patch,
            Some("old one\nold two\ncontext\n".to_string()),
            Some("new one\nnew two\nnew three\ncontext\n".to_string()),
        );
        let rows = document.aligned_rows();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[1].old_text.as_deref(), Some("old one"));
        assert_eq!(rows[1].new_text.as_deref(), Some("new one"));
        assert_eq!(rows[3].old_text, None);
        assert_eq!(rows[3].new_text.as_deref(), Some("new three"));
    }

    #[test]
    fn document_copy_text_uses_clean_source_lines() {
        let patch = "@@ -1 +1 @@\n-old\n+new\n";
        let document = DiffDocument::from_patch(
            "src/example.py",
            patch,
            Some("old\n".to_string()),
            Some("new\n".to_string()),
        );
        assert_eq!(document.copy_text(), "@@ -1 +1 @@\n-old\n+new\n");
    }

    #[test]
    fn binary_patch_has_no_renderable_rows() {
        let document = DiffDocument::from_patch(
            "assets/logo.png",
            "diff --git a/assets/logo.png b/assets/logo.png\nBinary files a/assets/logo.png and b/assets/logo.png differ\n",
            None,
            None,
        );
        assert!(document.binary);
        assert!(document.rows.is_empty());
    }

    #[test]
    fn binary_detection_ignores_matching_text_in_patch_content() {
        let patch = "@@ -1,1 +1,2 @@\n-old\n+let marker = \"Binary files \";\n+let other = \"GIT binary patch\";\n";
        let document = DiffDocument::from_patch(
            "src/git.rs",
            patch,
            Some("old\n".to_string()),
            Some(
                "let marker = \"Binary files \";\nlet other = \"GIT binary patch\";\n"
                    .to_string(),
            ),
        );
        assert!(!document.binary);
        assert!(!document.rows.is_empty());
    }

    #[test]
    fn document_keeps_multiple_hunks_but_hides_no_newline_metadata() {
        let patch = "@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n@@ -4 +4 @@\n-before\n+after\n";
        let document =
            DiffDocument::from_patch("src/example.rs", patch, None, None);
        assert_eq!(
            document
                .rows
                .iter()
                .filter(|row| row.kind == DiffLineKind::Hunk)
                .count(),
            2
        );
        assert_eq!(document.rows.len(), 6);
        assert!(document.copy_text().contains("@@ -4 +4 @@"));
        assert!(!document.copy_text().contains("No newline"));
    }

    #[test]
    fn parse_diff_accepts_zero_length_hunk_ranges() {
        let lines = parse_diff("@@ -0,0 +1 @@\n+created\n");
        assert_eq!(lines[0].kind, DiffLineKind::Hunk);
        assert_eq!(lines[1].new_no, Some(1));
        assert_eq!(lines[1].old_no, None);
    }

    #[test]
    fn raw_records_handle_binary_and_non_ascii_paths() {
        let zero = "0000000000000000000000000000000000000000";
        let data = format!(
            ":100644 100644 {zero} {} M\0space name/é.rs\0",
            "b".repeat(40)
        );
        let records = parse_raw_records(data.as_bytes());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].old_blob, None);
        assert_eq!(records[0].new_path, "space name/é.rs");
    }

    #[test]
    fn raw_records_accept_sha256_blob_ids() {
        let old_blob = "a".repeat(64);
        let new_blob = "b".repeat(64);
        let data =
            format!(":100644 100644 {old_blob} {new_blob} M\0src/main.rs\0");
        let records = parse_raw_records(data.as_bytes());
        assert_eq!(records[0].old_blob.as_deref(), Some(old_blob.as_str()));
        assert_eq!(records[0].new_blob.as_deref(), Some(new_blob.as_str()));
    }

    #[test]
    fn raw_records_assign_old_and_new_paths_by_status() {
        let data = b":000000 100644 0000000000000000000000000000000000000000 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa A\0new.rs\0:100644 000000 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 0000000000000000000000000000000000000000 D\0old.rs\0";
        let records = parse_raw_records(data);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].old_path, None);
        assert_eq!(records[0].new_path, "new.rs");
        assert_eq!(records[1].old_path.as_deref(), Some("old.rs"));
        assert_eq!(records[1].new_path, "old.rs");
    }

    #[test]
    fn merge_numstat_keeps_raw_file_identity() {
        let raw = parse_raw_records(
            b":100644 100644 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb M\0src/main.rs\0",
        );
        let stats = parse_numstat("7\t2\tsrc/main.rs\n");
        let merged = merge_numstat(raw, stats);
        assert_eq!(merged[0].added, Some(7));
        assert_eq!(merged[0].deleted, Some(2));
        assert_eq!(merged[0].status, FileChangeStatus::Modified);
    }

    #[test]
    fn numstat_expands_braced_rename_paths() {
        let files = parse_numstat("1\t0\tsrc/{old => new}.rs\n");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path.as_deref(), Some("src/old.rs"));
        assert_eq!(files[0].new_path, "src/new.rs");
        assert_eq!(files[0].path, "src/old.rs => src/new.rs");
    }

    #[test]
    fn merge_numstat_matches_expanded_rename_paths() {
        let raw = parse_raw_records(
            b":100644 100644 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb R100\0src/old.rs\0src/new.rs\0",
        );
        let stats = parse_numstat("1\t0\tsrc/{old => new}.rs\n");
        let merged = merge_numstat(raw, stats);
        assert_eq!(merged[0].added, Some(1));
        assert_eq!(merged[0].deleted, Some(0));
    }

    #[test]
    fn merge_diff_fixture_combines_raw_records_and_numstat() {
        let raw = parse_raw_records(
            b":100644 100644 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb M\0src/merge.rs\0:000000 100644 0000000000000000000000000000000000000000 cccccccccccccccccccccccccccccccccccccccc A\0src/incoming.rs\0",
        );
        let stats =
            parse_numstat("2\t1\tsrc/merge.rs\n4\t0\tsrc/incoming.rs\n");
        let merged = merge_numstat(raw, stats);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].path, "src/merge.rs");
        assert_eq!(merged[0].added, Some(2));
        assert_eq!(merged[0].deleted, Some(1));
        assert_eq!(merged[1].status, FileChangeStatus::Added);
        assert_eq!(merged[1].added, Some(4));
        assert_eq!(merged[1].deleted, Some(0));
    }

    #[test]
    fn numstat_decodes_quoted_control_character_paths() {
        let files = parse_numstat("1\t0\t\"tab\\tname.rs\"\n");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "tab\tname.rs");
    }
}
