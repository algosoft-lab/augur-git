# Augur Git

The primary Augur Git application, built with Tauri 2, React, and Vite. This is
an independently maintained application with its own frontend, Rust workspace,
assets, dependency manifests and lock files, build configuration, bundle
identity, and runtime settings.

The legacy application in `gpui-app/` is a separate implementation built with
GPUI. The two applications share no source files, assets, dependency manifests
or lock files, build configuration, or runtime settings. Each resolves
dependencies from its own manifests. Their configuration formats are
incompatible; this application does not read, import, migrate, or write GPUI
settings. It can be installed alongside the legacy application because it has
its own bundle and command identities.

## Layout

```text
tauri-app/
  index.html                 # one document served to every window
  vite.config.ts
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
      drag_drop.rs           # native file-drop routing to application windows
      events.rs              # the event protocol
      git_args.rs            # named operations to Git argument vectors
      menu.rs                # the native menu
      persistence.rs         # the two stored documents
      repo.rs                # one open repository: worker plus event forwarder
      state.rs               # application-wide state
```

`augur-core` is this application's platform-independent domain crate. It owns
the Git argument construction, output parsers, commit-graph layout, diff
parsing, and read-only state probes used by the Tauri backend.

## Main window modes

The main window supports a full desktop layout and a manually selected Sidecar
layout for use beside an editor. Sidecar opens at 420px wide, can be resized
from 360px to 520px, and provides Changes, History, and Branches pages with a
contextual full-page Diff view. Commit drafts, history searches, list positions,
and each mode's window bounds are kept separately per repository or mode as
appropriate. The last selected mode and both sets of window bounds are saved
in the workspace document. Switching modes never changes the standalone
Compare, Settings, or About windows.

## Requirements

- Rust 1.90 or newer (the workspace uses edition 2024)
- Bun 1.2 or newer as the package manager and script runner
- The platform webview development packages, listed in
  `src-tauri/Cargo.toml` and the Tauri prerequisites

## Commands

```bash
bun install          # frontend dependencies
bun run tauri:dev    # run the app in dev mode
bun run tauri:build  # produce a platform bundle
bun run tauri:build -- --bundles app   # one platform's bundle only
bun run format      # format TypeScript and Rust sources
bun run typecheck    # TypeScript, no emit
bun run test         # unit tests for the pure interface logic
bun run test:e2e     # browser tests for the whole interface
bun run test:all     # all three, in order
bun run package:windows # Windows x86-64 NSIS installer
bun run package:macos   # macOS ARM64 app and DMG
bun run package:linux   # Linux x86-64 AppImage, Debian package, and raw archive
cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit tests
```

Run these commands from the `tauri-app/` directory.

## Packaging

The platform packaging commands require the matching host and architecture:
Windows x86-64, macOS ARM64, or Linux x86-64. They write artifacts to
`packaging/out/`. Windows packaging requires NSIS. Linux packaging
requires the Tauri 2 development libraries, including GTK 3 and WebKitGTK 4.1.
The AppImage build runs with extraction mode for hosts without FUSE. The raw
Linux `.tar.gz` contains the GUI executable but relies on compatible system
GTK 3 and WebKitGTK 4.1 runtime libraries.

The macOS packaging command builds the ARM64 app bundle, verifies its
ad-hoc signature, and creates the DMG with `hdiutil` so it can run without a
Finder session. The app is not notarized; macOS may ask users to approve it
before its first launch.

On macOS the `.app` bundle is produced by either form, and the `.dmg` is a
wrapper around the same `.app`. The wrapper is produced by Tauri's
`bundle_dmg.sh`, which drives Finder over AppleScript to position the disk-image
window; that step needs an interactive desktop session, so it fails on a build
machine or over a remote shell. `--bundles app` produces the installable bundle
in that case, and the failure is in the window layout rather than in the
contents.

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
search, branch-name validation, the WSL path check, porcelain status
classification, the stat bar, remote-branch grouping, catalog lookup, the theme
writer, the graph geometry, the node-colour contrast rule, and the syntax
tokenizer.

**Browser tests** drive the real interface. The webview is ordinary web code,
so `tests/fixtures/stubBackend.ts` installs a stub Tauri runtime before the
application loads and the whole thing runs in Chromium. That means the tests
exercise the real components, the real store, and the real event reducers; only
the boundary is replaced. The stub serves fixture repositories with a linear
history and a merge, untracked and conflicted files, a partially staged file,
several refs, and a diff whose character-level ranges exercise inline
highlighting, and it records every command so a test can assert that a guard
really did prevent one. Its runtime lives in its own module because it is
serialised into the page, where the vocabulary and the fixture data are not.

The stub's translation catalog is read from the real `en-US.ftl` rather than
restated. That is deliberate: an early version of these tests carried its own
copy of the strings, and every string it had invented turned into an assertion
that the interface was wrong. Reading the catalog makes that class of mistake
impossible, and it is why the tests now assert wording like `Branch "master"
already exists.` rather than a paraphrase.

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

## Nightly updates

Packaged builds check the `tauri-nightly` channel five seconds after startup
and once every 24 hours when **Check automatically once a day** is enabled in
About. A check reads release metadata only; it never downloads or installs an
update automatically. Dismissing the main-window notice remembers that release
commit across restarts. About always provides a manual check and the current
update status.

