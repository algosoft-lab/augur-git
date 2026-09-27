//! The native application menu.
//!
//! macOS renders the first menu in the system menu bar; the other platforms get
//! a menu bar inside the window. The in-window hamburger menu is built by the
//! webview so it can show the recent repository list, but both surfaces use the
//! same identifiers and the same accelerator for every command.
//!
//! The menu is rebuilt whenever the language or the shortcut overrides change,
//! so labels and key equivalents never drift from the settings page.

use tauri::menu::{
    IsMenuItem, Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, Runtime};

use augur_core::i18n::{self, Locale};
use augur_core::keymap::ResolvedShortcut;

use crate::events::{AppEvent, AppEventEnvelope, MENU_EVENT};

/// Menu item identifiers. The webview maps them to actions; the same
/// identifiers are used by the in-window menu.
pub mod ids {
    pub const OPEN_REPOSITORY: &str = "menu.open-repository";
    pub const OPEN_WSL_REPOSITORY: &str = "menu.open-wsl-repository";
    pub const NEW_TAB: &str = "menu.new-tab";
    pub const INSTALL_CLI: &str = "menu.install-cli";
    pub const REMOVE_CLI: &str = "menu.remove-cli";
    pub const SETTINGS: &str = "menu.settings";
    pub const ABOUT: &str = "menu.about";
    pub const QUIT: &str = "menu.quit";
}

/// Translate a stored accelerator into the spelling the platform menu expects.
///
/// `cmd-q` becomes `CmdOrCtrl+Q` so the same default works on every platform,
/// while an explicit `ctrl-` or `alt-` prefix is preserved.
pub fn accelerator_for(keys: &[String]) -> Option<String> {
    let primary = keys.iter().find(|key| !key.trim().is_empty())?;
    let parts: Vec<&str> = primary.trim().split('-').collect();
    let (key, modifiers) = parts.split_last()?;
    let mut out: Vec<String> = Vec::new();
    let mut has_modifier = false;
    for modifier in modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "super" => out.push("CmdOrCtrl".to_string()),
            "ctrl" | "control" => out.push("Ctrl".to_string()),
            "alt" | "option" => out.push("Alt".to_string()),
            "shift" => out.push("Shift".to_string()),
            "meta" => out.push("Super".to_string()),
            _ => return None,
        }
        has_modifier = true;
    }
    if !has_modifier {
        // A bare key cannot be an accelerator on its own.
        return None;
    }
    out.push(normalize_key(key)?);
    Some(out.join("+"))
}

/// Uppercase a single key name the platform menu understands.
fn normalize_key(key: &str) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    if let Some(number) = key.strip_prefix('F').or_else(|| key.strip_prefix('f'))
        && let Ok(index) = number.parse::<u8>()
        && (1..=24).contains(&index)
    {
        return Some(format!("F{index}"));
    }
    if key.chars().count() == 1 {
        return Some(key.to_ascii_uppercase());
    }
    // Named keys are spelled out; anything else is rejected rather than
    // producing a menu entry the platform cannot display.
    const NAMED: [&str; 20] = [
        "space", "enter", "return", "escape", "esc", "tab", "backspace", "delete", "insert",
        "home", "end", "pageup", "pagedown", "up", "down", "left", "right", "printscreen",
        "pause", "capslock",
    ];
    NAMED
        .iter()
        .find(|name| name.eq_ignore_ascii_case(key))
        .map(|name| {
            let mut characters = name.chars();
            match characters.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
                None => String::new(),
            }
        })
}

/// Build and install the native menu for the given language and shortcuts.
pub fn install<R: Runtime>(app: &AppHandle<R>, locale: Locale, shortcuts: &[ResolvedShortcut]) {
    match build(app, locale, shortcuts) {
        Ok(menu) => {
            if let Err(error) = app.set_menu(menu) {
                log::error!("[menu] failed to install the native menu: {error}");
            }
        }
        Err(error) => log::error!("[menu] failed to build the native menu: {error}"),
    }
}

