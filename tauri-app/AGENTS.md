# Tauri application instructions

This directory is the primary, standalone Tauri 2, React, and Vite application.
Its frontend, Rust workspace, assets, dependencies, lock files, build settings,
and runtime configuration belong only to this application. Do not depend on
files from `legacy-app/` or import that application's settings.

Run frontend and Tauri commands from this directory. Run Rust workspace tests
with `cargo test --workspace --manifest-path src-tauri/Cargo.toml`. See this directory's
README for prerequisites, development commands, testing, architecture, and data
locations.

The root `AGENTS.md` applies as repository-wide guidance. Keep this file and
the Tauri README accurate when its architecture or development commands change.

## Frontend architecture

The interface is runtime-agnostic and serves both the desktop webview and
browsers:

- `src/bridge/transport.ts` defines the single backend seam: named commands
  with JSON arguments plus named events with JSON payloads. Two
  implementations exist — the Tauri runtime and the `augur-server` HTTP API
  with its event websocket — selected at runtime by probing
  `window.__TAURI_INTERNALS__`.
- `src/bridge/ipc.ts` is still the only module the rest of the interface
  imports for backend access; it never calls a transport directly.
- `src/platform/` wraps every capability a browser cannot reach directly
  (clipboard, opening URLs, folder pickers, window chrome, updater and CLI
  availability). Feature code imports these helpers, never a Tauri plugin.
- Desktop-only surfaces (window controls, title-bar drag, updater, CLI
  installation) must gate on the flags from `src/platform/` so the browser
  build renders cleanly.
- Backend command bodies belong in `crates/augur-service`: plain functions
  over `AppState`. A `#[tauri::command]` in `src-tauri/src/` is a thin
  wrapper, and the server dispatcher in
  `crates/augur-server/src/dispatch.rs` maps the same command names onto the
  same functions. When you add a command, add both wrappers or the two
  frontends drift apart.
