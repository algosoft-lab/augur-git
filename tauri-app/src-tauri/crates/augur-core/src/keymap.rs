//! Shortcut configuration schema and merge logic.
//!
//! System defaults are compiled into the binary; user overrides are stored in
//! the application's settings document. Overrides are per command: as soon as
//! the user file contains an entry for a command, that entry replaces every
//! default binding for the command on the current platform, and an entry with
//! an empty `keys` list unbinds it.
//!
//! This module is free of windowing and webview concerns. The platform
//! accelerator strings (`cmd-q`, `alt-f4`) are resolved by the frontend and the
//! native menu is rebuilt from the same resolved list.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// System-level shortcut defaults, embedded at compile time so a release
/// carries its baseline keymap without a runtime file lookup.
pub const SYSTEM_DEFAULTS_JSON: &str = include_str!("../keymap.default.json");

/// Command that quits the application.
pub const QUIT_COMMAND: &str = "app.quit";

/// User shortcut overrides, keyed by command id. Persisted beside the other
/// settings so one document owns everything the settings page can change.
pub type ShortcutOverrides = BTreeMap<String, Vec<String>>;

/// The shortcut state as it is stored and as it is sent to the webview.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShortcutState {
    /// System defaults merged with the user's overrides, for display.
    pub resolved: Vec<ResolvedShortcut>,
    /// The user's overrides only, so a reset can be written back.
    pub overrides: ShortcutOverrides,
}

/// Every command id the settings page can edit.
pub const COMMANDS: [&str; 1] = [QUIT_COMMAND];

/// The full set of shortcut bindings from one JSON document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutFile {
    pub bindings: Vec<ShortcutBinding>,
}

/// One command-to-keystrokes entry. `keys` holds accelerator strings such as
/// `cmd-q`; an empty list means the command is unbound. `platforms` restricts
/// the entry to the named operating systems (`std::env::consts::OS`); an empty
/// list applies everywhere.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutBinding {
    pub command: String,
    pub keys: Vec<String>,
    pub platforms: Vec<String>,
}

impl ShortcutBinding {
    pub fn matches_platform(&self, platform: &str) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|name| name == platform)
    }
}

/// The effective keys for one command after merging defaults and overrides.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedShortcut {
    pub command: String,
    pub keys: Vec<String>,
}

/// Parse a shortcut document, reporting syntax errors to the caller.
pub fn parse(text: &str) -> anyhow::Result<ShortcutFile> {
    Ok(serde_json::from_str(text)?)
}

/// System defaults. The embedded document is a build artifact, so a parse
/// failure degrades to "no default shortcuts" instead of aborting startup.
pub fn system_defaults() -> ShortcutFile {
    match parse(SYSTEM_DEFAULTS_JSON) {
        Ok(file) => file,
        Err(error) => {
            log::error!("[keymap] embedded system defaults failed to parse: {error}");
            ShortcutFile::default()
        }
    }
}

/// Merge system defaults with user overrides for the given known command ids.
/// Unknown commands are skipped with a warning so a stale user file cannot
/// resurrect removed shortcuts.
pub fn resolve(
    system: &ShortcutFile,
    user: &ShortcutFile,
    commands: &[&str],
) -> Vec<ResolvedShortcut> {
    let platform = std::env::consts::OS;
    let mut resolved = Vec::new();
    for command in commands {
        let user_entries: Vec<&ShortcutBinding> = user
            .bindings
            .iter()
            .filter(|binding| binding.command == *command && binding.matches_platform(platform))
            .collect();
        let entries = if user_entries.is_empty() {
            system
                .bindings
                .iter()
                .filter(|binding| binding.command == *command && binding.matches_platform(platform))
                .collect::<Vec<_>>()
        } else {
            user_entries
        };
        if entries.is_empty() {
            continue;
        }
        let mut keys: Vec<String> = Vec::new();
        for binding in entries {
            for key in &binding.keys {
                let key = key.trim();
                if !key.is_empty() && !keys.iter().any(|seen| seen == key) {
                    keys.push(key.to_string());
                }
            }
        }
        resolved.push(ResolvedShortcut {
            command: (*command).to_string(),
            keys,
        });
    }
    for binding in system.bindings.iter().chain(user.bindings.iter()) {
        if !commands.contains(&binding.command.as_str()) {
            log::warn!(
                "[keymap] ignoring unknown command in shortcut file: {}",
                binding.command
            );
        }
    }
    resolved
}

/// Replace or remove the user override for one command. `None` deletes every
/// user entry for the command, restoring the system defaults.
pub fn set_user_command(file: &mut ShortcutFile, command: &str, keys: Option<Vec<String>>) {
    file.bindings.retain(|binding| binding.command != command);
    if let Some(keys) = keys {
        file.bindings.push(ShortcutBinding {
            command: command.to_string(),
            keys,
            platforms: Vec::new(),
        });
    }
}

/// Project the user document into a compact `command -> keys` map for storage
/// alongside the other settings.
pub fn overrides_by_command(file: &ShortcutFile) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for binding in &file.bindings {
        if COMMANDS.contains(&binding.command.as_str()) {
            map.insert(binding.command.clone(), binding.keys.clone());
        }
    }
    map
}

