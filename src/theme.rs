//! Embedded theme registry, runtime switching, and theme-dependent commit-graph
//! lane colors that have no gpui-component token counterpart.
//!
//! Lives outside `core/` because it is presentation-layer: it wires gpui and
//! gpui-component globals rather than pure domain logic.

use gpui::{App, Hsla, Rgba, SharedString, rgb};
use gpui_component::theme::{Theme, ThemeRegistry};

use crate::core::config::{ThemePreference, TypographySettings};

const THEMES_JSON: &str = include_str!("../assets/themes/augur-themes.json");

/// Register the embedded themes and apply `preference`. Call once at startup,
/// after `gpui_component::init` and after a `Theme::change` call has created
/// the `Theme` global (the registry observer reads it).
pub fn init(
    preference: ThemePreference,
    typography: &TypographySettings,
    cx: &mut App,
) {
    if let Err(error) =
        ThemeRegistry::global_mut(cx).load_themes_from_str(THEMES_JSON)
    {
        log::warn!("[theme] failed to load embedded themes: {error}");
    }
    apply(preference, typography, cx);
}

/// Switch to `preference` at runtime and refresh all windows.
pub fn apply(
    preference: ThemePreference,
    typography: &TypographySettings,
    cx: &mut App,
) {
    let Some(config) = ThemeRegistry::global(cx)
        .themes()
        .get(preference.registry_name())
        .cloned()
    else {
        log::warn!(
            "[theme] theme not registered: {}",
            preference.registry_name()
        );
        return;
    };
    let mut config = (*config).clone();
    config.font_family =
        typography.ui_font_family.clone().map(SharedString::from);
    config.mono_font_family =
        typography.mono_font_family.clone().map(SharedString::from);
    let mode = config.mode;
    let theme = Theme::global_mut(cx);
    if mode.is_dark() {
        theme.dark_theme = config.into();
    } else {
        theme.light_theme = config.into();
    }
    Theme::change(mode, None, cx);
    cx.refresh_windows();
    log::info!("[theme] applied theme: {}", preference.registry_name());
}

/// Preference matching the currently active theme (unknown names fall back to
/// the default). Lets render code resolve theme accents without extra state.
pub fn active_preference(cx: &App) -> ThemePreference {
    match Theme::global(cx).theme_name().as_ref() {
        "Catppuccin Latte" => ThemePreference::CatppuccinLatte,
        "Catppuccin Frappé" => ThemePreference::CatppuccinFrappe,
        "Catppuccin Macchiato" => ThemePreference::CatppuccinMacchiato,
        "Catppuccin Mocha" => ThemePreference::CatppuccinMocha,
        _ => ThemePreference::GitHubDark,
    }
}

/// Commit-graph lane palette for the active theme.
pub fn lane_colors(cx: &App) -> [Hsla; 10] {
    lanes(active_preference(cx))
}

/// Normalize rgitui-style `hsl(h°, s%, l%)` values: this gpui fork takes
/// 0..1 normalized `hsla` arguments, so raw degrees would clamp to plain
/// white.
fn hsla(h: f32, s: f32, l: f32, a: f32) -> Hsla {
    Hsla {
        h: h / 360.0,
        s: s / 100.0,
        l: l / 100.0,
        a,
    }
}

