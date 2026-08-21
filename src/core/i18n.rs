//! 编译期内嵌的翻译目录与语言环境选择（镜像 augur-pdf/augur-term src/core/i18n.rs）。
//!
//! 文案在仓库根 i18n/ 下的 .ftl 文件（key = value 一行一条），
//! include_str! 编译期嵌入，缺失键回落英文、再缺失返回键本身。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::core::config::LanguagePreference;

const ENGLISH: &str = "en-US";
const SIMPLIFIED_CHINESE: &str = "zh-CN";

const ENGLISH_TRANSLATIONS: &str = include_str!("../../i18n/en-US.ftl");
const SIMPLIFIED_CHINESE_TRANSLATIONS: &str = include_str!("../../i18n/zh-CN.ftl");

/// 已编译进二进制的语言环境。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    English,
    SimplifiedChinese,
}

impl Locale {
    /// 资源与日志中使用的稳定语言标识。
    pub const fn id(self) -> &'static str {
        match self {
            Self::English => ENGLISH,
            Self::SimplifiedChinese => SIMPLIFIED_CHINESE,
        }
    }
}

/// 把持久化的语言偏好解析为具体语言环境。
pub fn resolve(preference: &LanguagePreference) -> Locale {
    match preference {
        LanguagePreference::System => detect_system_language(),
        LanguagePreference::English => Locale::English,
        LanguagePreference::SimplifiedChinese => Locale::SimplifiedChinese,
    }
}

/// 检测系统语言，不支持的语言回落到英文。
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

/// 返回翻译文本，缺失时回落英文，再缺失返回 key 本身。
pub fn text(locale: Locale, key: &str) -> String {
    text_args(locale, key, &[])
}

/// 返回带 `{ $name }` 占位符替换的翻译文本。
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
    use super::{Locale, detect_system_language, text, text_args, translations};

    #[test]
    fn translates_and_falls_back_to_english() {
        assert_eq!(text(Locale::English, "toolbar-refresh"), "Refresh");
        assert_eq!(text(Locale::SimplifiedChinese, "toolbar-refresh"), "刷新");
        assert_eq!(text(Locale::English, "missing-key"), "missing-key");
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