/// Rebuild a user document from a compact map.
pub fn file_from_overrides(overrides: &BTreeMap<String, Vec<String>>) -> ShortcutFile {
    let mut file = ShortcutFile::default();
    for (command, keys) in overrides {
        if COMMANDS.contains(&command.as_str()) {
            file.bindings.push(ShortcutBinding {
                command: command.clone(),
                keys: keys.clone(),
                platforms: Vec::new(),
            });
        }
    }
    file
}

/// Whether a keystroke string is a well-formed accelerator combination.
///
/// The accepted grammar matches the strings the system defaults use:
/// modifiers joined by `-` in `ctrl-alt-shift-meta` order, optionally followed
/// by exactly one key.
pub fn is_valid_combo(combo: &str) -> bool {
    let combo = combo.trim();
    if combo.is_empty() || combo.contains(char::is_whitespace) {
        return false;
    }
    let mut parts = combo.split('-');
    let Some(last) = parts.next_back() else {
        return false;
    };
    if last.is_empty() {
        return false;
    }
    let mut seen_modifier = false;
    for part in parts {
        match part {
            "ctrl" | "control" => {
                if seen_modifier {
                    return false;
                }
                seen_modifier = true;
            }
            "alt" | "option" | "shift" | "meta" | "cmd" | "command" | "super" => {}
            _ => return false,
        }
    }
    true
}

/// Split a user-entered combination into individual accelerators.
pub fn parse_combo_list(value: &str) -> Result<Vec<String>, String> {
    let mut combos = Vec::new();
    for part in value.split(',') {
        let combo = part.trim();
        if combo.is_empty() {
            continue;
        }
        if !is_valid_combo(combo) {
            return Err(combo.to_string());
        }
        if !combos.iter().any(|seen: &String| seen == combo) {
            combos.push(combo.to_string());
        }
    }
    Ok(combos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(command: &str, keys: &[&str]) -> ShortcutBinding {
        ShortcutBinding {
            command: command.to_string(),
            keys: keys.iter().map(|key| key.to_string()).collect(),
            platforms: Vec::new(),
        }
    }

    #[test]
    fn system_defaults_document_parses() {
        let file = system_defaults();
        assert!(!file.bindings.is_empty());
        let platform = std::env::consts::OS;
        let quit = file
            .bindings
            .iter()
            .filter(|entry| entry.command == QUIT_COMMAND && entry.matches_platform(platform))
            .collect::<Vec<_>>();
        assert_eq!(quit.len(), 1, "exactly one quit default per platform");
        assert!(!quit[0].keys.is_empty());
    }

    #[test]
    fn user_entries_replace_system_per_command() {
        let system = ShortcutFile {
            bindings: vec![binding(QUIT_COMMAND, &["cmd-q"])],
        };
        let user = ShortcutFile {
            bindings: vec![binding(QUIT_COMMAND, &["ctrl-shift-q"])],
        };
        let resolved = resolve(&system, &user, &COMMANDS);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].keys, vec!["ctrl-shift-q".to_string()]);
    }

    #[test]
    fn empty_user_keys_unbind_command() {
        let system = ShortcutFile {
            bindings: vec![binding(QUIT_COMMAND, &["cmd-q"])],
        };
        let user = ShortcutFile {
            bindings: vec![binding(QUIT_COMMAND, &[])],
        };
        let resolved = resolve(&system, &user, &COMMANDS);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].keys.is_empty());
    }

    #[test]
    fn unknown_commands_are_skipped() {
        let system = ShortcutFile {
            bindings: vec![binding("app.removed", &["cmd-x"])],
        };
        let user = ShortcutFile {
            bindings: vec![binding("app.other", &["cmd-y"])],
        };
        assert!(resolve(&system, &user, &COMMANDS).is_empty());
    }

    #[test]
    fn platform_restricted_entries_only_apply_to_their_platform() {
        let binding = ShortcutBinding {
            command: QUIT_COMMAND.to_string(),
            keys: vec!["alt-f4".into()],
            platforms: vec!["plan9".into()],
        };
        assert!(!binding.matches_platform(std::env::consts::OS));
        assert!(
            ShortcutBinding {
                platforms: Vec::new(),
                ..binding
            }
            .matches_platform(std::env::consts::OS)
        );
    }

    #[test]
    fn overrides_round_trip_through_the_compact_map() {
        let mut file = ShortcutFile::default();
        set_user_command(&mut file, QUIT_COMMAND, Some(vec!["ctrl-q".into()]));
        let map = overrides_by_command(&file);
        assert_eq!(map.get(QUIT_COMMAND), Some(&vec!["ctrl-q".to_string()]));
        assert_eq!(file_from_overrides(&map), file);
    }

    #[test]
    fn combo_validation_rejects_malformed_input() {
        assert!(is_valid_combo("cmd-q"));
        assert!(is_valid_combo("ctrl-shift-p"));
        assert!(is_valid_combo("alt-f4"));
        assert!(!is_valid_combo(""));
        assert!(!is_valid_combo("cmd q"));
        assert!(!is_valid_combo("hyper-q"));
        assert!(!is_valid_combo("cmd-"));
        assert!(!is_valid_combo("ctrl-ctrl-q"));
        assert!(parse_combo_list("cmd-q, ctrl-shift-q").is_ok());
        // A bare key is a legal accelerator; a modifier typo is not.
        assert!(parse_combo_list("cmd-q, f5").is_ok());
        assert!(parse_combo_list("cmd-q, hyper-q").is_err());
        assert!(parse_combo_list("  ").unwrap().is_empty());
    }
}
