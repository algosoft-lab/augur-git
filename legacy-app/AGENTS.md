# Legacy application instructions

This directory contains the standalone legacy Rust application, implemented
with GPUI. Active development is focused on `tauri-app/`. Unless a request
explicitly asks for the legacy version, application development requests target
Tauri. Its Cargo workspace, dependencies, assets, translations, Lua extensions,
packaging scripts, and runtime configuration belong only to this application.
Do not depend on files from `tauri-app/`.

Run GPUI commands from this directory. The default `agent` feature includes the
embedded terminal and Lua extension runtime; `--no-default-features` builds the
plain Git GUI. Keep agent-only code behind the `agent` feature gate.

The root `AGENTS.md` applies as repository-wide guidance. Keep this file and
the legacy README accurate when its architecture or development commands change.
