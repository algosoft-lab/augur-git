use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, IndexPath,
    button::{Button, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    searchable_list::{SearchableListItem, SearchableVec},
    select::{Select, SelectEvent, SelectState},
    v_flex,
};

use crate::core::config::{
    AppConfig, DiffLayoutPreference, GraphHistoryPreference,
    LanguagePreference, ThemePreference,
};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

#[derive(Clone, Debug)]
pub enum SettingsPanelEvent {
    Close,
    LanguageChanged(LanguagePreference),
    AutoRefreshOnFocusChanged(bool),
    ThemeChanged(ThemePreference),
    DiffLayoutChanged(DiffLayoutPreference),
    GraphHistoryChanged(GraphHistoryPreference),
    UiFontChanged(Option<String>),
    MonoFontChanged(Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSection {
    General,
    Appearance,
    Layout,
}

#[derive(Clone, Debug)]
struct SettingsOption<T: Clone + PartialEq> {
    value: T,
    label: SharedString,
}

impl<T: Clone + PartialEq> SettingsOption<T> {
    fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}

impl<T: Clone + PartialEq> SearchableListItem for SettingsOption<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

pub struct SettingsPanel {
    locale: Locale,
    section: SettingsSection,
    language: LanguagePreference,
    auto_refresh_on_focus: bool,
    theme: ThemePreference,
    diff_layout: DiffLayoutPreference,
    graph_history: GraphHistoryPreference,
    ui_font: Option<String>,
    mono_font: Option<String>,
    font_families: Vec<String>,
    language_state:
        Entity<SelectState<Vec<SettingsOption<LanguagePreference>>>>,
    auto_refresh_state: Entity<SelectState<Vec<SettingsOption<bool>>>>,
    theme_state: Entity<SelectState<Vec<SettingsOption<ThemePreference>>>>,
    diff_layout_state:
        Entity<SelectState<Vec<SettingsOption<DiffLayoutPreference>>>>,
    graph_history_state:
        Entity<SelectState<Vec<SettingsOption<GraphHistoryPreference>>>>,
    ui_font_state:
        Entity<SelectState<SearchableVec<SettingsOption<Option<String>>>>>,
    mono_font_state:
        Entity<SelectState<SearchableVec<SettingsOption<Option<String>>>>>,
}

impl EventEmitter<SettingsPanelEvent> for SettingsPanel {}

impl SettingsPanel {
    pub fn new(
        config: &AppConfig,
        font_families: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let locale = i18n::resolve(&config.language);
        let language = config.language;
        let auto_refresh_on_focus = config.view.auto_refresh_on_focus;
        let theme = config.theme;
        let diff_layout = config.view.diff_layout;
        let graph_history = config.view.graph_history;
        let ui_font = config.typography.ui_font_family.clone();
        let mono_font = config.typography.mono_font_family.clone();

        let language_state = cx.new(|cx| {
            SelectState::new(
                language_options(locale),
                selected_index(&language_options(locale), &language),
                window,
                cx,
            )
        });
        let auto_refresh_state = cx.new(|cx| {
            SelectState::new(
                auto_refresh_options(locale),
                selected_index(
                    &auto_refresh_options(locale),
                    &auto_refresh_on_focus,
                ),
                window,
                cx,
            )
        });
        let theme_state = cx.new(|cx| {
            SelectState::new(
                theme_options(locale),
                selected_index(&theme_options(locale), &theme),
                window,
                cx,
            )
        });
        let diff_layout_state = cx.new(|cx| {
            SelectState::new(
                diff_layout_options(locale),
                selected_index(&diff_layout_options(locale), &diff_layout),
                window,
                cx,
            )
        });
        let graph_history_state = cx.new(|cx| {
            SelectState::new(
                graph_history_options(locale),
                selected_index(&graph_history_options(locale), &graph_history),
                window,
                cx,
            )
        });
        let ui_font_state = cx.new(|cx| {
            let options = font_options(locale, &font_families);
            SelectState::new(
                SearchableVec::from(options.clone()),
                selected_index(&options, &ui_font),
                window,
                cx,
            )
            .searchable(true)
        });
        let mono_font_state = cx.new(|cx| {
            let options = font_options(locale, &font_families);
            SelectState::new(
                SearchableVec::from(options.clone()),
                selected_index(&options, &mono_font),
                window,
                cx,
            )
            .searchable(true)
        });

        let panel = Self {
            locale,
            section: SettingsSection::General,
            language,
            auto_refresh_on_focus,
            theme,
            diff_layout,
            graph_history,
            ui_font,
            mono_font,
            font_families,
            language_state,
            auto_refresh_state,
            theme_state,
            diff_layout_state,
            graph_history_state,
            ui_font_state,
            mono_font_state,
        };

        let language_state_for_events = panel.language_state.clone();
        cx.subscribe(&language_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.language = *value;
            cx.emit(SettingsPanelEvent::LanguageChanged(*value));
        })
        .detach();

        let auto_refresh_state_for_events = panel.auto_refresh_state.clone();
        cx.subscribe(&auto_refresh_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.auto_refresh_on_focus = *value;
            cx.emit(SettingsPanelEvent::AutoRefreshOnFocusChanged(*value));
        })
        .detach();

        let theme_state_for_events = panel.theme_state.clone();
        cx.subscribe(&theme_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.theme = *value;
            cx.emit(SettingsPanelEvent::ThemeChanged(*value));
        })
        .detach();

