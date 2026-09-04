# Augur Git

<p align="center">
  <img src="assets/augur-git-logo-lockup.svg" alt="augur-git logo" width="360">
</p>

> Review the change. Decide what becomes history.

Augur Git is an open-source, local-first, review-first desktop Git client for
developers working with coding agents. It combines a focused working-tree
review surface, explicit local Git operations, visible Agent sessions, and
optional Lua workflow automation.

It turns local repository changes into clear, navigable diffs so that a human
can understand, select, and approve what will become part of the project's
history.

It is designed to sit beside a terminal, an editor such as Neovim, and a coding
agent. It does not try to replace any of them. The terminal runs the project,
the editor handles focused changes, the agent produces larger changes, and
Augur Git provides the visual review surface between those changes and the next
commit.

## Why Augur Git exists

AI-assisted development has made producing code much faster, but understanding
and accepting code still requires human judgment. A coding agent can change
many files in a few minutes; the developer remains responsible for deciding
whether those changes are correct, coherent, and safe to keep.

Augur Git grew out of a practical workflow:

1. Run the application and development tools in a terminal.
2. Use a coding agent for substantial implementation work.
3. Make small, precise corrections in an editor.
4. Inspect the resulting Git changes before staging, committing, or pushing.

The missing piece was a fast, focused Git interface where review is the primary
activity rather than a secondary view hidden among repository-management
features.

The name reflects that idea. An augur reads signs to understand what may come
next. In Git, the commit graph records the past, the working tree contains a
possible future, and the developer decides which changes become history.

## Product definition

Augur Git is a **review-first Git client for local, AI-assisted development**.

Its primary job is to help a developer answer four questions:

- What changed?
- Why does the change matter?
- Which parts should be kept, revised, or discarded?
- Is the repository ready for the next commit or push?

Augur Git is not a coding agent, a code editor, a hosted pull-request platform,
or an automated judge of code quality. It complements those tools by providing
a dedicated local review layer:

```text
Coding agent or editor
          |
          v
   Working tree changes
          |
          v
      Augur Git review
          |
          +----> Return issues to the editor or agent
          |
          v
   Stage, commit, and push
```

## Design principles

- **Review comes first** — diffs, changed files, and repository context are the
  center of the experience, not an afterthought.
- **Complement the existing workflow** — Augur Git should work naturally beside
  terminals, editors, and coding agents instead of absorbing their roles.
- **Keep the human in control** — repository-changing operations require an
  explicit user action and surface their result or error.
- **Stay local by default** — repository inspection runs through the system
  `git` executable. Augur Git has no account requirement and does not need a
  hosted service to review local changes.
- **Make large changes understandable** — navigation, layout, syntax-aware
  rendering, and responsive performance should make multi-file agent changes
  practical to review.
- **Build in the open** — the application, its design decisions, and its
  development process are available for users to inspect and improve.

## Features

### Review working-tree changes

- **Working-tree review** — inspect staged, unstaged, untracked, and conflicted
  files with per-file or all-files diffs. Stage and unstage individual files or
  groups; discard tracked changes only after confirmation, and delete untracked
  files only through an explicit action.
- **Native diff workspace** — switch between inline and side-by-side layouts
  with syntax-aware rendering, character-level highlights, binary-file
  handling, and diff-specific font settings. Side-by-side rendering adapts to
  narrow windows.
- **Commit workflow** — compose a commit while reviewing the staged diff,
  create a normal commit, amend the latest commit, or use the fixed Commit by
  AI action. The preferred commit action can be saved and shared across
  repository tabs.

### Explore history and repository state

- **Paged commit graph** — browse a lane-based history with hashes, messages,
  authors, relative dates, refs, changed-file lists, and commit diffs. History
  can cover all local branches, remote-tracking branches, tags, and `HEAD`, or
  be limited to the current branch and its tracked upstream.
