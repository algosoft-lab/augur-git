# Shortcut Layer

The application resolves keyboard shortcuts from two JSON documents with the
same schema, merged per command:

1. **System settings** (`assets/keymap.default.json`) are compiled into the
   binary with `include_str!`. They define the shipped defaults, currently the
   quit command on `cmd-q` (macOS) and `alt-f4` (Windows and Linux).
2. **User settings** (`keybindings.json` next to `config.json` in the platform
   config directory) may override any command. When the user file contains at
   least one entry for a command on the current platform, those entries replace
   the system entries for that command completely; an entry with an empty
   `keys` list unbinds it. Unknown command ids and unreadable files are logged
   and ignored.

```json
{
  "bindings": [
    { "command": "app.quit", "keys": ["cmd-q"], "platforms": ["macos"] }
  ]
}
```

- `keys` holds keystroke combinations in GPUI syntax: modifiers joined to a
  key with `-` (for example `cmd-q`, `ctrl-k ctrl-s` for a two-keystroke
  sequence). Entries in the list are comma-separated on the settings page.
  An uppercase letter canonicalizes to an explicit `shift` modifier.
- `platforms` accepts `std::env::consts::OS` values (`macos`, `windows`,
  `linux`); an empty list applies everywhere.

## Layers

- `src/core/keymap.rs` owns the schema, merge logic, and file I/O with no GPUI
  dependency so parsing and overriding are unit-testable.
- `src/workspace/keymap.rs` is the GPUI bridge: it maps command ids to actions
  (`app.quit` reuses the existing `Quit` action, which defers while operations
  are running), validates keystrokes, installs the merged keymap before the
  native menu is built, and applies later edits as deltas.

GPUI keeps one shared app-level keymap. Bindings added later win ties, and a
`NoAction` binding suppresses older bindings for the same keystrokes. Remaps
therefore take effect immediately: retired combinations get a `NoAction`
marker, then new combinations bind their action, without clearing keymap
entries owned by other code (gpui-component widgets add their own bindings).
Because `App::set_menus` reads the keymap, native menu items and the in-window
menu pick up key equivalents such as the macOS Cmd-Q accelerator.

## Settings UI

The Shortcuts section of the settings page lists every registered command with
an editable text field (comma-separated combinations) and a Reset button that
removes the user override. Edits commit on Enter or blur so partial keystroke
sequences are never bound mid-typing; invalid combinations show an inline
error and the previous binding stays active.
