# Augur Git

<p align="center">
  <img src="assets/augur-git-logo-lockup.svg" alt="augur-git logo" width="360">
</p>

> Review the change. Decide what becomes history.

Augur Git is an open-source, local-first, review-first desktop Git client for
developers working with coding agents. It turns local repository changes into
clear, navigable diffs so that a human can understand, select, and approve what
will become part of the project's history.

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

## Current capabilities

- **Working-tree review** — inspect staged, unstaged, and untracked files with
  per-file diffs. Stage or unstage individual files or groups of files; discard
  always requires confirmation, and untracked files are permanently deleted
  only when explicitly selected.
- **Native diff experience** — review one file or all changed files with inline
  or side-by-side layouts, syntax-aware rendering, and character-level change
  highlighting.
- **Commit history** — browse a lane-based commit graph with hashes, messages,
  authors, relative dates, changed-file lists, and commit diffs.
- **Revision comparison** — compare branches, tags, commits, or manually entered
  revisions in a dedicated comparison window.
- **Visible Agent connectivity tests** — test any valid configured CLI from
  Settings in a separate interactive terminal and a fresh empty temporary
  directory. The fixed diagnostic challenge does not touch the current
  repository or persist its prompt and transcript.
- **Commit by AI** — choose the current Agent profile from Settings and launch
  a fixed, visible commit operation from the Commit menu. The Agent reviews and
  stages all non-ignored working-tree changes, creates one Conventional Commit,
  and never pushes or changes file contents.
- **Merge by AI and conflict recovery** — start a fixed merge operation from a
  local branch context menu, or hand an ordinary merge conflict to the current
  Agent. Clean-worktree preflight, immutable target IDs, visible PTY output, and
  Git-state verification keep the result explicit; failed merges can be
  aborted or left open for manual resolution.
- **Rebase by AI and pull-rebase recovery** — rebase onto a selected local
  branch through the current Agent, or hand conflicts from ordinary Rebase and
  Pull (Rebase) operations to that Agent. Clean-worktree checks, immutable
  upstream IDs, visible PTY output, and Git-state verification keep the
  history rewrite explicit; failed rebases can be aborted or left open for
  manual resolution.
- **Repository operations** — browse branches, remotes, tags, and stashes, and
  run explicit fetch, pull, push, checkout, branch, and commit operations.
- **Multiple repositories** — keep several repositories open as tabs and return
  to recently opened repositories.
- **Cross-platform interface** — use Catppuccin Mocha by default or switch to
  another Catppuccin variant or GitHub Dark on Windows, macOS, or Linux, with
  English and Simplified Chinese localization.

The system `git` executable is the only runtime dependency for repository
operations; Augur Git does not embed or emulate a separate Git implementation.
Agent connectivity tests are optional and require the corresponding CLI to be
installed by the user. Each test runs in a fresh empty temporary directory and
does not touch an open repository. On macOS and Linux, the directory is placed
under the per-user application data directory so Agent CLIs that restrict
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
See
[`docs/agent-terminal.md`](docs/agent-terminal.md) for profile configuration,
executable lookup, lifecycle rules, and troubleshooting.

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
  (for example `~/.config/augur-git/config.json` on Linux). Window geometry
  and global panel layout are stored alongside it in `augur-git/ui-state.json`.
- Debug builds append application logs to `debug.log` in the working directory;
  release builds append logs to `augur-git/logs/augur-git.log` under the
  platform's standard local data directory (for example
  `~/Library/Application Support/augur-git/logs/augur-git.log` on macOS).
  Release builds never create `debug.log`. `RUST_LOG` can optionally override
  the log level.

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
