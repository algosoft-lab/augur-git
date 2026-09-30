//! Embedded translation catalogs and locale selection.
//!
//! The catalogs are `.ftl` files compiled into the binary. The same catalogs
//! are handed to the webview at startup so the interface and the native menu
//! bar can never disagree about a translation.
//!
//! Placeholders use the `{ $name }` spelling. A key missing from the active
//! catalog falls back to English and then to the key itself, so an incomplete
//! translation degrades to readable text instead of an empty control.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::LanguagePreference;

const ENGLISH: &str = "en-US";
const SIMPLIFIED_CHINESE: &str = "zh-CN";

const ENGLISH_TRANSLATIONS: &str = include_str!("../i18n/en-US.ftl");
const SIMPLIFIED_CHINESE_TRANSLATIONS: &str = include_str!("../i18n/zh-CN.ftl");

/// A locale embedded in the application binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    English,
    SimplifiedChinese,
}

impl Locale {
    /// Every locale the application ships, in settings-list order.
    pub const ALL: [Locale; 2] = [Locale::English, Locale::SimplifiedChinese];

    /// Stable language identifier used by resources and logs.
    pub const fn id(self) -> &'static str {
        match self {
            Self::English => ENGLISH,
            Self::SimplifiedChinese => SIMPLIFIED_CHINESE,
        }
    }
}

/// Resolve a persisted language preference to a concrete locale.
pub fn resolve(preference: &LanguagePreference) -> Locale {
    match preference {
        LanguagePreference::System => detect_system_language(),
        LanguagePreference::English => Locale::English,
        LanguagePreference::SimplifiedChinese => Locale::SimplifiedChinese,
    }
}

/// Detect the system language, falling back to English when unsupported.
pub fn detect_system_language() -> Locale {
    let Some(language) = sys_locale::get_locale()
        .and_then(|locale| locale.split(['-', '_']).next().map(str::to_lowercase))
    else {
        return Locale::English;
    };

    if language == "zh" {
        Locale::SimplifiedChinese
    } else {
        Locale::English
    }
}

/// Return translated text, falling back to English and then to the key.
pub fn text(locale: Locale, key: &str) -> String {
    text_args(locale, key, &[])
}

/// Return translated text with `{ $name }` placeholders substituted.
pub fn text_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let template = translations(locale)
        .get(key)
        .or_else(|| translations(Locale::English).get(key))
        .copied()
        .unwrap_or(key);
    args.iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{ ${name} }}"), value)
        })
}

/// Owned copy of one catalog, used to hand translations to the webview.
pub fn catalog(locale: Locale) -> HashMap<String, String> {
    translations(locale)
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn translations(locale: Locale) -> &'static HashMap<&'static str, &'static str> {
    static ENGLISH_CATALOG: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    static CHINESE_CATALOG: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

    match locale {
        Locale::English => ENGLISH_CATALOG.get_or_init(|| parse_catalog(ENGLISH_TRANSLATIONS)),
        Locale::SimplifiedChinese => {
            CHINESE_CATALOG.get_or_init(|| parse_catalog(SIMPLIFIED_CHINESE_TRANSLATIONS))
        }
    }
}

