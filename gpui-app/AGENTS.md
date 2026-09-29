# GPUI application instructions

This directory is the standalone legacy Rust and GPUI application. Active
development is focused on `tauri-app/`. Unless a request explicitly asks for
the GPUI version, application development requests target Tauri. Its Cargo
workspace, dependencies, assets, translations, Lua extensions, packaging
scripts, and runtime configuration belong only to this application. Do not
depend on files from `tauri-app/`.

Run GPUI commands from this directory. The default `agent` feature includes the
embedded terminal and Lua extension runtime; `--no-default-features` builds the
plain Git GUI. Keep agent-only code behind the `agent` feature gate.

The root `AGENTS.md` applies as repository-wide guidance. Keep this file and
the GPUI README accurate when its architecture or development commands change.
