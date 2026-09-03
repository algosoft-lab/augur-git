//! Global shortcut layer: bridges persisted keymap JSON with GPUI bindings.
//!
//! GPUI keeps one shared app-level keymap; bindings added later win ties, a
//! `NoAction` binding suppresses older bindings for the same keystrokes, and
//! `App::set_menus` reads the keymap to print native menu key equivalents. All
//! three facts drive the design here: defaults and user overrides are bound
//! once at startup, and edits are applied as deltas (NoAction for retired
//! combinations, fresh action bindings for new ones) so remapping takes
//! effect immediately without disturbing bindings owned by other code.

use std::collections::HashMap;

use gpui::{Action, App, Global, KeyBinding, Keystroke, NoAction};

use crate::core::keymap::{self, ResolvedShortcut, ShortcutFile};

use super::app_menu;

/// Command ids accepted by the shortcut files. `app.quit` routes into the
/// existing `Quit` action, which defers while operations are active.
pub(crate) const QUIT_COMMAND: &str = "app.quit";

/// A command exposed on the Shortcuts settings page.
pub(crate) struct ShortcutCommandSpec {
    pub id: &'static str,
    pub label_key: &'static str,
}

pub(crate) const COMMANDS: &[ShortcutCommandSpec] = &[ShortcutCommandSpec {
    id: QUIT_COMMAND,
    label_key: "shortcut-app-quit",
}];

fn command_ids() -> Vec<&'static str> {
    COMMANDS.iter().map(|command| command.id).collect()
}

fn action_for(command: &str) -> Option<Box<dyn Action>> {
    match command {
        QUIT_COMMAND => Some(Box::new(app_menu::Quit)),
        _ => None,
    }
}

/// Parse-time view of the merged keymap, stored as a GPUI global so panels
/// can read current and default keys without plumbing the state downward.
pub(crate) struct KeymapState {
    system: ShortcutFile,
    user: ShortcutFile,
    resolved: Vec<ResolvedShortcut>,
}

impl Global for KeymapState {}

