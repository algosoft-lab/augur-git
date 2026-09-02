//! Option-list builders shared by the settings selects.
//!
//! Each function maps persisted preferences to localized list items.

use crate::core::config::{
    DiffLayoutPreference, GraphHistoryPreference, LanguagePreference,
    ThemePreference,
};
use crate::core::i18n::{self, Locale};
use gpui_component::IndexPath;

use super::SettingsOption;

pub(super) fn selected_index<T: Clone + PartialEq>(
    options: &[SettingsOption<T>],
    value: &T,
) -> Option<IndexPath> {
    options
        .iter()
        .position(|option| &option.value == value)
        .map(|index| IndexPath::default().row(index))
}

pub(super) fn language_options(
    locale: Locale,
) -> Vec<SettingsOption<LanguagePreference>> {
    vec![
        SettingsOption::new(
            LanguagePreference::System,
            i18n::text(locale, "language-system"),
        ),
        SettingsOption::new(
            LanguagePreference::SimplifiedChinese,
            i18n::text(locale, "language-chinese"),
        ),
        SettingsOption::new(
            LanguagePreference::English,
            i18n::text(locale, "language-english"),
        ),
    ]
}

pub(super) fn theme_options(
    locale: Locale,
) -> Vec<SettingsOption<ThemePreference>> {
    vec![
        SettingsOption::new(
            ThemePreference::GitHubDark,
            i18n::text(locale, "theme-github-dark"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinLatte,
            i18n::text(locale, "theme-catppuccin-latte"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinFrappe,
            i18n::text(locale, "theme-catppuccin-frappe"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinMacchiato,
            i18n::text(locale, "theme-catppuccin-macchiato"),
        ),
        SettingsOption::new(
            ThemePreference::CatppuccinMocha,
            i18n::text(locale, "theme-catppuccin-mocha"),
        ),
    ]
}

pub(super) fn diff_layout_options(
    locale: Locale,
) -> Vec<SettingsOption<DiffLayoutPreference>> {
    vec![
        SettingsOption::new(
            DiffLayoutPreference::SideBySide,
            i18n::text(locale, "diff-layout-side-by-side"),
        ),
        SettingsOption::new(
            DiffLayoutPreference::Inline,
            i18n::text(locale, "diff-layout-inline"),
        ),
    ]
}

pub(super) fn graph_history_options(
    locale: Locale,
) -> Vec<SettingsOption<GraphHistoryPreference>> {
    vec![
        SettingsOption::new(
            GraphHistoryPreference::CurrentBranch,
            i18n::text(locale, "graph-history-current"),
        ),
        SettingsOption::new(
            GraphHistoryPreference::AllBranches,
            i18n::text(locale, "graph-history-all"),
        ),
    ]
}

pub(super) fn auto_refresh_options(
    locale: Locale,
) -> Vec<SettingsOption<bool>> {
    vec![
        SettingsOption::new(true, i18n::text(locale, "setting-enabled")),
        SettingsOption::new(false, i18n::text(locale, "setting-disabled")),
    ]
}

pub(super) fn font_options(
    locale: Locale,
    families: &[String],
) -> Vec<SettingsOption<Option<String>>> {
    let mut options = Vec::with_capacity(families.len() + 1);
    options.push(SettingsOption::new(
        None,
        i18n::text(locale, "font-system-default"),
    ));
    options.extend(
        families
            .iter()
            .cloned()
            .map(|family| SettingsOption::new(Some(family.clone()), family)),
    );
    options
}
