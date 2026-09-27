# Augur Git Tauri

A second, independent desktop Git client for this repository, built with
Tauri 2, React, and Vite. It is a separate product from the GPUI application in
the repository root: different name, different bundle identifier, different
configuration directory, and a different shell command. Both can be installed
and run side by side.

Its goal is feature parity with the GPUI application built
`--no-default-features`. The coding-agent integration, the embedded terminal,
and the Lua extension runtime are intentionally out of scope.

| | GPUI application | This application |
|---|---|---|
| Product name | Augur Git | Augur Git Tauri |
| Bundle identifier | `com.augur.git` | `com.augur.git.tauri` |
| Desktop binary | `augur-git` | `augur-git-tauri` |
| Shell command | `augurgit` | `augurgit-tauri` |
| Settings | `augur-git/config.json` etc. | Tauri app data dir, `settings.json` and `workspace.json` |

Because the identifiers differ, neither application can read, write, or migrate
the other's files. There is no configuration import: each product starts from
its own defaults.

## Layout

```text
tauri-app/
  index.html                 # one document served to every window
  vite.config.ts
  scripts/sidecar.mjs        # builds the CLI companion into src-tauri/binaries
  src/                       # the webview
    bridge/                  # the only module that talks to Tauri
    components/              # menus, dialogs, splitters, virtual list
    features/                # one folder per area of the interface
    i18n/                    # catalog lookup
    styles/                  # theme tokens and layout
  src-tauri/
    tauri.conf.json
    capabilities/default.json
    crates/augur-core/       # pure Rust Git domain logic, no UI
    src/
      commands/              # the typed surface exposed to the webview
      events.rs              # the event protocol
      git_args.rs            # named operations to Git argument vectors
      menu.rs                # the native menu
      persistence.rs         # the two stored documents
      repo.rs                # one open repository: worker plus event forwarder
      state.rs               # application-wide state
```

`augur-core` is a decoupled copy of the GPUI application's domain layer: Git
argument construction, output parsers, commit-graph layout, diff parsing, and
the read-only state probes. It contains no user-interface code, which is what
makes it reusable from a Tauri command.

## Requirements

- Rust 1.90 or newer (the workspace uses edition 2024)
- Node.js 20 or newer
- The platform webview development packages, listed in
  `src-tauri/Cargo.toml` and the Tauri prerequisites

## Commands

```bash
npm install          # frontend dependencies
npm run tauri:dev    # build the CLI companion, then run the app in dev mode
npm run tauri:build  # produce a platform bundle
npm run typecheck    # TypeScript, no emit
npm test             # unit tests for the pure interface logic
npm run test:e2e     # browser tests for the whole interface
npm run test:all     # all three, in order
cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit tests
```

`npm run tauri:dev` and `npm run tauri:build` run `scripts/sidecar.mjs` first.
That script compiles the `augurgit-tauri` companion and copies it to
`src-tauri/binaries/`, where the Tauri bundler expects a sidecar. A bare
`cargo build` has no such hook, so `build.rs` writes a clearly labelled
placeholder instead; the real binary is only ever produced by the npm scripts.

## Testing

Three layers, each covering what the others cannot.

**Rust unit tests** cover the domain crate: argument construction, output
parsers, commit-graph layout, diff parsing, and the read-only probes. They need
no window and no repository.

**Rust pipeline tests** create a throwaway repository, drive the real `git`
binary through the real worker, and assert on the events that reach the
interface. They reach no private parser, so a wrong argument vector fails them
too, which a test of a parser alone cannot do. Each one returns early when `git`
is not on the path.

**Frontend unit tests** cover the parts of the interface with no window: commit
search, branch-name validation, porcelain status classification, the stat bar,
remote-branch grouping, catalog lookup, the theme writer, and the syntax
tokenizer.

**Browser tests** drive the real interface. The webview is ordinary web code,
so `tests/fixtures/stubBackend.ts` installs a stub Tauri runtime before the
application loads and the whole thing runs in Chromium. That means the tests
exercise the real components, the real store, and the real event reducers; only
the boundary is replaced. The stub serves fixture repositories with a linear
history and a merge, untracked and conflicted files, several refs, and a diff
whose character-level ranges exercise inline highlighting, and it records every
command so a test can assert that a guard really did prevent one.

This layer is worth its cost. It found a race that unit tests cannot see: the
backend starts a worker thread inside `open_repository`, and Tauri makes no
ordering promise between an event and the command's own reply, so the first
status snapshot could arrive before the webview knew the repository existed. It
also found two functions with the same name and different meanings, a path
rendered right to left, and controls clipped out of reach.

## Architecture notes

**The webview never runs Git.** Every mutating action is a named operation
(`stageFiles`, `merge`, `pushForce`, …). The backend turns the name into a
fixed argument vector, so the frontend cannot be talked into running an
arbitrary command and a repository path can never be reinterpreted as command
syntax. Destructive operations still require the same confirmation dialogs as
the reference application, and the backend re-checks repository identity,
operation state, and paths.

**Events carry an identity.** Every repository event carries `repoId` and, where
a reply can arrive late, a request id. The webview discards results that are
not the newest it asked for, so switching files or comparison endpoints quickly
cannot display a stale diff.

**Git work runs off the UI thread.** Each open repository owns a worker thread
that executes commands serially, and a second thread forwards its events to the
webview. Read-only preflight probes run on Tauri's blocking pool.

**Two stored documents.** `settings.json` holds preferences and shortcut
overrides; `workspace.json` holds the open tabs, the active tab, and the pane
geometry. Both carry a `schemaVersion`, and both are validated on load: a
corrupt document degrades to defaults in memory and is reported instead of being
silently overwritten.

## Data locations

`settings.json` and `workspace.json` are created by the Tauri store plugin in
this application's data directory:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/com.augur.git.tauri/` |
| Windows | `%APPDATA%\com.augur.git.tauri\` |
| Linux | `~/.config/com.augur.git.tauri/` |

Logs are written to the platform log directory. The About window reports the
resolved store paths.
