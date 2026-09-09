# Repository Engineering Policy

These rules are mandatory for every change in this repository. Keep this file
focused on durable engineering policy. Feature plans, release notes, historical
investigations, and manual test cases belong in dedicated documents under
`docs/`.

## Project

`augur-git` is a cross-platform desktop Git GUI built with Rust and GPUI. It
opens local Git repositories, presents repository status, branches, history,
commit graphs, and diffs, and runs user-requested Git operations.

## Project structure

```text
src/
├── main.rs          # Process startup, assets, window setup, and application entry
├── workspace.rs     # Top-level GPUI state, layout, routing, and event coordination
├── workspace/
│   └── welcome.rs    # Welcome page and settings overlay rendering
├── core/
│   ├── config.rs    # Persisted application and repository settings
│   ├── git.rs       # Git worker, command execution, events, and output parsers
│   ├── graph.rs     # Pure commit-graph layout and time-formatting logic
│   └── i18n.rs      # Locale selection and translation lookup
└── git/
    ├── mod.rs       # GitView bridge between the worker and UI panels
    ├── graph.rs     # Commit-graph presentation
    ├── panel.rs     # Commit details, commit input, file list, and diff panels
    ├── sidebar.rs   # Repository, branch, staging, and working-tree sections
    └── toolbar.rs   # Git operation controls and status indicators
i18n/               # User-facing English and Simplified Chinese translations
assets/              # Logos and local interface icons
packaging/           # Packaging-specific assets and scripts
build.rs             # Platform-specific build metadata, including the Windows icon
```

## Language

- All source-code comments, doc comments, commit messages, and newly created
  or updated documentation MUST be written in English.
- Chinese text MUST NOT be added to comments or engineering documentation. When
  touching an existing non-English comment or documentation section, translate
  the affected text to English as part of the same change.
- Localized user-facing strings are exempt from this rule. Keep localization
  content in `i18n/` and separate from engineering documentation whenever
  practical.
- Names and prose MUST be clear enough to explain intent. Do not add comments
  that merely restate the code.

## Product scope and repository boundaries

- This repository is a desktop client for local Git repositories. Keep Git as
  the supported version-control system unless the product scope is explicitly
  changed.
- The system `git` executable is an external runtime dependency. Do not add
  silent fallback behavior that changes repository semantics or invokes another
  VCS tool without an explicit scope decision.
- Repository operations that can change user data, including commit, checkout,
  fetch, pull, push, staging, and reset, MUST be initiated by an explicit user
  action and MUST surface their result or error.
- Pass Git arguments as structured arguments to `std::process::Command`. Do not
  build shell command strings, invoke a shell for routine operations, or allow
  repository paths and user input to become command syntax.
- Treat repository contents, paths, refs, and Git output as untrusted input.
  Operations that cannot be validated safely MUST fail with a useful error
  instead of silently changing or corrupting repository state.
- Preserve user-visible paths and command output accurately where possible.
  Do not silently discard statuses, refs, diff data, or parser fields merely
  because they are unfamiliar; handle unsupported cases explicitly.

## Architecture and dependency direction

Dependencies flow from UI and rendering toward application state and domain
services, never in the opposite direction.

- `src/main.rs` owns process startup, asset registration, window creation, and
  the application entry point. Keep it thin.
- `src/workspace.rs` owns top-level state, layout, page routing, configuration
  coordination, and event wiring between panels and `GitView`.
- Modules under `src/git/` own GPUI presentation and user intent. UI code MUST
  NOT invoke Git processes, parse raw Git output, or block on filesystem work.
- `src/core/git.rs` owns the Git worker boundary, command execution, event
  payloads, and pure parsers for status, log, and diff output.
- `src/core/graph.rs` owns pure commit-graph layout and related calculations;
  keep rendering details out of it.
- `src/core/config.rs` owns persisted settings and recent-repository state.
  `src/core/i18n.rs` owns locale resolution and translation lookup.
- `GitView` is the bridge between background Git events and UI panels. Keep
  panel state local to the owning panel and route cross-panel coordination
  through `Workspace` events.
