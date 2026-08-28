# augur-git

<p align="center">
  <img src="assets/augur-git-logo-lockup.svg" alt="augur-git logo" width="360">
</p>

augur-git is a cross-platform desktop Git GUI client built with Rust and
[GPUI](https://github.com/zed-industries/zed). It opens local Git repositories
and presents repository status, branches, history, a commit graph, and diffs,
and runs user-requested Git operations such as fetch, pull, push, checkout,
and commit.

The system `git` executable is the only runtime dependency for repository
operations; augur-git does not embed a Git implementation.

## Features

- **Repository status** — current branch, ahead/behind counts, and staged and
  unstaged changes, with commit for staged changes.
- **Commit graph** — lane-based history graph with hash, message, author, and
  relative dates; select a commit to inspect its file list and per-file diffs.
- **Branches, remotes, tags, and stashes** — browsable sidebar sections, with
  fetch, pull, and push controls in the toolbar.
- **Multiple repositories** — open several repositories as tabs; recent
  repositories are remembered.
- **Themes** — GitHub Dark plus Catppuccin Latte, Frappé, Macchiato, and Mocha,
  switchable at runtime and persisted.
- **Bilingual UI** — English and Simplified Chinese (Fluent-based
  localization), with automatic system-locale detection.

## Supported platforms

| Platform | Status |
| -------- | ------ |
| Windows  | Supported (embedded application icon) |
| macOS    | Supported |
| Linux    | Supported (X11 and Wayland) |

Building requires a recent stable Rust toolchain (edition 2024).

## Getting started

1. Install a stable Rust toolchain via [rustup](https://rustup.rs).
2. Make sure `git` is available on `PATH`.
3. Build and run:

   ```bash
   cargo run --release
   ```

### Configuration and logs

- Settings and the recent-repository list are stored in
  `augur-git/config.json` under the platform's standard user config directory
  (for example `~/.config/augur-git/config.json` on Linux).
- Debug builds append application logs to `debug.log` in the working
  directory; release builds do not write a log file. `RUST_LOG` can optionally
  override the log level.

### Packaging

Native packaging scripts for Windows, macOS, and Linux are documented in
[`packaging/README.md`](packaging/README.md). They create an Inno Setup
installer, a macOS `.app` and `.dmg`, or a Linux AppImage respectively.

## Architecture

Dependencies flow from UI and rendering toward application state and domain
services. UI code never invokes Git processes or parses raw Git output
directly; a background Git worker communicates with the UI through explicit
message-passing boundaries, so blocking Git and filesystem work never runs on
the UI thread.

```text
src/
├── main.rs          # Process startup, assets, window setup, application entry
├── workspace.rs     # Top-level GPUI state, layout, routing, event coordination
├── theme.rs         # Embedded themes and runtime theme switching
├── workspace/
│   ├── tabs.rs      # Repository tab bar
│   ├── repo_tab.rs  # Per-repository tab state
│   └── welcome.rs   # Welcome page and settings overlay
├── core/
│   ├── commit_diff.rs # Commit diff context and Git argument builders
│   ├── config.rs    # Persisted application and repository settings
│   ├── git.rs       # Git worker, command execution, events, output parsers
│   ├── graph.rs     # Pure commit-graph layout and time formatting
│   └── i18n.rs      # Locale selection and translation lookup
└── git/
    ├── mod.rs       # GitView bridge between the worker and UI panels
    ├── graph.rs     # Commit-graph presentation
    ├── panel.rs     # Commit details, commit input, file list, diff panels
    ├── sidebar.rs   # Repository, branch, staging, working-tree sections
    └── toolbar.rs   # Git operation controls and status indicators
i18n/               # English and Simplified Chinese translations (Fluent)
assets/             # Logos, interface icons, and theme definitions
```

`src/core/graph.rs` keeps commit-graph layout as pure, unit-tested logic
without rendering concerns. Diff context and Git argument construction live in
`src/core/commit_diff.rs`, while the status, log, and diff output parsers are
exercised by unit tests without a GUI or live repository.

Repository operations that can change user data are only initiated by explicit
user actions, and their results or errors are surfaced in the UI.

## Development

```bash
cargo fmt --all          # required before every commit
cargo test               # unit tests for parsers, graph layout, config, i18n
cargo check --all-targets
```

Debug builds write logs to `debug.log`. To investigate an area, run the app,
exercise the relevant flow, and filter by the module's log prefix:

```bash
cargo run
rg "\[(git_view|workspace|git_command)\]" debug.log > git-debug.log
```

Engineering policy, architecture rules, and validation requirements are
described in [AGENTS.md](AGENTS.md). Design documents and feature plans belong
in dedicated documents under `docs/` when needed.