        let diff_layout_state_for_events = panel.diff_layout_state.clone();
        cx.subscribe(&diff_layout_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.diff_layout = *value;
            cx.emit(SettingsPanelEvent::DiffLayoutChanged(*value));
        })
        .detach();

        let graph_history_state_for_events = panel.graph_history_state.clone();
        cx.subscribe(&graph_history_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.graph_history = *value;
            cx.emit(SettingsPanelEvent::GraphHistoryChanged(*value));
        })
        .detach();

        let ui_font_state_for_events = panel.ui_font_state.clone();
        cx.subscribe(&ui_font_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.ui_font = value.clone();
            cx.emit(SettingsPanelEvent::UiFontChanged(value.clone()));
        })
        .detach();

        let mono_font_state_for_events = panel.mono_font_state.clone();
        cx.subscribe(&mono_font_state_for_events, |panel, _, event, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            panel.mono_font = value.clone();
            cx.emit(SettingsPanelEvent::MonoFontChanged(value.clone()));
        })
        .detach();

        panel
    }

    pub fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        let language = self.language;
        let auto_refresh_on_focus = self.auto_refresh_on_focus;
        let theme = self.theme;
        let diff_layout = self.diff_layout;
        let graph_history = self.graph_history;
        let ui_font = self.ui_font.clone();
        let mono_font = self.mono_font.clone();
        let fonts = self.font_families.clone();

        self.language_state.update(cx, |state, cx| {
            let options = language_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&language, window, cx);
        });
        self.auto_refresh_state.update(cx, |state, cx| {
            let options = auto_refresh_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&auto_refresh_on_focus, window, cx);
        });
        self.theme_state.update(cx, |state, cx| {
            let options = theme_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&theme, window, cx);
        });
        self.diff_layout_state.update(cx, |state, cx| {
            let options = diff_layout_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&diff_layout, window, cx);
        });
        self.graph_history_state.update(cx, |state, cx| {
            let options = graph_history_options(locale);
            state.set_items(options, window, cx);
            state.set_selected_value(&graph_history, window, cx);
        });
        self.ui_font_state.update(cx, |state, cx| {
            let options = font_options(locale, &fonts);
            state.set_items(SearchableVec::from(options), window, cx);
            state.set_selected_value(&ui_font, window, cx);
        });
        self.mono_font_state.update(cx, |state, cx| {
            let options = font_options(locale, &fonts);
            state.set_items(SearchableVec::from(options), window, cx);
            state.set_selected_value(&mono_font, window, cx);
        });
        cx.notify();
    }

    fn select_section(
        &mut self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) {
        self.section = section;
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(SettingsPanelEvent::Close);
    }

    fn category_button(
        &self,
        id: &'static str,
        label: String,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.section == section;
        let this = cx.entity();
        // Selected rows use the same accent pair as hovered menu rows so the
        // label stays readable and inverted on every theme; list_active keeps
        // the theme foreground, which never flips on light accent blues.
        div()
            .id(id)
            .w_full()
            .px_3()
            .py_2()
            .rounded_md()
            .text_size(px(12.))
            .text_color(if selected {
                cx.theme().accent_foreground
            } else {
                cx.theme().colors.muted_foreground
            })
            .bg(if selected {
                cx.theme().tokens.accent.color
            } else {
                cx.theme().transparent
            })
            .hover(|element| {
                if selected {
                    // Keep the accent pairing while hovered; the list hover
                    // background would hide the inverted label.
                    element.bg(cx.theme().tokens.accent.color)
                } else {
                    element.bg(cx.theme().colors.list_hover)
                }
            })
            .on_click(move |_event, _window, cx| {
                this.update(cx, |panel, cx| panel.select_section(section, cx));
            })
            .child(shared(label))
    }

    fn field(
        label: String,
        control: AnyElement,
        foreground: Hsla,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(foreground)
                    .child(shared(label)),
            )
            .child(control)
    }

    fn section_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors.clone();
        match self.section {
            SettingsSection::General => v_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "settings-general",
                        ))),
                )
                .child(Self::field(
                    i18n::text(self.locale, "language-title"),
                    Select::new(&self.language_state)
                        .w_full()
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "auto-refresh-on-focus-title"),
                    Select::new(&self.auto_refresh_state)
                        .w_full()
                        .into_any_element(),
                    colors.foreground,
                ))
                .into_any_element(),
            SettingsSection::Appearance => v_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "settings-appearance",
                        ))),
                )
                .child(Self::field(
                    i18n::text(self.locale, "theme-title"),
                    Select::new(&self.theme_state).w_full().into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "ui-font-title"),
                    Select::new(&self.ui_font_state)
                        .w_full()
                        .search_placeholder(i18n::text(
                            self.locale,
                            "font-search-placeholder",
                        ))
                        .menu_width(px(360.))
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "mono-font-title"),
                    Select::new(&self.mono_font_state)
                        .w_full()
                        .search_placeholder(i18n::text(
                            self.locale,
                            "font-search-placeholder",
                        ))
                        .menu_width(px(360.))
                        .into_any_element(),
                    colors.foreground,
                ))
                .into_any_element(),
            SettingsSection::Layout => v_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "settings-layout",
                        ))),
                )
                .child(Self::field(
                    i18n::text(self.locale, "diff-layout-title"),
                    Select::new(&self.diff_layout_state)
                        .w_full()
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(Self::field(
                    i18n::text(self.locale, "graph-history-title"),
                    Select::new(&self.graph_history_state)
                        .w_full()
                        .into_any_element(),
                    colors.foreground,
                ))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "graph-history-description",
                        ))),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(
                            self.locale,
                            "layout-persistence-description",
                        ))),
                )
                .into_any_element(),
        }
    }
}

