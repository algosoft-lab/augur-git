//! Shortcut configuration schemas and merge logic.
//!
//! System defaults are compiled into the binary from
//! `assets/keymap.default.json`. User overrides live in a VS Code-style JSON
//! file (`keybindings.json` next to `config.json`). Override semantics are
//! per command: when the user file contains at least one binding entry for a
//! command, that user entry replaces all default bindings for the command on
//! the current platform; an entry with an empty `keys` list unbinds it.
//!
//! Keystroke syntax validation happens in the GPUI layer (`workspace::keymap`)
//! so this module stays free of rendering dependencies.

use serde::{Deserialize, Serialize};

use crate::core::config;

/// System-level shortcut defaults, embedded at compile time so releases carry
/// their baseline keymap without a runtime file lookup.
pub const SYSTEM_DEFAULTS_JSON: &str =
    include_str!("../../assets/keymap.default.json");

/// The full set of shortcut bindings from one JSON document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutFile {
    pub bindings: Vec<ShortcutBinding>,
}

/// One command-to-keystrokes entry. `keys` holds keystroke combination
/// strings such as `"cmd-q"`; an empty list means the command is unbound.
/// `platforms` restricts the entry to the named OS values (`std::env::consts::OS`);
/// an empty list applies to every platform.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutBinding {
    pub command: String,
    pub keys: Vec<String>,
    pub platforms: Vec<String>,
}

impl ShortcutBinding {
    pub fn matches_platform(&self, platform: &str) -> bool {
        self.platforms.is_empty()
            || self.platforms.iter().any(|name| name == platform)
    }
}

/// The effective keys for one command after merging defaults and overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedShortcut {
    pub command: String,
    pub keys: Vec<String>,
}

/// Parse a shortcut document, reporting syntax errors to the caller.
pub fn parse(text: &str) -> anyhow::Result<ShortcutFile> {
    Ok(serde_json::from_str(text)?)
}

/// System defaults: the embedded document is a build artifact, so a parse
/// failure is logged and degraded to "no default shortcuts" rather than
/// aborting startup.
pub fn system_defaults() -> ShortcutFile {
    match parse(SYSTEM_DEFAULTS_JSON) {
        Ok(file) => file,
        Err(error) => {
            log::error!(
                "[keymap] embedded system defaults failed to parse: {error}"
            );
            ShortcutFile::default()
        }
    }
}

/// Load user shortcut overrides; a missing or broken file means "no
/// overrides", matching the resilience rules for `config.json`.
pub fn load_user() -> ShortcutFile {
    let path = config::keybindings_store_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text).unwrap_or_else(|error| {
            log::warn!(
                "[keymap] failed to parse {}; using defaults: {error}",
                path.display()
            );
            ShortcutFile::default()
        }),
        Err(_) => ShortcutFile::default(),
    }
}

/// Persist user shortcut overrides atomically.
pub fn save_user(file: &ShortcutFile) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(file)?;
    config::write_atomically(&config::keybindings_store_path(), &text)?;
    Ok(())
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
            .filter(|binding| {
                binding.command == *command
                    && binding.matches_platform(platform)
            })
            .collect();
        let entries = if user_entries.is_empty() {
            system
                .bindings
                .iter()
                .filter(|binding| {
                    binding.command == *command
                        && binding.matches_platform(platform)
                })
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
/// user entry for the command, restoring system defaults.
pub fn set_user_command(
    file: &mut ShortcutFile,
    command: &str,
    keys: Option<Vec<String>>,
) {
    file.bindings.retain(|binding| binding.command != command);
    if let Some(keys) = keys {
        file.bindings.push(ShortcutBinding {
            command: command.to_string(),
            keys,
            platforms: Vec::new(),
        });
    }
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
            .filter(|entry| {
                entry.command == "app.quit" && entry.matches_platform(platform)
            })
            .collect::<Vec<_>>();
        assert_eq!(quit.len(), 1, "exactly one quit default per platform");
        assert!(!quit[0].keys.is_empty());
    }

    #[test]
    fn user_entries_replace_system_per_command() {
        let system = ShortcutFile {
            bindings: vec![binding("app.quit", &["cmd-q"])],
        };
        let user = ShortcutFile {
            bindings: vec![binding("app.quit", &["ctrl-shift-q"])],
        };
        let resolved = resolve(&system, &user, &["app.quit"]);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].keys, vec!["ctrl-shift-q".to_string()]);
    }

    #[test]
    fn empty_user_keys_unbind_command() {
        let system = ShortcutFile {
            bindings: vec![binding("app.quit", &["cmd-q"])],
        };
        let user = ShortcutFile {
            bindings: vec![binding("app.quit", &[])],
        };
        let resolved = resolve(&system, &user, &["app.quit"]);
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
        let resolved = resolve(&system, &user, &["app.quit"]);
        assert!(resolved.is_empty());
    }

    #[test]
    fn platform_filtering_selects_entries() {
        let mut entry = binding("app.quit", &["cmd-q"]);
        entry.platforms = vec!["no-such-platform".to_string()];
        let system = ShortcutFile {
            bindings: vec![binding("app.quit", &["alt-f4"]), entry],
        };
        let resolved =
            resolve(&system, &ShortcutFile::default(), &["app.quit"]);
        assert_eq!(resolved[0].keys, vec!["alt-f4".to_string()]);
    }

    #[test]
    fn duplicate_keys_are_deduplicated() {
        let system = ShortcutFile {
            bindings: vec![
                binding("app.quit", &["cmd-q", "cmd-q", " alt-f4 "]),
                binding("app.quit", &["cmd-q"]),
            ],
        };
        let resolved =
            resolve(&system, &ShortcutFile::default(), &["app.quit"]);
        assert_eq!(
            resolved[0].keys,
            vec!["cmd-q".to_string(), "alt-f4".to_string()]
        );
    }

    #[test]
    fn set_user_command_round_trips_entries() {
        let mut file = ShortcutFile {
            bindings: vec![binding("app.quit", &["cmd-q"])],
        };
        set_user_command(&mut file, "app.quit", Some(vec![]));
        assert_eq!(file.bindings.len(), 1);
        assert!(file.bindings[0].keys.is_empty());
        set_user_command(&mut file, "app.quit", Some(vec!["ctrl-q".into()]));
        assert_eq!(file.bindings.len(), 1);
        assert_eq!(file.bindings[0].keys, vec!["ctrl-q".to_string()]);
        set_user_command(&mut file, "app.quit", None);
        assert!(file.bindings.is_empty());
    }

    #[test]
    fn documents_round_trip_through_json() {
        let file = ShortcutFile {
            bindings: vec![binding("app.quit", &["cmd-q"])],
        };
        let text = serde_json::to_string(&file).unwrap();
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, file);
    }
}