- **Commit search and context** — search by subject or full message with loose,
  case-insensitive matching; inspect full messages and metadata in hover
  details; copy hashes or complete commit messages; and check out a selected
  commit from its context menu.
- **Refs and repository operations** — browse local branches, remote branches
  grouped by remote, tags, and stashes. Explicit actions include checkout,
  branch creation and rename/delete, tag deletion, stash pop/drop, merge,
  merge with `--no-ff`, fetch with pruning, pull with merge or rebase, push,
  force push with confirmation, and refresh. Ahead/behind status is shown for
  tracked branches.

### Compare and exchange revisions

- **Revision comparison** — compare local or remote-tracking branches, tags,
  commits from history, or manually entered 7–64 character revisions in a
  dedicated, resizable window. Comparison reads both endpoints directly and
  does not check out or fetch them.
- **Comparison tools** — select one file or all files, switch diff layout,
  swap endpoints, copy the diff, and save the complete revision diff as a
  patch. Exported patches preserve binary changes and renames where Git can
  represent them.
- **Apply Patch** — choose a patch file from the Branch menu and apply it to
  the current worktree with Git. Applied changes remain visible as unstaged
  changes for review.

See [`docs/branch-comparison.md`](docs/branch-comparison.md) for the detailed
comparison workflow.

### Work with coding Agents

- **Configurable profiles** — use built-in Codex, Claude Code, and OpenCode
  profiles, or define a custom CLI with an explicit executable path, arguments,
  and prompt mode. Executables can be selected from disk or discovered from
  common installation locations and `PATH`.
- **Launch-time model options** — optionally override the model for a session;
  Codex and Claude Code expose reasoning effort, while OpenCode can expose a
  model-specific Variant when the installed CLI supports it. Empty values
  inherit the CLI's environment and configuration.
- **Visible connectivity tests** — test a configured CLI in an interactive
  terminal using a fixed diagnostic challenge in a fresh empty temporary
  directory. Tests never touch the current repository and do not persist
  prompts, transcripts, or session IDs.
- **Commit by AI** — the Agent reviews all staged, unstaged, and untracked
  non-ignored changes, stages them, reviews the staged diff, and creates one
  concise Conventional Commit. It cannot edit file contents, reset or
  checkout, amend, merge, rebase, or push.
- **Merge and rebase assistance** — start merge or rebase operations from
  branch actions, or hand conflicts from ordinary Merge, Rebase, and Pull
  (Rebase) operations to the current Agent. Clean-worktree preflight, visible
  terminal output, explicit conflict recovery, and Git-state verification keep
  these operations reviewable; failures can be aborted or left open for manual
  resolution.

The Agent terminal is intentionally dedicated to visible connectivity tests and
fixed Git operations rather than a general-purpose shell. See
[`docs/agent-terminal.md`](docs/agent-terminal.md) for profile configuration,
executable lookup, lifecycle rules, and troubleshooting.

### Automate repeatable workflows with Lua

The standalone Extensions window manages trusted Lua 5.4 packages, settings,
manual runs, scheduling, cancellation, and run history. The bundled
`sync-open-tabs` extension is an example workflow that can pull with rebase,
recover conflicts through the configured Agent, commit dirty worktrees, and
push without force-pushing. Additional local packages are discovered from the
application data directory; there is no online marketplace in the current
version. See [`docs/extensions.md`](docs/extensions.md) for the package format,
events, permissions, and host API.

### Customize the workspace

- **Repositories and layout** — open multiple repositories as tabs, use the
  start page's folder picker or folder drop, return to up to eight recent
  repositories, and refresh automatically on focus or tab switches. Sidebar,
  file-list, right-panel, and window geometry are resizable and persisted.
- **Appearance and language** — choose GitHub Dark or any Catppuccin theme,
  select UI and monospace fonts, adjust UI and diff font sizes, choose inline
  or side-by-side diff as the default, and switch between English and
  Simplified Chinese.