fn build<R: Runtime>(
    app: &AppHandle<R>,
    locale: Locale,
    shortcuts: &[ResolvedShortcut],
) -> tauri::Result<Menu<R>> {
    let quit_accelerator = shortcuts
        .iter()
        .find(|shortcut| shortcut.command == augur_core::keymap::QUIT_COMMAND)
        .and_then(|shortcut| accelerator_for(&shortcut.keys));

    // Every item is built up front and kept alive for the whole function: the
    // submenu builders borrow them, so a temporary would be dropped too early.
    let open_repository = MenuItemBuilder::with_id(
        ids::OPEN_REPOSITORY,
        i18n::text(locale, "menu-open-repository"),
    )
    .build(app)?;
    let open_wsl = MenuItemBuilder::with_id(
        ids::OPEN_WSL_REPOSITORY,
        i18n::text(locale, "menu-open-wsl-repository"),
    )
    .build(app)?;
    let new_tab =
        MenuItemBuilder::with_id(ids::NEW_TAB, i18n::text(locale, "menu-new-tab")).build(app)?;
    let install_cli =
        MenuItemBuilder::with_id(ids::INSTALL_CLI, i18n::text(locale, "menu-install-cli"))
            .build(app)?;
    let remove_cli =
        MenuItemBuilder::with_id(ids::REMOVE_CLI, i18n::text(locale, "menu-remove-cli"))
            .build(app)?;
    let settings =
        MenuItemBuilder::with_id(ids::SETTINGS, i18n::text(locale, "menu-settings")).build(app)?;
    let about =
        MenuItemBuilder::with_id(ids::ABOUT, i18n::text(locale, "menu-about")).build(app)?;
    let mut quit_builder = MenuItemBuilder::with_id(ids::QUIT, i18n::text(locale, "menu-quit"));
    if let Some(accelerator) = &quit_accelerator {
        quit_builder = quit_builder.accelerator(accelerator);
    }
    let quit = quit_builder.build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator_two = PredefinedMenuItem::separator(app)?;

    let mut file_items: Vec<&dyn IsMenuItem<R>> = vec![&open_repository];
    if cfg!(windows) {
        file_items.push(&open_wsl);
    }
    file_items.push(&new_tab);
    file_items.push(&separator);
    file_items.push(&install_cli);
    file_items.push(&remove_cli);
    if !cfg!(target_os = "macos") {
        file_items.push(&separator_two);
        file_items.push(&quit);
    }

    let file_menu = SubmenuBuilder::new(app, i18n::text(locale, "menu-file"))
        .items(&file_items)
        .build()?;
    let edit_menu = SubmenuBuilder::new(app, i18n::text(locale, "menu-edit"))
        .item(&settings)
        .build()?;
    let help_menu = SubmenuBuilder::new(app, i18n::text(locale, "menu-help"))
        .item(&about)
        .build()?;
    // macOS expects the application menu first, carrying About and Quit.
    let app_menu = SubmenuBuilder::new(app, i18n::text(locale, "app-name"))
        .item(&about)
        .separator()
        .item(&quit)
        .build()?;

    let mut submenus: Vec<&dyn IsMenuItem<R>> = Vec::new();
    if cfg!(target_os = "macos") {
        submenus.push(&app_menu);
    }
    submenus.push(&file_menu);
    submenus.push(&edit_menu);
    submenus.push(&help_menu);

    Menu::with_items(app, &submenus)
}

/// Forward a native menu activation to the webview, which owns the behavior.
///
/// Quit is the exception: the state has to be flushed before the process ends,
/// so the webview receives the activation, writes the final snapshot, and then
/// asks the backend to exit.
pub fn dispatch<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let _ = app.emit(
        MENU_EVENT,
        MenuActivation {
            id: id.to_string(),
        },
    );
}

/// Payload of a native menu activation.
#[derive(Clone, serde::Serialize)]
pub struct MenuActivation {
    pub id: String,
}

/// Tell every window the language changed so it can refresh its own labels.
pub fn notify_locale_changed<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit(
        crate::events::APP_EVENT,
        AppEventEnvelope {
            event: AppEvent::SettingsChanged,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_translate_to_the_platform_spelling() {
        assert_eq!(
            accelerator_for(&["cmd-q".to_string()]).as_deref(),
            Some("CmdOrCtrl+Q")
        );
        assert_eq!(
            accelerator_for(&["alt-f4".to_string()]).as_deref(),
            Some("Alt+F4")
        );
        assert_eq!(
            accelerator_for(&["ctrl-shift-p".to_string()]).as_deref(),
            Some("Ctrl+Shift+P")
        );
        assert_eq!(
            accelerator_for(&["cmd-alt-4".to_string()]).as_deref(),
            Some("CmdOrCtrl+Alt+4")
        );
    }

    #[test]
    fn function_and_named_keys_are_accepted() {
        assert_eq!(
            accelerator_for(&["cmd-f5".to_string()]).as_deref(),
            Some("CmdOrCtrl+F5")
        );
        assert_eq!(
            accelerator_for(&["alt-space".to_string()]).as_deref(),
            Some("Alt+Space")
        );
    }

    #[test]
    fn an_unbound_bare_or_misspelled_key_produces_no_accelerator() {
        assert_eq!(accelerator_for(&[]), None);
        assert_eq!(accelerator_for(&["  ".to_string()]), None);
        assert_eq!(accelerator_for(&["q".to_string()]), None);
        assert_eq!(accelerator_for(&["hyper-q".to_string()]), None);
        assert_eq!(accelerator_for(&["cmd-nonsense".to_string()]), None);
    }

    #[test]
    fn the_first_binding_wins_when_several_are_listed() {
        assert_eq!(
            accelerator_for(&["cmd-shift-p".to_string(), "cmd-p".to_string()]).as_deref(),
            Some("CmdOrCtrl+Shift+P")
        );
    }
}