impl Render for SettingsPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let close = this.clone();
        let card = h_flex()
            .id("settings-card")
            .w(px(760.))
            .h(relative(0.9))
            .max_w(px(820.))
            .min_w(px(620.))
            .bg(colors.background)
            .border_1()
            .border_color(colors.border)
            .rounded_lg()
            .when(cx.theme().shadow, |element| element.shadow_lg())
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(
                v_flex()
                    .w(px(172.))
                    .h_full()
                    .flex_shrink_0()
                    .p_3()
                    .gap_1()
                    .border_r_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .px_2()
                            .py_2()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(colors.foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "settings-title",
                            ))),
                    )
                    .child(self.category_button(
                        "settings-category-general",
                        i18n::text(self.locale, "settings-general"),
                        SettingsSection::General,
                        cx,
                    ))
                    .child(self.category_button(
                        "settings-category-appearance",
                        i18n::text(self.locale, "settings-appearance"),
                        SettingsSection::Appearance,
                        cx,
                    ))
                    .child(self.category_button(
                        "settings-category-layout",
                        i18n::text(self.locale, "settings-layout"),
                        SettingsSection::Layout,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        h_flex()
                            .w_full()
                            .flex_shrink_0()
                            .items_center()
                            .justify_between()
                            .p_4()
                            .border_b_1()
                            .border_color(colors.border)
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(colors.muted_foreground)
                                    .child(shared(i18n::text(
                                        self.locale,
                                        "settings-description",
                                    ))),
                            )
                            .child(
                                Button::new("settings-close")
                                    .label(i18n::text(
                                        self.locale,
                                        "settings-close",
                                    ))
                                    .ghost()
                                    .on_click(move |_event, _window, cx| {
                                        close.update(cx, |panel, cx| {
                                            panel.close(cx)
                                        });
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .p_6()
                            .child(self.section_content(cx)),
                    ),
            );

        v_flex()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .bg(colors.background.opacity(0.9))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                this.update(cx, |panel, cx| panel.close(cx));
            })
            .child(card)
    }
}

fn selected_index<T: Clone + PartialEq>(
    options: &[SettingsOption<T>],
    value: &T,
) -> Option<IndexPath> {
    options
        .iter()
        .position(|option| &option.value == value)
        .map(|index| IndexPath::default().row(index))
}

fn language_options(locale: Locale) -> Vec<SettingsOption<LanguagePreference>> {
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

fn theme_options(locale: Locale) -> Vec<SettingsOption<ThemePreference>> {
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

fn diff_layout_options(
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

fn graph_history_options(
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

fn auto_refresh_options(locale: Locale) -> Vec<SettingsOption<bool>> {
    vec![
        SettingsOption::new(true, i18n::text(locale, "setting-enabled")),
        SettingsOption::new(false, i18n::text(locale, "setting-disabled")),
    ]
}

fn font_options(
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
