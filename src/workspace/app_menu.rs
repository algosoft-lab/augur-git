//! The JetBrains-style application menu in the window title bar.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    IconName,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::rc::Rc;

use crate::core::i18n;

gpui::actions!(
    augur_git,
    [OpenRepository, NewTab, OpenSettings, OpenAbout, Quit,]
);

#[derive(Clone, Debug)]
pub(crate) enum AppMenuEvent {
    OpenRecent(String),
}

pub(crate) struct AppMenu {
    locale: i18n::Locale,
    recent_repos: Vec<String>,
}

impl EventEmitter<AppMenuEvent> for AppMenu {}

impl AppMenu {
    pub(crate) fn new(locale: i18n::Locale, recent_repos: Vec<String>) -> Self {
        Self {
            locale,
            recent_repos,
        }
    }

    pub(crate) fn set_locale(&mut self, locale: i18n::Locale) {
        self.locale = locale;
    }

    pub(crate) fn set_recent_repos(&mut self, recent_repos: Vec<String>) {
        self.recent_repos = recent_repos;
    }
}

impl Render for AppMenu {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let locale = self.locale;
        let recent_repos = Rc::new(self.recent_repos.clone());
        let app_menu = cx.entity();

        Button::new("app-menu")
            .ghost()
            .compact()
            .icon(IconName::Menu)
            .tooltip(i18n::text(locale, "menu-open"))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, cx| {
                let recent_repos = recent_repos.clone();
                let app_menu = app_menu.clone();
                let recent_menu = PopupMenu::build(window, cx, move |menu, _, _| {
                    if recent_repos.is_empty() {
                        return menu.item(PopupMenuItem::label(i18n::text(
                            locale,
                            "menu-no-recent-repositories",
                        )));
                    }

                    recent_repos.iter().fold(menu, |menu, path| {
                        let path_for_event = path.clone();
                        let app_menu = app_menu.clone();
                        menu.item(PopupMenuItem::new(path.clone()).on_click(
                            move |_event, _window, cx| {
                                let _ = app_menu.update(cx, |_menu, cx| {
                                    cx.emit(AppMenuEvent::OpenRecent(path_for_event.clone()));
                                });
                            },
                        ))
                    })
                });

                let file_menu = PopupMenu::build(window, cx, move |menu, _, _| {
                    menu.menu_with_icon(
                        i18n::text(locale, "menu-open-repository"),
                        IconName::FolderOpen,
                        Box::new(OpenRepository),
                    )
                    .menu_with_icon(
                        i18n::text(locale, "menu-new-tab"),
                        IconName::Plus,
                        Box::new(NewTab),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::submenu(
                            i18n::text(locale, "menu-recent-repositories"),
                            recent_menu.clone(),
                        )
                        .icon(IconName::Folder),
                    )
                });

                let view_menu = PopupMenu::build(window, cx, move |menu, _, _| {
                    menu.menu_with_icon(
                        i18n::text(locale, "menu-settings"),
                        IconName::Settings,
                        Box::new(OpenSettings),
                    )
                });

                let help_menu = PopupMenu::build(window, cx, move |menu, _, _| {
                    menu.menu_with_icon(
                        i18n::text(locale, "menu-about"),
                        IconName::Info,
                        Box::new(OpenAbout),
                    )
                });

                menu.item(
                    PopupMenuItem::submenu(i18n::text(locale, "menu-file"), file_menu)
                        .icon(IconName::FolderOpen),
                )
                .item(
                    PopupMenuItem::submenu(i18n::text(locale, "menu-view"), view_menu)
                        .icon(IconName::Settings),
                )
                .item(
                    PopupMenuItem::submenu(i18n::text(locale, "menu-help"), help_menu)
                        .icon(IconName::Info),
                )
                .separator()
                .menu(i18n::text(locale, "menu-quit"), Box::new(Quit))
            })
    }
}

/// Install the native application menu. macOS renders the first menu in the
/// system menu bar; the in-window menu remains available on every platform.
pub(crate) fn install_native_menu(locale: i18n::Locale, cx: &mut App) {
    let mut menus = Vec::new();

    #[cfg(target_os = "macos")]
    menus.push(Menu::new(crate::core::build_info::APP_NAME).items([
        MenuItem::action(i18n::text(locale, "menu-about"), OpenAbout),
        MenuItem::separator(),
        MenuItem::action(i18n::text(locale, "menu-quit"), Quit),
    ]));

    let mut file_items = vec![
        MenuItem::action(
            i18n::text(locale, "menu-open-repository"),
            OpenRepository,
        ),
        MenuItem::action(i18n::text(locale, "menu-new-tab"), NewTab),
    ];
    #[cfg(not(target_os = "macos"))]
    file_items.extend([
        MenuItem::separator(),
        MenuItem::action(i18n::text(locale, "menu-quit"), Quit),
    ]);
    menus.push(Menu::new(i18n::text(locale, "menu-file")).items(file_items));
    menus.push(Menu::new(i18n::text(locale, "menu-view")).items([
        MenuItem::action(i18n::text(locale, "menu-settings"), OpenSettings),
    ]));
    menus.push(Menu::new(i18n::text(locale, "menu-help")).items([
        MenuItem::action(i18n::text(locale, "menu-about"), OpenAbout),
    ]));

    cx.set_menus(menus);
}
