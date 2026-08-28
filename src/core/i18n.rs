//! Compile-time embedded translations and locale selection.
//!
//! Translation catalogs live in `.ftl` files under the repository's `i18n/`
//! directory. Missing keys fall back to English, then to the key itself.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::core::config::LanguagePreference;

const ENGLISH: &str = "en-US";
const SIMPLIFIED_CHINESE: &str = "zh-CN";

const ENGLISH_TRANSLATIONS: &str = include_str!("../../i18n/en-US.ftl");
const SIMPLIFIED_CHINESE_TRANSLATIONS: &str =
    include_str!("../../i18n/zh-CN.ftl");

/// A locale embedded in the application binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    English,
    SimplifiedChinese,
}

impl Locale {
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
    let Some(language) = sys_locale::get_locale().and_then(|locale| {
        locale.split(['-', '_']).next().map(str::to_lowercase)
    }) else {
        return Locale::English;
    };

    if language == "zh" {
        Locale::SimplifiedChinese
    } else {
        Locale::English
    }
}

/// Return translated text, falling back to English and then the key itself.
pub fn text(locale: Locale, key: &str) -> String {
    text_args(locale, key, &[])
}

/// Return translated text with `{ $name }` placeholders substituted.
pub fn text_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let catalog = translations(locale);
    let template = catalog
        .get(key)
        .or_else(|| translations(Locale::English).get(key))
        .copied()
        .unwrap_or(key);

    args.iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{ ${name} }}"), value)
        })
}

fn translations(
    locale: Locale,
) -> &'static HashMap<&'static str, &'static str> {
    static ENGLISH_CATALOG: OnceLock<HashMap<&'static str, &'static str>> =
        OnceLock::new();
    static CHINESE_CATALOG: OnceLock<HashMap<&'static str, &'static str>> =
        OnceLock::new();

    match locale {
        Locale::English => {
            ENGLISH_CATALOG.get_or_init(|| parse_catalog(ENGLISH_TRANSLATIONS))
        }
        Locale::SimplifiedChinese => CHINESE_CATALOG
            .get_or_init(|| parse_catalog(SIMPLIFIED_CHINESE_TRANSLATIONS)),
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
    use super::{
        Locale, detect_system_language, text, text_args, translations,
    };

    #[test]
    fn translates_and_falls_back_to_english() {
        assert_eq!(text(Locale::English, "toolbar-refresh"), "Refresh");
        assert_eq!(text(Locale::SimplifiedChinese, "toolbar-refresh"), "刷新");
        assert_eq!(text(Locale::English, "missing-key"), "missing-key");
    }

    #[test]
    fn commit_placeholder_is_compact() {
        assert_eq!(
            text(Locale::English, "commit-placeholder"),
            "Commit Message"
        );
        assert_eq!(
            text(Locale::SimplifiedChinese, "commit-placeholder"),
            "提交信息"
        );
    }

    #[test]
    fn catalogs_define_the_same_keys() {
        let english = translations(Locale::English);
        let chinese = translations(Locale::SimplifiedChinese);

        assert_eq!(english.len(), chinese.len());
        for key in english.keys() {
            assert!(chinese.contains_key(key), "missing Chinese key: {key}");
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
}