/// Pure per-theme lane tables, testable without an `App`.
fn lanes(preference: ThemePreference) -> [Hsla; 10] {
    match preference {
        // Keep the primary lane blue, matching the Git graph accent used by
        // VS Code, while retaining the existing theme-specific palette.
        ThemePreference::GitHubDark => [
            hsla(217.0, 92.0, 65.0, 1.0),
            hsla(267.0, 84.0, 75.0, 1.0),
            hsla(115.0, 60.0, 65.0, 1.0),
            hsla(23.0, 92.0, 65.0, 1.0),
            hsla(343.0, 81.0, 65.0, 1.0),
            hsla(170.0, 65.0, 60.0, 1.0),
            hsla(41.0, 86.0, 70.0, 1.0),
            hsla(189.0, 75.0, 60.0, 1.0),
            hsla(316.0, 72.0, 72.0, 1.0),
            hsla(10.0, 70.0, 75.0, 1.0),
        ],
        ThemePreference::CatppuccinLatte => accent_lanes([
            0x1E66F5, 0x8839EF, 0x40A02B, 0xFE640B, 0xD20F39, 0x179299,
            0xDF8E1D, 0x04A5E5, 0xEA76CB, 0xE64553,
        ]),
        ThemePreference::CatppuccinFrappe => accent_lanes([
            0x8CAAEE, 0xCA9EE6, 0xA6D189, 0xEF9F76, 0xE78284, 0x81C8BE,
            0xE5C890, 0x99D1DB, 0xF4B8E4, 0xEA999C,
        ]),
        ThemePreference::CatppuccinMacchiato => accent_lanes([
            0x8AADF4, 0xC6A0F6, 0xA6DA95, 0xF5A97F, 0xED8796, 0x8BD5CA,
            0xEED49F, 0x91D7E3, 0xF5BDE6, 0xEE99A0,
        ]),
        ThemePreference::CatppuccinMocha => accent_lanes([
            0x89B4FA, 0xCBA6F7, 0xA6E3A1, 0xFAB387, 0xF38BA8, 0x94E2D5,
            0xF9E2AF, 0x89DCEB, 0xF5C2E7, 0xEBA0AC,
        ]),
    }
}

/// Catppuccin lane rotation:
/// [blue, mauve, green, peach, red, teal, yellow, sky, pink, maroon].
fn accent_lanes(accent: [u32; 10]) -> [Hsla; 10] {
    std::array::from_fn(|i| Hsla::from(rgb(accent[i])))
}

/// Readable author-initials text color for the filled HEAD graph node: dark
/// text on a light fill, light text on a dark fill. The crossover is the
/// black/white contrast crossover, `sqrt(0.05 * 1.05) - 0.05 ≈ 0.179` in WCAG
/// relative luminance, the point where either choice yields the same ratio.
pub fn initials_text_color(fill: Hsla) -> Hsla {
    if relative_luminance(fill) > 0.179 {
        Hsla::from(rgb(0x000000))
    } else {
        Hsla::from(rgb(0xFFFFFF))
    }
}