- **Keyboard shortcuts** — customize registered commands in
  `keybindings.json`, with live validation and reset support. The default quit
  shortcut is Cmd-Q on macOS and Alt-F4 on Windows and Linux. See
  [`docs/shortcuts.md`](docs/shortcuts.md).

The system `git` executable is the only runtime dependency for repository
operations; Augur Git does not embed or emulate a separate Git implementation.
Agent features are optional and require the corresponding CLI to be installed
by the user. Each connectivity test runs in a fresh empty temporary directory
and does not touch an open repository. On macOS and Linux, the directory is
placed under the per-user application data directory so Agent CLIs that restrict
system temporary locations can access it; Windows keeps using the system
temporary directory.

Built-in Agent profiles can optionally override the model at launch. Codex and
Claude Code expose a per-session reasoning-effort setting, while OpenCode
accepts a model-specific Variant name when the installed root TUI advertises
`--variant`. Use OpenCode's `opencode models` or `/models` command to discover
available model and Variant values. Current OpenCode releases expose this flag
for `opencode run`, but not for the root interactive TUI; Augur detects that
case and reports it in the visible test window.
Leaving these fields empty preserves the CLI's environment and configuration
defaults.
Augur Git does not inject generic permission or mode flags.

The Agent terminal is intentionally dedicated to visible connectivity tests and
fixed Git operations rather than a general-purpose shell. Test state, operation
state, and terminal transcripts are not persisted across application restarts.
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
   cargo run
   ```

   Use the release profile when preferred:

   ```bash
   cargo run --release
   ```

### Configuration and logs

- Settings and the recent-repository list are stored in
  `augur-git/config.json` under the platform's standard user config directory
  (for example `~/.config/augur-git/config.json` on Linux). Window geometry
  and global panel layout are stored alongside it in `augur-git/ui-state.json`.
- Debug builds write file-only diagnostic logs in the working directory,
  including `debug.log` and per-domain files for application, Git, Agent,
  extension, terminal, and system events. Release builds write the
  corresponding files under the platform's standard local data directory and
  never create `debug.log`. `RUST_LOG` can optionally override the log level.
  See [`docs/logging.md`](docs/logging.md) for the complete file list and
  rotation behavior.

### Packaging

Native packaging scripts for Windows, macOS, and Linux are documented in
[`packaging/README.md`](packaging/README.md). They create an Inno Setup
installer, a macOS `.app` and `.dmg`, or a Linux AppImage respectively.

## Architecture

Dependencies flow from UI and rendering toward application state and domain
services. UI code never invokes Git processes or parses raw Git output
directly. A background Git worker communicates with the UI through explicit
message-passing boundaries, so blocking Git and filesystem work never runs on
the UI thread.

```text
src/
├── main.rs          # Process startup, assets, window setup, application entry
├── workspace.rs     # Top-level state, layout, routing, and event coordination
├── workspace/       # Tabs, repository state, dialogs, settings, and windows
├── core/            # Configuration, Git worker, parsers, graph, diff, and i18n
├── git/             # GPUI presentation for Git history, changes, and diffs
├── agent/            # External Agent profiles and secure launch specs
├── terminal/         # Alacritty PTY state machine and Agent terminal view
├── extension/        # Lua runtime, package manager, host bridge, and workers
└── theme.rs         # Embedded themes and runtime theme switching
i18n/                # English and Simplified Chinese translations
assets/              # Logos, interface icons, and theme definitions
packaging/           # Cross-platform native packaging scripts
```

Parser and transformation logic stays pure where possible so it can be tested
without a GUI or a live repository. Repository operations that can change user
data are only initiated by explicit user actions, and their results or errors
are surfaced in the UI.

## Open source

Augur Git is licensed under the [Apache License 2.0](LICENSE). Contributions,
bug reports, design discussions, and workflow feedback are welcome.

Augur Git is part of the [AlgoSoft](https://algosoft.cc) family of open-source
developer tools. Each project can have its own identity while sharing the same
commitment to speed, transparency, local workflows, and user control.