fn parse_catalog(source: &'static str) -> HashMap<&'static str, &'static str> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim(), value.trim()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGLISH_FALLBACK_KEYS: &[&str] = &[
        "auto-check-updates",
        "auto-refresh-title",
        "check-for-updates",
        "diff-image-absent",
        "diff-image-after",
        "diff-image-before",
        "diff-image-loading",
        "diff-image-mode",
        "diff-image-too-large",
        "diff-image-unavailable",
        "diff-image-unsupported",
        "diff-soft-wrap",
        "diff-text-mode",
        "download-update",
        "graph-history-current-short",
        "homebrew-copied",
        "homebrew-copy-upgrade",
        "homebrew-upgrade-hint",
        "install-update",
        "menu-more",
        "open-release-page",
        "review-update",
        "sidecar-back",
        "sidecar-changes",
        "sidecar-conflicts",
        "sidecar-diff",
        "sidecar-diff-file",
        "sidecar-graph-lanes-scroll",
        "sidecar-history",
        "sidecar-navigation",
        "sidecar-select-repository",
        "toolbar-ahead",
        "toolbar-behind",
        "update-notice-description",
        "update-notice-title",
        "update-status-available",
        "update-status-checking",
        "update-status-downloaded",
        "update-status-downloading",
        "update-status-error",
        "update-status-idle",
        "update-status-up-to-date",
        "updates-title",
    ];

    #[test]
    fn translates_and_falls_back_to_english() {
        assert_eq!(text(Locale::English, "toolbar-refresh"), "Refresh");
        assert_eq!(text(Locale::SimplifiedChinese, "toolbar-refresh"), "刷新");
        assert_eq!(text(Locale::English, "menu-view"), "View");
        assert_eq!(text(Locale::SimplifiedChinese, "menu-view"), "视图");
        assert_eq!(text(Locale::English, "menu-appearance"), "Themes…");
        assert_eq!(text(Locale::SimplifiedChinese, "menu-appearance"), "主题…");
        assert_eq!(text(Locale::English, "menu-branch"), "Branch");
        assert_eq!(text(Locale::SimplifiedChinese, "menu-branch"), "分支");
        assert_eq!(
            text(Locale::SimplifiedChinese, "sidecar-switch-mode"),
            "切换到 Sidecar 模式"
        );
        assert_eq!(text(Locale::English, "missing-key"), "missing-key");
    }

    #[test]
    fn catalog_keys_are_translated_or_use_the_english_fallback() {
        let english = translations(Locale::English);
        let chinese = translations(Locale::SimplifiedChinese);

        assert_eq!(english.len(), chinese.len() + ENGLISH_FALLBACK_KEYS.len());
        for key in english.keys() {
            assert!(
                chinese.contains_key(key) || ENGLISH_FALLBACK_KEYS.contains(key),
                "missing Chinese translation without an English fallback: {key}"
            );
        }
        for key in ENGLISH_FALLBACK_KEYS {
            assert!(!chinese.contains_key(key));
            assert_eq!(
                text(Locale::SimplifiedChinese, key),
                text(Locale::English, key)
            );
        }
    }

    #[test]
    fn substitutes_named_arguments() {
        assert_eq!(
            text_args(
                Locale::English,
                "command-success",
                &[("label", "fetch --all")]
            ),
            "fetch --all succeeded"
        );
        assert_eq!(
            text_args(Locale::SimplifiedChinese, "rel-min", &[("n", "5")]),
            "5 分钟前"
        );
    }

    #[test]
    fn system_language_is_supported() {
        assert!(matches!(
            detect_system_language(),
            Locale::English | Locale::SimplifiedChinese
        ));
    }

    #[test]
    fn exported_catalog_matches_the_binary_lookup() {
        let exported = catalog(Locale::SimplifiedChinese);
        assert_eq!(
            exported.get("toolbar-refresh").map(String::as_str),
            Some("刷新")
        );
        assert!(!exported.contains_key("auto-refresh-title"));
        assert_eq!(
            text(Locale::SimplifiedChinese, "auto-refresh-title"),
            "Auto refresh selected tab"
        );
    }

    #[test]
    fn deliberately_removed_keys_stay_removed() {
        // Keys dropped from this catalog on purpose, each for a recorded
        // reason: `no-repo-open` was superseded by `status-no-repo-selected`;
        // `err-git` by `err-git-run`; `status-scanning` by
        // `status-scanning-at`, which names the repository; `err-invalid-location`, `err-compare`,
        // and `shortcut-app-quit-reset` never had a producer here; and the
        // `workspace-close-*` card only ever renders in the reference's agent
        // build, which this product has no counterpart of. Re-adding one of
        // these should come with a use, not by accident. `auto-refresh-on-focus-title`
        // was replaced by the English-fallback `auto-refresh-title` copy.
        for key in [
            "no-repo-open",
            "err-git",
            "err-invalid-location",
            "err-compare",
            "shortcut-app-quit-reset",
            "status-scanning",
            "workspace-close-title",
            "workspace-close-warning",
            "workspace-close-cancel",
            "workspace-close-confirm",
            "auto-refresh-on-focus-title",
        ] {
            assert_eq!(text(Locale::English, key), key, "key {key} is back");
            assert_eq!(
                text(Locale::SimplifiedChinese, key),
                key,
                "key {key} is back"
            );
        }
    }
}
