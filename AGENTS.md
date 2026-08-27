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

## 1. Language

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

## 2. Product scope and repository boundaries

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

## 3. Architecture and dependency direction

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

## 4. Cross-platform requirements

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

## 5. Logging and debugging

- Debug builds MUST write application logs to `debug.log` by default. Running
  the application MUST NOT require stdout or stderr redirection to capture
  logs.
- Release builds MUST NOT create or write `debug.log`. Release logging must be
  disabled or sent to an explicitly approved non-terminal destination.
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
  exercises the relevant flow and filters `debug.log` into a focused log file.
  For example:

  ```bash
  cargo run
  rg "\[(git_view|workspace|git_command)\]" debug.log > git-debug.log
  ```

- Generated `*.log` files MUST remain untracked and MUST NOT be included in
  commits or release archives.

## 6. Git data safety and parsing

- Validate repository paths before opening them and report repository-specific
  failures with useful context.
- Parse Git status, branch, log, ref, numstat, and diff output defensively.
  Account for empty output, merge commits, renamed paths, non-ASCII names,
  binary files, detached HEAD, missing upstreams, and unexpected fields.
- Do not use `panic!`, `unwrap`, or `expect` for malformed user input,
  repository content, or Git command output. Propagate or present structured
  errors instead.
- Do not infer destructive repository actions from filenames, display labels,
  or ambiguous parser results. Require explicit command arguments and clear
  user intent.
- Keep parser and transformation logic pure where possible so it can be tested
  without a GUI or a live repository.
- After changing a write operation or output parser, verify the resulting state
  or round-trip through the relevant reader before reporting success.
- Do not silently discard unknown Git metadata, refs, file states, or diff
  sections outside the requested presentation. Mark unsupported cases and
  preserve data whenever the operation permits.

## 7. Code organization and file size

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

## 8. Required validation

- Before every commit, run `cargo fmt --all`. This is mandatory even when the
  change appears not to affect formatting.
- After formatting, run the most relevant automated checks. `cargo test` is the
  minimum default for Rust behavior changes; use `cargo check --all-targets`
  when a full test run is not applicable.
- For Git command, parser, graph, or state changes, add or update focused unit
  tests and run the relevant fixtures or round-trip checks.
- Do not report a check as successful unless it was actually run. Clearly state
  any check that could not be completed and why.
- GUI behavior that cannot be validated reliably in the agent environment MUST
  be handed off with concise, platform-neutral manual verification steps.
- Do not make Windows-only manual verification the canonical acceptance path for
  cross-platform behavior.
- Before declaring work complete, check `git diff --check`, inspect the final
  diff, and confirm generated artifacts are not included.

## 9. Documentation and task tracking

- `README.md` contains the product overview, supported scope, and developer
  entry points. Keep detailed design decisions in focused English documents
  under `docs/` when such documentation is needed.
- Feature plans, migration notes, release notes, historical investigations,
  and manual test procedures belong in dedicated English documents under
  `docs/`.
- Keep `AGENTS.md` free of feature plans, milestone checklists, copied design
  specifications, and historical implementation notes.
- Do not recreate removed legacy documents or maintain a stale checklist of
  completed tasks. When a task is complete, remove its pending entry from the
  relevant planning document.
- Documentation MUST describe actual behavior. Clearly label planned behavior,
  unsupported input, experimental features, and platform-specific limitations.

## 10. Commits

- Every commit MUST use a complete Conventional Commits message:
  `<type>(optional-scope): imperative summary`.
- Use the narrowest accurate type, such as `feat`, `fix`, `refactor`, `docs`,
  `test`, `build`, `ci`, or `chore`. Vague subjects such as `update files` or
  `misc fixes` are forbidden.
- Non-trivial commits MUST include a body explaining the motivation, behavior
  change, and important compatibility or validation details.
- Breaking changes MUST use `!` in the header or a `BREAKING CHANGE:` footer.
- Do not commit, amend, push, or create a pull request unless the user
  explicitly requests it.

## 11. Safety and repository hygiene

- Inspect `git status` before editing. Preserve unrelated user changes and do
  not rewrite them.
- Never commit generated logs, credentials, private keys, build artifacts,
  local profile databases, or user repository outputs.
- Destructive or irreversible commands require explicit user approval. Confirm
  exact targets before deleting or overwriting files.
- Use `rg` for text search, `fd` for file discovery, and `uv run` for Python
  commands. Prefer `apply_patch` for source and documentation edits.
- Do not create or switch Git branches unless the user explicitly requests it.
- Keep changes focused. Do not mix unrelated cleanup, refactoring, and feature
  work in one commit.