- Background workers MUST receive owned or immutable job inputs and communicate
  with the UI through `std::sync::mpsc` or an equivalent explicit boundary.
  Blocking Git and filesystem work MUST NOT run on the UI thread.
- Keep public APIs small and predictable. Avoid global mutable state, circular
  module dependencies, and convenience modules that become dumping grounds.

## Cross-platform requirements

- New functionality MUST support every maintained platform unless the task
  explicitly narrows its scope.
- A Windows-only toolchain, PowerShell script, batch file, registry operation,
  or Win32 command MUST NOT be the sole implementation of a build, test,
  development, or maintenance workflow.
- Prefer portable Rust code and established cross-platform crates. Isolate
  unavoidable platform-specific behavior behind explicit `cfg` boundaries and
  provide equivalent behavior for other maintained platforms.
- For repository automation that cannot reasonably be implemented in Rust,
  prefer a Python script using the standard library. Invoke Python tooling with
  `uv run`. Do not create parallel shell, PowerShell, and batch implementations
  when one portable script can serve all platforms.
- Platform-specific packaging scripts are allowed inside the relevant packaging
  workflow. They MUST NOT become prerequisites for normal development on other
  platforms.
- Do not introduce environment variables for routine configuration when a
  command-line option, configuration file, or stable application default is
  sufficient. Any required environment variable MUST be documented and kept to
  the narrowest possible scope.
- Use `std::path::Path` and `PathBuf` for filesystem paths. Do not hardcode path
  separators, drive letters, home directories, or platform-specific executable
  suffixes in shared code.
- Use cross-platform file dialogs, window APIs, image loading, and atomic file
  replacement. Do not make a GUI acceptance path depend on one operating
  system.

## Logging and debugging

- Debug builds MUST write file-only application logs under `debug-logs/` in the
  working directory by default. The application MUST create that directory as
  needed; the summary file is `debug-logs/debug.log` and category files use
  the `debug-*.log` naming convention. Running the application MUST NOT
  require stdout or stderr redirection to capture logs.
- Release builds MUST NOT create or write the local `debug-logs/` files.
  Release logging must use the platform's standard local application-data log
  directory or be disabled.
- Normal application logging MUST NOT write to the terminal. Startup must remain
  resilient if the log file cannot be created.
- `RUST_LOG` may be used as an optional log-level override, but the application
  MUST provide a useful default without it.
- Never log passwords, tokens, private keys, credentials, local secrets, or
  complete user-provided paths when they may contain sensitive information.
- Logs added for a feature or investigation MUST use a stable prefix such as
  `[git_view]`, `[workspace]`, or `[git_command]` so they can be filtered
  reliably.
- When handing off a debugging workflow, provide a ready-to-run command that
  exercises the relevant flow and filters the appropriate file under
  `debug-logs/` into a focused log file in the same directory.
  For example:

  ```bash
  cargo run
  rg "\[(git_view|workspace|git_command)\]" \
    debug-logs/debug-app.log debug-logs/debug-git.log > debug-logs/git-debug.log
  ```

- Generated files under `debug-logs/` MUST remain untracked and MUST NOT be
  included in commits or release archives.

## Code organization and file size

- Preserve the existing structure and formatting unless a refactor is part of
  the requested change.
- Every source file over 1,000 lines MUST trigger an explicit design review
  before more responsibilities are added. Evaluate cohesion, dependency
  direction, state ownership, and whether behavior can move to focused modules.
- Do not allow a file to cross the 1,000-line threshold without recording the
  assessment in the change summary or commit body.
- When modifying an existing file that already exceeds 1,000 lines, avoid
  increasing its scope. If the affected behavior has a clear boundary, split it
  during the change. If an immediate split would make the change riskier, state
  the reason and identify the intended module boundary.
- New modules MUST have one clear responsibility. Keep entry points, `mod.rs`
  files, and application coordinators thin.
- Prefer `cargo fmt`-standard Rust, explicit imports in submodules, and
  `Result`-based error propagation with `thiserror`/`anyhow` when appropriate.
- Do not add emojis or unnecessary comments to source, documentation, or
  commit messages.
