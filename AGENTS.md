# AGENTS.md

## Project

This repository contains two independent desktop Git applications:

- **GPUI application** — the original `augur-git` product in the repository
  root, built with Rust and GPUI. It opens local Git repositories, shows status,
  branches, history, commit graphs, and diffs, and runs Git operations on
  explicit user action. Its default `agent` feature adds the Lua extension
  runtime and external coding-agent CLIs in an embedded terminal.
- **Tauri application** — `tauri-app/`, a separate product built with Tauri 2,
  React, and Vite. It has its own frontend, Rust backend, bundle identity,
  settings, and workspace state. It can be developed and installed alongside
  the GPUI application.

Treat them as separate applications when exploring or changing the code. The
root `README.md` documents the GPUI product, while
[`tauri-app/README.md`](tauri-app/README.md) documents the Tauri product and
its commands. The Tauri app's
`src-tauri/crates/augur-core` is a decoupled copy of GPUI domain logic, not a
shared workspace crate; changes to common behavior may need to be applied to
both implementations.

All agent/terminal/Lua-extension code is gated behind the default-on `agent`
Cargo feature; `cargo build --no-default-features` produces a plain Git GUI
with none of that code (or the `alacritty_terminal` and `mlua` dependencies)
compiled in. New code in shared files that references agent-only types, UI,
or behavior must be gated by `#[cfg(feature = "agent")]`; the CI
`check-no-ai` job enforces this.

## GPUI project structure (repository root)

```text
src/main.rs              startup, assets, window
src/lib.rs               run(): CLI parsing, single-instance forwarding, GPUI boot
src/bin/augurgit.rs      secondary CLI entry
src/logging.rs           logger init, log file targets
src/theme.rs             theme
src/dropdown.rs          shared dropdown widget
src/workspace.rs         top-level GPUI state, layout, routing, event coordination
src/workspace/           welcome, settings/, tabs, preferences, app_menu,
                         repo_tab/ (per-repo page: layout, branch_ops, dialogs),
                         about, remote_open, wsl_open_dialog, persistence,
                         keymap, focus_refresh, window_lifecycle, agent_* 
src/git/mod.rs           GitView: worker handle + snapshot event dispatch (no rendering)
src/git/                 git page panels: sidebar, toolbar, panel, changes_panel,
                         diff_view, graph, graph_painter, graph_search,
                         bottom_panel/, branch_compare, revision_picker,
                         commit_message_dialog, commit_preview
src/core/git.rs          git worker, command execution, output parsers
src/core/git/            commit_log, working_tree, branch_compare, automation,
                         agent_operation, location, progress
src/core/                config, graph (pure layout), i18n, diff/ + commit_diff,
                         commit_search, refs, paths, cli, ipc, keymap,
                         build_info, extension, shell_install
src/agent/               coding-agent profiles, safe launch args, PTY request build
src/extension/           Lua runtime: host, manager, api, builtin, storage, sessions
src/terminal/            embedded PTY terminal view (alacritty_terminal)
i18n/                    translations (en, zh-CN)
assets/                  icons and images
extensions/              bundled Lua extension packages
packaging/               platform packaging scripts
build.rs                 platform build metadata (Windows icon)
```

## Tauri project structure

```text
tauri-app/src/                  React/Vite webview, bridge, components, features
tauri-app/src-tauri/src/        Tauri commands, events, Git worker, persistence
tauri-app/src-tauri/crates/     Decoupled Rust domain crate(s)
tauri-app/tests/                Frontend unit and browser tests
```

Run Tauri frontend commands from `tauri-app/` (for example `bun run typecheck`,
`bun run test`, and `bun run test:e2e`). Its Rust tests use
`cargo test --manifest-path src-tauri/Cargo.toml` from that directory. The root
Cargo manifest and the `agent` feature rules below describe the GPUI product;
do not assume those commands or feature gates apply to the Tauri app.

## Rules

1. All documentation, code comments, and commit messages are written in English.
2. Before adding more responsibility to a source file over 1,000 lines, evaluate
   whether the file should be split or refactored first.

Logs: debug builds write logs to the working directory under `debug-logs/`
(`debug.log` summary plus per-category `debug-*.log` files).
