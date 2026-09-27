//! Font family discovery for the appearance settings.
//!
//! The webview can only enumerate fonts it has already loaded, which is not
//! enough for a picker. This module walks the platform font directories and
//! returns the family names it can determine from the file names. The list is
//! a convenience: a family that is missing here can still be typed in, and an
//! unavailable family falls back to the platform default when the webview
//! applies it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Extensions treated as font files.
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];

/// Families available on this machine, sorted and de-duplicated.
pub fn font_families() -> Vec<String> {
    let mut names = BTreeSet::new();
    for directory in font_directories() {
        collect(&directory, 0, &mut names);
    }
    names.into_iter().collect()
}

fn font_directories() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut roots = vec![
            PathBuf::from("/System/Library/Fonts"),
            PathBuf::from("/Library/Fonts"),
            PathBuf::from("/Network/Library/Fonts"),
        ];
        if let Some(home) = dirs_home() {
            roots.push(home.join("Library").join("Fonts"));
        }
        return roots;
    }
    #[cfg(target_os = "windows")]
    {
        let mut roots = Vec::new();
        if let Ok(windir) = std::env::var("WINDIR") {
            roots.push(PathBuf::from(windir).join("Fonts"));
        }
        roots.push(PathBuf::from("C:\\Windows\\Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
        return roots;
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let mut roots = vec![
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
        ];
        if let Some(home) = dirs_home() {
            roots.push(home.join(".fonts"));
            roots.push(home.join(".local").join("share").join("fonts"));
        }
        roots
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Walk one directory tree, bounded so a deep or hostile tree cannot stall
/// startup.
fn collect(directory: &Path, depth: usize, names: &mut BTreeSet<String>) {
    const MAX_DEPTH: usize = 4;
    const MAX_ENTRIES: usize = 4_000;
    if depth > MAX_DEPTH || names.len() > MAX_ENTRIES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, depth + 1, names);
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !FONT_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
            continue;
        }
        if let Some(family) = path.file_stem().and_then(|value| value.to_str()) {
            // "SF-Pro-Text" and "SF_Pro_Text" both name a readable family.
            let spaced = family.replace(['-', '_'], " ");
            let cleaned = spaced
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string();
            if !cleaned.is_empty() {
                names.insert(cleaned);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_font_list_is_sorted_and_unique() {
        let families = font_families();
        let mut sorted = families.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(families, sorted);
    }

    #[test]
    fn a_missing_directory_is_skipped_silently() {
        let mut names = BTreeSet::new();
        collect(Path::new("/definitely/not/a/font/directory"), 0, &mut names);
        assert!(names.is_empty());
    }

    #[test]
    fn recursion_stops_at_the_depth_limit() {
        // A path this deep does not exist, but the call must return promptly
        // instead of recursing without bound.
        let mut names = BTreeSet::new();
        collect(Path::new("/"), 99, &mut names);
        assert!(names.is_empty());
    }
}