/// Normalize one keystroke combination to the canonical form GPUI stores
/// parsed keystrokes in, so equivalent spellings deduplicate. Note that an
/// uppercase character canonicalizes to an explicit `shift` modifier and the
/// platform modifier alias is OS dependent (`cmd`/`super`/`win`). Returns
/// `None` when any part of the combination fails to parse.
pub(crate) fn normalize_combo(combo: &str) -> Option<String> {
    let mut parts = Vec::new();
    for source in combo.split_whitespace() {
        let keystroke = Keystroke::parse(source).ok()?;
        parts.push(keystroke.unparse());
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Parse the settings-page text form of a command's keys: comma-separated
/// combinations. Invalid combinations are reported back to the UI instead of
/// being bound.
pub(crate) fn parse_combo_list(
    text: &str,
) -> Result<Vec<String>, InvalidCombo> {
    let mut combos = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match normalize_combo(part) {
            Some(combo) => {
                if !combos.iter().any(|seen: &String| seen == &combo) {
                    combos.push(combo);
                }
            }
            None => return Err(InvalidCombo(part.to_string())),
        }
    }
    Ok(combos)
}

/// A combination that GPUI's keystroke parser rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InvalidCombo(pub String);

/// Bind the resolved system+user keymap. Must run before
/// `app_menu::install_native_menu` so native menus pick up key equivalents.
pub(crate) fn install(cx: &mut App) {
    let system = keymap::system_defaults();
    let user = keymap::load_user();
    install_with(cx, system, user);
}

fn install_with(cx: &mut App, system: ShortcutFile, user: ShortcutFile) {
    let resolved = keymap::resolve(&system, &user, &command_ids());
    let empty: Vec<ResolvedShortcut> = Vec::new();
    apply_diff(cx, &empty, &resolved);
    log::debug!(
        "[keymap] installed {} resolved shortcut entries",
        resolved.len()
    );
    cx.set_global(KeymapState {
        system,
        user,
        resolved,
    });
}

/// Override one command's keys, persist the user file, and re-apply.
pub(crate) fn set_shortcut(
    cx: &mut App,
    command: &str,
    keys: Vec<String>,
) -> anyhow::Result<()> {
    update_user_bindings(cx, command, Some(keys))
}

/// Remove one command's user override, restoring system defaults.
pub(crate) fn reset_shortcut(
    cx: &mut App,
    command: &str,
) -> anyhow::Result<()> {
    update_user_bindings(cx, command, None)
}

fn update_user_bindings(
    cx: &mut App,
    command: &str,
    keys: Option<Vec<String>>,
) -> anyhow::Result<()> {
    if action_for(command).is_none() {
        log::warn!("[keymap] ignored override for unknown command");
        return Ok(());
    }
    let (system, mut user, old_resolved) = match cx.try_global::<KeymapState>()
    {
        Some(state) => (
            state.system.clone(),
            state.user.clone(),
            state.resolved.clone(),
        ),
        None => (keymap::system_defaults(), keymap::load_user(), Vec::new()),
    };
    keymap::set_user_command(&mut user, command, keys);
    keymap::save_user(&user)?;
    let new_resolved = keymap::resolve(&system, &user, &command_ids());
    apply_diff(cx, &old_resolved, &new_resolved);
    let state = KeymapState {
        system,
        user,
        resolved: new_resolved,
    };
    if cx.has_global::<KeymapState>() {
        *cx.global_mut::<KeymapState>() = state;
    } else {
        cx.set_global(state);
    }
    log::info!("[keymap] applied shortcut changes for {command}");
    Ok(())
}

fn owner_map(resolved: &[ResolvedShortcut]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for shortcut in resolved {
        for combo in &shortcut.keys {
            map.entry(combo.clone())
                .or_insert_with(|| shortcut.command.clone());
        }
    }
    map
}

/// Emit the minimal delta between two resolved keymaps: retired or
/// reassigned combinations get a `NoAction` marker first (shadowing older
/// bindings), then newly assigned combinations bind their action (beating any
/// preceding `NoAction`).
fn apply_diff(
    cx: &mut App,
    previous: &[ResolvedShortcut],
    next: &[ResolvedShortcut],
) {
    let previous_map = owner_map(previous);
    let next_map = owner_map(next);
    for (combo, command) in &previous_map {
        if next_map.get(combo) != Some(command) {
            bind_combo(cx, combo, None);
        }
    }
    for (combo, command) in &next_map {
        if previous_map.get(combo) != Some(command) {
            bind_combo(cx, combo, action_for(command));
        }
    }
}

fn bind_combo(cx: &mut App, combo: &str, action: Option<Box<dyn Action>>) {
    let (action, label) = match action {
        Some(action) => (action, "action"),
        None => (Box::new(NoAction {}) as Box<dyn Action>, "no-action"),
    };
    let mapper = cx.keyboard_mapper().clone();
    match KeyBinding::load(combo, action, None, false, None, mapper.as_ref()) {
        Ok(binding) => cx.bind_keys([binding]),
        Err(error) => {
            log::warn!("[keymap] rejected {label} binding \"{combo}\": {error}")
        }
    }
}

/// Current effective keys for one command (already canonical).
pub(crate) fn resolved_keys(cx: &App, command: &str) -> Vec<String> {
    let Some(state) = cx.try_global::<KeymapState>() else {
        return Vec::new();
    };
    state
        .resolved
        .iter()
        .find(|shortcut| shortcut.command == command)
        .map(|shortcut| shortcut.keys.clone())
        .unwrap_or_default()
}

/// System default keys for one command on the current platform.
pub(crate) fn default_keys(cx: &App, command: &str) -> Vec<String> {
    let system = match cx.try_global::<KeymapState>() {
        Some(state) => state.system.clone(),
        None => keymap::system_defaults(),
    };
    let resolved =
        keymap::resolve(&system, &ShortcutFile::default(), &command_ids());
    resolved
        .into_iter()
        .find(|shortcut| shortcut.command == command)
        .map(|shortcut| shortcut.keys)
        .unwrap_or_default()
}

/// Whether the user file currently overrides this command.
pub(crate) fn is_overridden(cx: &App, command: &str) -> bool {
    let platform = std::env::consts::OS;
    cx.try_global::<KeymapState>().is_some_and(|state| {
        state.user.bindings.iter().any(|binding| {
            binding.command == command && binding.matches_platform(platform)
        })
    })
}

/// Settings-page display form of a key list.
pub(crate) fn combo_display(keys: &[String]) -> String {
    keys.join(", ")
}

/// Effective keys for one command in display form.
pub(crate) fn resolved_display(cx: &App, command: &str) -> String {
    combo_display(&resolved_keys(cx, command))
}

/// System default keys for one command in display form.
pub(crate) fn default_display(cx: &App, command: &str) -> String {
    combo_display(&default_keys(cx, command))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{EmptyView, TestAppContext};

    use super::*;

    fn quit_resolved(keys: &[String]) -> Vec<ResolvedShortcut> {
        vec![ResolvedShortcut {
            command: QUIT_COMMAND.to_string(),
            keys: keys.to_vec(),
        }]
    }

    #[gpui::test]
    fn platform_default_combination_dispatches_quit(cx: &mut TestAppContext) {
        let quits = Rc::new(Cell::new(0usize));
        let counter = quits.clone();
        cx.update(|cx| {
            cx.on_action(move |_: &app_menu::Quit, _cx| {
                counter.set(counter.get() + 1);
            });
            install_with(
                cx,
                keymap::system_defaults(),
                ShortcutFile::default(),
            );
        });
        let combo = {
            let keys = cx.read(|cx| resolved_keys(cx, QUIT_COMMAND));
            keys.first().cloned().expect("default quit shortcut")
        };
        let window = cx.add_window(|_, _| EmptyView);
        cx.simulate_keystrokes(window.into(), &combo);
        assert_eq!(quits.get(), 1, "{combo} should dispatch Quit once");
    }

    #[gpui::test]
    fn remap_and_unbind_shadow_previous_bindings(cx: &mut TestAppContext) {
        // Platform-independent combinations keep canonicalization stable.
        let first = normalize_combo("ctrl-alt-home").expect("ctrl-alt-home");
        let second = normalize_combo("ctrl-alt-end").expect("ctrl-alt-end");
        let quits = Rc::new(Cell::new(0usize));
        let counter = quits.clone();
        cx.update(|cx| {
            cx.on_action(move |_: &app_menu::Quit, _cx| {
                counter.set(counter.get() + 1);
            });
        });
        let window = cx.add_window(|_, _| EmptyView);

        cx.update(|cx| {
            apply_diff(cx, &[], &quit_resolved(&[first.clone()]));
        });
        cx.simulate_keystrokes(window.into(), &first);
        assert_eq!(quits.get(), 1);

        // Reassigning the command moves the key and suppresses the old one.
        cx.update(|cx| {
            apply_diff(
                cx,
                &quit_resolved(&[first.clone()]),
                &quit_resolved(&[second.clone()]),
            );
        });
        cx.simulate_keystrokes(window.into(), &first);
        assert_eq!(quits.get(), 1, "retired combination stays silent");
        cx.simulate_keystrokes(window.into(), &second);
        assert_eq!(quits.get(), 2);

        // Unbinding suppresses the live combination; restoring rebinds it
        // after the suppressor so a stale duplicate cannot fire twice.
        cx.update(|cx| {
            apply_diff(
                cx,
                &quit_resolved(&[second.clone()]),
                &quit_resolved(&[]),
            );
        });
        cx.simulate_keystrokes(window.into(), &second);
        assert_eq!(quits.get(), 2, "unbound combination stays silent");

        cx.update(|cx| {
            apply_diff(
                cx,
                &quit_resolved(&[]),
                &quit_resolved(&[second.clone()]),
            );
        });
        cx.simulate_keystrokes(window.into(), &second);
        assert_eq!(quits.get(), 3);
        cx.simulate_keystrokes(window.into(), &first);
        assert_eq!(quits.get(), 3, "the original binding stays suppressed");
    }

    #[test]
    fn normalizes_combo_case_and_spacing() {
        // Uppercase characters canonicalize to an explicit shift modifier and
        // the cmd alias is platform dependent, so assert on ctrl/named keys.
        assert_eq!(
            normalize_combo("Ctrl-Shift-K").as_deref(),
            Some("ctrl-shift-k")
        );
        assert_eq!(
            normalize_combo("ctrl-k  ctrl-s").as_deref(),
            Some("ctrl-k ctrl-s")
        );
        let cmd_q = normalize_combo("cmd-q").expect("cmd-q parses");
        assert_eq!(normalize_combo(&cmd_q).as_deref(), Some(cmd_q.as_str()));
    }

    #[test]
    fn rejects_invalid_combos() {
        // Keystroke grammar treats a bare `-` or `>` suffix as a key, so use
        // a multi-character component after a non-modifier to force an error.
        assert_eq!(normalize_combo("c-m"), None);
        assert_eq!(normalize_combo(""), None);
    }

    #[test]
    fn parses_comma_separated_list() {
        let cmd_q = normalize_combo("cmd-q").expect("cmd-q parses");
        let parsed = parse_combo_list(&format!(" {cmd_q} , alt-f4 ")).unwrap();
        assert_eq!(parsed, vec![cmd_q, "alt-f4".to_string()]);
        assert_eq!(parse_combo_list("").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_combo_list("cmd-q, c-m").unwrap_err(),
            InvalidCombo("c-m".to_string())
        );
    }

    #[test]
    fn default_combos_load_as_keybindings() {
        for source in ["cmd-q", "alt-f4"] {
            let combo = normalize_combo(source).expect(source);
            let binding = KeyBinding::load(
                &combo,
                Box::new(app_menu::Quit),
                None,
                false,
                None,
                &gpui::DummyKeyboardMapper,
            );
            assert!(
                binding.is_ok(),
                "{source} should load: {:?}",
                binding.as_ref().err()
            );
        }
    }

    #[test]
    fn owner_map_first_command_wins_shared_combos() {
        let resolved = vec![
            ResolvedShortcut {
                command: "app.a".to_string(),
                keys: vec!["cmd-q".to_string()],
            },
            ResolvedShortcut {
                command: "app.b".to_string(),
                keys: vec!["cmd-q".to_string(), "cmd-b".to_string()],
            },
        ];
        let map = owner_map(&resolved);
        assert_eq!(map.get("cmd-q").map(String::as_str), Some("app.a"));
        assert_eq!(map.get("cmd-b").map(String::as_str), Some("app.b"));
    }
}