Windows builds use the signed Tauri updater. The main-window notice opens About,
where users choose when to download and install. macOS builds do not install themselves:
Apple Silicon Homebrew cask installations show `brew upgrade --cask augur-git`
next to a button that copies it, and other installations can open the nightly
release page. Linux users use the AppImage, Debian package, or raw archive
attached to the release.

Homebrew requires explicit trust for casks from non-official taps. On Apple
Silicon, trust the project cask and install it with:

```bash
brew tap algosoft-lab/augur-git
brew trust --cask algosoft-lab/augur-git/augur-git
brew install --cask augur-git
```

When a newer nightly is published, upgrade it with:

```bash
brew upgrade --cask augur-git
```

Every `publish-tauri` run uses one version of the form
`0.1.1-nightly.<run-number>` across the Windows installer, macOS DMG, Linux
packages, About window, and `--version` output. CI first publishes immutable
assets under a versioned `tauri-nightly-<version>` release. It then updates the
checksum-pinned cask in `algosoft-lab/homebrew-augur-git` and advances the
rolling `tauri-nightly` release. The signed `latest.json` feed is published
after its installer target and the rolling download links have been validated.
After the rolling release succeeds, CI removes older Tauri nightly releases and
tags. It keeps the rolling release and the current versioned release, which the
updater feed and Homebrew cask reference.

### Release setup

The workflow needs these GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY` — the contents of the Tauri updater private-key
  file. The corresponding public key is embedded in `src-tauri/tauri.conf.json`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — optional; set this if the key was
  generated with a password.
- `TAP_PUSH_TOKEN` — a fine-grained token with Contents read/write access to
  `algosoft-lab/homebrew-augur-git`; its default branch must allow that token to
  push updates to `Casks/augur-git.rb`.

From `tauri-app/`, generate a key pair once and retain the private key
securely. Do not replace the key after releases have been published: installed
builds trust the public key committed in `tauri.conf.json`.

```bash
mkdir -p ~/.config/augur-git/tauri
bunx tauri signer generate --write-keys ~/.config/augur-git/tauri/updater.key
```

Create the public tap repository with a default branch and an initial commit
before enabling publication. The first successful publish creates
`Casks/augur-git.rb` with the versioned DMG URL and its SHA-256 checksum. Missing
signing or tap credentials stop the workflow before it changes the rolling
release. macOS DMGs use an ad-hoc signature and are not notarized, so Gatekeeper
may require approval after installation.

The separate GPUI workflow in `.github/workflows/build.yml` has no push
trigger. It remains available only through GitHub Actions **Run workflow** for
legacy builds.

## License

This application is licensed under the [Apache License 2.0](LICENSE). The
license file is included here so the application can be packaged independently.
Built-in theme sources and third-party notices are listed in
[`docs/theme-sources.md`](docs/theme-sources.md) and
[`public/THIRD_PARTY_THEME_NOTICES.txt`](public/THIRD_PARTY_THEME_NOTICES.txt).

## Terminal command

On macOS and Linux, `agit` opens or focuses Augur Git and returns control to the
terminal immediately. With no path argument, it opens the Git working tree
containing the current directory. Explicit paths may be a local Git working tree
or a directory inside one; subdirectories resolve to the working tree root.
Linked worktrees and submodules are supported. All paths are checked before
launching the GUI.

```bash
agit
agit .
agit ~/projects/repo-a ~/projects/repo-b
agit -- -repository-name
agit --help
agit --version
```

Running `agit` outside a Git working tree reports an error with exit code 2.
Exit code 0 means the launch was requested successfully, 2 means invalid
arguments or repository paths, and 1 means the launcher could not start the
application. A successful launch does not acknowledge completion of repository
loading. WSL repositories are opened through the interface. Starting the desktop
application directly still does not open its working directory.

Homebrew cask installations expose `agit` automatically. Debian packages install
it in `/usr/bin`. Those commands are upgraded and removed by their package
manager. For manual macOS app bundles, Linux AppImages, and extracted Linux
archives, open **Settings > General > Terminal command: agit** to install,
repair, or remove a user-owned command. The installer prefers a writable user
directory already in the application's PATH and otherwise uses `~/.local/bin`.
It never replaces a foreign command or requests administrator privileges. The
settings page reports both the application's PATH and a bounded probe of the
configured bash, zsh, or fish shell. Shell startup files are evaluated for this
read-only diagnostic; unsupported shells, startup errors, or timeouts are shown
as unknown. If the shell cannot find the command, Settings provides an optional
shell-specific PATH command. No shell configuration is changed automatically.

Install a macOS app in a permanent location before installing its command;
launching from a DMG or a Gatekeeper translocation is unsuitable. Manual app
bundles and extracted archives use a link to their bundled launcher. AppImage
installations copy the launcher into the user directory and record the original
AppImage path, avoiding the temporary mount. Moving the application requires
repairing the command from its new location. Removing the command leaves the
application and its settings intact.

The CLI ownership record is `~/.local/share/com.augur.git.tauri/cli-install.json`.
CLI builds are prepared by the Tauri
`beforeBuildCommand`; macOS and Linux platform configurations include the
launcher in their packages. Packaging verifies help, version, and usage-error
exit codes from the packaged launcher. Windows CLI installation is not yet
supported.