/// WCAG relative luminance of `color` with sRGB channels gamma-expanded.
fn relative_luminance(color: Hsla) -> f32 {
    let rgba = Rgba::from(color);
    let linear = |channel: f32| {
        if channel <= 0.03928 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_themes_cover_all_preferences() {
        let set: serde_json::Value = serde_json::from_str(THEMES_JSON).unwrap();
        let themes = set["themes"].as_array().unwrap();
        for preference in [
            ThemePreference::GitHubDark,
            ThemePreference::CatppuccinLatte,
            ThemePreference::CatppuccinFrappe,
            ThemePreference::CatppuccinMacchiato,
            ThemePreference::CatppuccinMocha,
        ] {
            let theme = themes
                .iter()
                .find(|t| t["name"] == preference.registry_name())
                .unwrap_or_else(|| {
                    panic!("missing theme {}", preference.registry_name())
                });
            let expected_mode = match preference {
                ThemePreference::CatppuccinLatte => "light",
                _ => "dark",
            };
            assert_eq!(
                theme["mode"].as_str().unwrap(),
                expected_mode,
                "wrong mode for {}",
                preference.registry_name()
            );
            assert!(
                theme["colors"]["background"].is_string(),
                "missing background for {}",
                preference.registry_name()
            );
            // Every theme hovers menus and lists on an accent background; the
            // label stays readable only if the accent foreground is paired
            // with it explicitly (the upstream fallback is the theme
            // foreground, which clashes on both light and dark accents).
            assert!(
                theme["colors"]["accent.background"].is_string()
                    && theme["colors"]["accent.foreground"].is_string(),
                "missing accent color pair for {}",
                preference.registry_name()
            );
        }
    }

    #[test]
    fn github_dark_reproduces_startup_overrides() {
        // Regression guard for the migration from the hardcoded startup
        // palette: key spellings must stay exact (unknown keys would be
        // silently ignored by the theme loader).
        let set: serde_json::Value = serde_json::from_str(THEMES_JSON).unwrap();
        let github_dark = set["themes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "GitHub Dark")
            .unwrap();
        let colors = github_dark["colors"].as_object().unwrap();
        let expected: &[(&str, &str)] = &[
            ("background", "#0D1117"),
            ("foreground", "#E6EDF3"),
            ("border", "#30363D"),
            ("tab_bar.background", "#161B22"),
            ("title_bar.background", "#161B22"),
            ("input.border", "#21262D"),
            ("list.hover.background", "#21262D"),
            ("list.active.background", "#264F78"),
            ("muted.foreground", "#8B949E"),
            ("table.head.foreground", "#8B949E"),
            ("base.blue", "#2F81F7"),
            ("accent.background", "#2F81F7"),
            // Menu and list rows hover with an accent background but keep the
            // theme foreground unless this is set, so the label becomes
            // unreadable on dark themes (white-on-blue) and light themes
            // (dark-on-dark-blue). Pair it with the theme background instead.
            ("accent.foreground", "#0D1117"),
            ("base.green", "#3FB950"),
            ("base.red", "#F85149"),
            ("warning.background", "#D29922"),
            ("drag.border", "#388BFD"),
            // Without these, gpui-component falls back to a near-white
            // primary background and the theme foreground, rendering
            // primary buttons as white-on-white.
            ("primary.background", "#2F81F7"),
            ("primary.foreground", "#FFFFFF"),
            // The switch tokens default to secondary_active / background,
            // which on dark themes blend into the surrounding panel and make
            // the revision-picker toggle invisible.
            ("switch.background", "#3D444D"),
            ("switch.thumb.background", "#FFFFFF"),
        ];
        assert_eq!(
            colors.len(),
            expected.len(),
            "GitHub Dark token set changed"
        );
        for (key, value) in expected {
            assert_eq!(
                colors.get(*key).and_then(|v| v.as_str()),
                Some(*value),
                "unexpected value for key {key}"
            );
        }
    }

    #[test]
    fn lanes_are_distinct_per_theme() {
        for preference in [
            ThemePreference::GitHubDark,
            ThemePreference::CatppuccinLatte,
            ThemePreference::CatppuccinFrappe,
            ThemePreference::CatppuccinMacchiato,
            ThemePreference::CatppuccinMocha,
        ] {
            let lanes = lanes(preference);
            assert_eq!(lanes.len(), 10);
            let keys: Vec<(f32, f32, f32)> =
                lanes.iter().map(|c| (c.h, c.s, c.l)).collect();
            for i in 0..keys.len() {
                for j in (i + 1)..keys.len() {
                    assert_ne!(
                        keys[i], keys[j],
                        "duplicate lane color in {preference:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn initials_text_color_flips_with_fill_luminance() {
        // GitHub Dark primary lane: light blue fill needs dark initials.
        assert_eq!(
            initials_text_color(hsla(217.0, 92.0, 65.0, 1.0)),
            Hsla::from(rgb(0x000000))
        );
        // Catppuccin Latte primary lane: dark blue fill needs light initials.
        assert_eq!(
            initials_text_color(Hsla::from(rgb(0x1E66F5))),
            Hsla::from(rgb(0xFFFFFF))
        );
        // Extremes stay on the sane side.
        assert_eq!(
            initials_text_color(Hsla::from(rgb(0xFFFFFF))),
            Hsla::from(rgb(0x000000))
        );
        assert_eq!(
            initials_text_color(Hsla::from(rgb(0x000000))),
            Hsla::from(rgb(0xFFFFFF))
        );
    }

    #[test]
    fn primary_lane_is_blue_per_theme() {
        for preference in [
            ThemePreference::GitHubDark,
            ThemePreference::CatppuccinLatte,
            ThemePreference::CatppuccinFrappe,
            ThemePreference::CatppuccinMacchiato,
            ThemePreference::CatppuccinMocha,
        ] {
            let primary = lanes(preference)[0];
            assert!(
                (0.5..0.7).contains(&primary.h),
                "primary lane is not blue for {preference:?}: hue={}",
                primary.h
            );
        }
    }
}
