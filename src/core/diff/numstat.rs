//! Parser for NUL-delimited Git numstat output.

use super::{FileChange, FileChangeStatus};

/// Parse NUL-delimited numstat records without Git's path quoting.
///
/// With `--find-renames -z`, a rename is represented as one count record with
/// an empty path field, followed by the old and new paths as separate
/// NUL-delimited fields. A normal record carries its path in the count record.
pub(crate) fn parse_numstat_z(data: &[u8]) -> Vec<FileChange> {
    let fields: Vec<&[u8]> = data.split(|byte| *byte == 0).collect();
    let mut index = 0;
    let mut changes = Vec::new();

    while index < fields.len() {
        let record = fields[index];
        index += 1;
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(3, |byte| *byte == b'\t');
        let Some(added_token) = parts.next() else {
            continue;
        };
        let Some(deleted_token) = parts.next() else {
            continue;
        };
        let Some(encoded_path) = parts.next() else {
            continue;
        };

        let added = parse_count(added_token);
        let deleted = parse_count(deleted_token);
        let (old_path, new_path, status) = if encoded_path.is_empty() {
            let Some(old_path) = fields.get(index).copied() else {
                continue;
            };
            let Some(new_path) = fields.get(index + 1).copied() else {
                continue;
            };
            index += 2;
            let old_path = String::from_utf8_lossy(old_path).into_owned();
            let new_path = String::from_utf8_lossy(new_path).into_owned();
            if old_path.is_empty() || new_path.is_empty() {
                continue;
            }
            (Some(old_path), new_path, FileChangeStatus::Renamed)
        } else {
            (
                None,
                String::from_utf8_lossy(encoded_path).into_owned(),
                FileChangeStatus::Modified,
            )
        };
        let path = old_path
            .as_ref()
            .map(|old| format!("{old} => {new_path}"))
            .unwrap_or_else(|| new_path.clone());
        changes.push(FileChange {
            path,
            old_path,
            new_path,
            status,
            old_blob: None,
            new_blob: None,
            added,
            deleted,
        });
    }
    changes
}

fn parse_count(token: &[u8]) -> Option<usize> {
    if token == b"-" {
        None
    } else {
        String::from_utf8_lossy(token).parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unicode_special_and_rename_paths() {
        let data =
            "4\t1\t\0old name.txt\0new name-中.txt\0-\t-\tassets/logo.png\0";
        let records = parse_numstat_z(data.as_bytes());
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].old_path.as_deref(), Some("old name.txt"));
        assert_eq!(records[0].new_path, "new name-中.txt");
        assert_eq!(records[0].status, FileChangeStatus::Renamed);
        assert_eq!(records[0].added, Some(4));
        assert_eq!(records[0].deleted, Some(1));
        assert!(records[1].is_binary());
        assert_eq!(records[1].new_path, "assets/logo.png");
    }
}
