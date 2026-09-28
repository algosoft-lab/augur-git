# Tauri application instructions

This directory is the primary, standalone Tauri 2, React, and Vite application.
Its frontend, Rust workspace, assets, dependencies, lock files, build settings,
and runtime configuration belong only to this application. Do not depend on
files from `gpui-app/` or import that application's settings.

Run frontend and Tauri commands from this directory. Run Rust workspace tests
with `cargo test --manifest-path src-tauri/Cargo.toml`. See this directory's
README for prerequisites, development commands, testing, architecture, and data
locations.

The root `AGENTS.md` applies as repository-wide guidance. Keep this file and
the Tauri README accurate when its architecture or development commands change.
