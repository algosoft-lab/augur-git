# AGENTS.md

## Project

`augur-git` is a cross-platform desktop Git GUI built with Rust and GPUI. It
opens local Git repositories, shows status, branches, history, commit graphs,
and diffs, runs Git operations on explicit user action, and supports a Lua
extension runtime and launching external coding-agent CLIs in an embedded
terminal.

## Structure (quick lookup)

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

## Rules

1. All documentation, code comments, and commit messages are written in English.
2. Before adding more responsibility to a source file over 1,000 lines, evaluate
   whether the file should be split or refactored first.

Logs: debug builds write logs to the working directory under `debug-logs/`
(`debug.log` summary plus per-category `debug-*.log` files).
