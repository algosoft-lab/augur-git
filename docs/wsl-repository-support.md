# WSL Repository Support — Implementation Plan

Status: implemented (phases 1–3; the stretch phase was dropped)
Audience: maintainers of `augur-git`

Implementation notes:

- `RepoLocation::Wsl` exists on every platform so configs stay portable; the
  platform boundary lives in `LocationConfig::to_repo` (explicit
  `err-wsl-unsupported` rejection), the `cfg(windows)` arms of
  `GitRepo::command`/`command_in_location`, and the user-facing entry points
  (menu items, welcome button). The WSL open dialog compiles on every
  platform so its wiring is type-checked by all targets.
- `wsl -l -q` output is UTF-16LE when redirected; `decode_wsl_output` sniffs
  the BOM/NUL bytes. Unit fixtures run on every platform.
- Verified per environment: `cargo fmt --all`, `cargo test`, and
  `cargo check --all-targets` on Linux; the Windows cfg branches of
  `src/core/git/location.rs` are additionally type-checked with
  `rustc --target x86_64-pc-windows-msvc --emit=metadata`. A full
  Windows-target crate check is not available in the development environment
  (the `psm` dependency requires a cross C toolchain); runtime verification
  of the WSL flow (§5 integration list) requires a Windows machine with WSL.

## 1. Goal

Open and operate Git repositories that live inside a Windows Subsystem for
Linux (WSL) distribution, using the distro's own `git` executable via
`wsl.exe`. Local repositories keep their current behavior byte-for-byte.

### Non-goals

- SSH remote repositories (separate future decision).
- Cross-distro filesystem access via `\\wsl$` UNC as a *preferred* path; UNC
  input is only recognized and translated, never executed against.
- WSL-aware Agents, Lua extensions, or the embedded terminal (see
  "Known boundaries").
- `\\wsl$` interception of the native folder picker result (decided out of
  scope; paste translation in the WSL open dialog is the supported input).

## 2. Current state (what the change builds on)

| Concern | Location | Note |
| --- | --- | --- |
| Single git spawn entry point | `git_command()` in `src/core/git.rs` | 34 call sites, all inside `src/core/git.rs` and `src/core/git/*.rs`, all on worker threads |
| Repo open + validation | `spawn_open()` in `src/core/git.rs` | synchronous `is_dir()` + `.git` exists check on the UI thread; assumes local fs |
| Worker plumbing | `worker_loop` / `refresh_all` / `run_git` | thread + `std::sync::mpsc`, serial commands |
| Direct local fs access in worker | `working_tree.rs` untracked preview (`fs::read`), clean safety check (`fs::symlink_metadata`) | must become location-aware |
| Arg builders embedding `-C <path>` | `commit_diff.rs`, `branch_compare.rs`, `working_tree.rs` helpers | take `repo_path: &str`, so a Linux-style path value flows through unchanged |
| `current_dir()` instead of `-C` | `git_command_in_repo()` in `agent_operation.rs` | Windows-side cwd does not map to WSL cwd; must switch to `-C` |
| Tabs / recents config | `OpenTabConfig { path }`, `recent_repos: Vec<String>` in `src/core/config.rs` | flat strings, no location concept |
| Tab identity | `repo_key()` in `workspace/persistence.rs` (`normalized_path` + Windows lowercasing) | canonicalize is local-fs only |
| Open flow | `Workspace::open_repo_path` → `GitView::open_repo` → `spawn_open`; `GitUiEvent::RepoOpened(String)` | path string flows end to end |
| Terminal | `src/terminal/` spawns an arbitrary `Shell::new(program, args)` | agent-scoped; no repo terminal button exists today |

Policy checks: WSL support is Windows-only functionality behind an explicit
`cfg(windows)` boundary (§4); all git invocations remain structured argv —
`wsl.exe -d <distro> --cd <path> --exec git <argv...>` never involves a
shell on either side (§2).

## 3. Design

### 3.1 Core types (`src/core/git.rs`, re-exported)

```rust
/// Where a repository physically lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoLocation {
    Local,
    #[cfg(windows)]
    Wsl { distro: String },
}

/// Everything the worker needs to talk to one repository.
#[derive(Clone, Debug)]
pub struct GitRepo {
    location: RepoLocation,
    /// Canonical path as seen from inside the location
    /// (`C:\dev\repo` local, `/home/u/repo` WSL).
    path: String,
}

impl GitRepo {
    /// Local: `git`. WSL: `wsl.exe -d <distro> --cd <path> --exec git`.
    /// All variants set CREATE_NO_WINDOW on Windows.
    pub(crate) fn command(&self) -> Command;

    /// The `-C <path>` argument value every subcommand builder already
    /// embeds; for WSL the value is the Linux path and `--cd` already
    /// positions the process, so the pair is redundant but harmless and is
    /// kept to avoid touching every builder.
    pub(crate) fn path(&self) -> &str;
}
```

`spawn_open(repo: GitRepo, event_tx)` replaces the `repo_path: String`
parameter. `GitView` stores a `GitRepo` instead of `repo_path: String`.

### 3.2 WSL command template

```text
wsl.exe -d <distro> --cd <linux-path> --exec git [args...]
```

- `--exec` bypasses the login shell; argv is passed verbatim to the Linux
  process (requires WSL from Windows 10 21H1+ / WSL 0.64+; older builds
  re-joined and re-split arguments — covered by the test matrix).
- `--cd` makes distro-relative paths correct even if a builder omits `-C`.
- Git absence in the distro surfaces as wsl.exe stderr; map to a dedicated
  error key (see §6).
- `wsl.exe` output quirks: `wsl -l -q` prints UTF-16LE when stdout is
  redirected. Decode by BOM/NUL-byte sniff, fall back to UTF-8.

### 3.3 Open-time validation moves into the worker for WSL

Local keeps the synchronous `is_dir` + `.git` check (phase 1 must not change
local behavior). WSL validation runs as the worker's first act:

```text
wsl.exe -d <distro> --cd <path> --exec git rev-parse --is-inside-work-tree
```

This also accepts worktrees/submodules whose `.git` is a file — an upgrade
over the local check, which carries a TODO for exactly that. On failure the
worker sends a new event and exits:

```rust
pub enum GitEvent {
    ...
    /// The worker could not open the repository; it exits after sending.
    OpenFailed(GitError),
}
```

`GitView` handles `OpenFailed` by dropping the handle and showing the error
status, mirroring today's synchronous `spawn_open` failure path. Existing
`StatusError` keeps its "worker continues" meaning.

Error keys: reuse `err-path-not-exist` / `err-not-a-repo` where the wsl.exe
stderr matches; add `err-wsl-distro-not-found`, `err-wsl-git-missing`
(non-Windows open attempts use `err-wsl-unsupported`).

### 3.4 Replacing direct filesystem access

Two worker paths touch the local filesystem and must become location-aware:

1. **Untracked-file content preview** (`working_tree.rs`). The relative path
   is already validated (no absolute paths, no `..` components). For WSL:
   `wsl.exe ... --exec cat -- <relpath>`; keep the 10 MiB / UTF-8 gates.
   Paths containing control characters are skipped with a `debug!` log
   (preview is best-effort; the local implementation also silently declines
   oversized/non-UTF-8 content).
2. **Clean safety check** (`run_clean_command` refuses to clean a path that
   has become a directory). For WSL, replace `symlink_metadata` with
   `git ls-files --others --directory -z -- <path>` and treat a trailing `/`
   in the record as "directory → refuse". Git is guaranteed present (it is
   the tool under test); if the probe itself fails, refuse the operation
   with an explicit error rather than proceeding (fail-safe per policy §6).

Patch export (`branch_compare.rs` `write_atomic`) writes to a user-chosen
**local** destination and stays untouched.

### 3.5 `current_dir()` cleanup

`agent_operation.rs::git_command_in_repo` sets the Windows working directory
instead of passing `-C`. Under WSL the child is `wsl.exe`, whose cwd is a
Windows path and does not position the Linux process. Switch this helper to
the `-C` argument form used everywhere else. This is a no-op refactor for
local repositories.

### 3.6 Configuration and identity

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LocationConfig {
    #[default]
    Local,
    #[serde(rename = "wsl")]
    Wsl { distro: String },
}
```

- The enum is **not** `cfg`-gated: a config written on Windows must parse on
  every platform. On non-Windows builds, opening a WSL tab surfaces
  `err-wsl-unsupported`. This is the explicit platform boundary.
- `OpenTabConfig` gains `#[serde(default)] location: LocationConfig`;
  `recent_repos` becomes `Vec<RecentRepo { path, location }>`. Configs
  without the field migrate to `Local` — no versioning needed.
- `repo_key` extends to encode location:
  `local|C:\dev\repo` (Windows-lowercased, as today) and
  `wsl|ubuntu|/home/u/repo` (distro lowercased, path case-sensitive).
- `active_tab_path` remains a string but is matched against `repo_key`
  values, which are now unambiguous.

### 3.7 UI flow

- Welcome page and File menu gain **"Open WSL Repository…"** (rendered only
  on Windows). The dialog contains:
  - a distro dropdown populated from `wsl -l -q` (decoded per §3.2), with a
    refresh affordance and an empty state that links to `wsl --install`;
  - a Linux path input (absolute paths starting with `/`; `~` is rejected
    with a hint, since `--exec` performs no shell expansion);
  - an inline validation state driven by the same `rev-parse` probe,
    debounced;
  - a paste-tolerant parser: values starting with `\\wsl$\<distro>\` or
    `\\wsl.localhost\<distro>\` are split into distro + Linux path by direct
    component mapping (no `wslpath` subprocess needed for this direction).
- Sidebar / tab label: `<distro> · <dir-name>`; `dir_name` already splits on
  `/`.
- `GitUiEvent::RepoOpened(String)` becomes `RepoOpened(GitRepo)` so the tab
  layer can label and persist without re-deriving the location.

## 4. Delivery plan

| Phase | Content | Exit criteria |
| --- | --- | --- |
| 1. Core refactor | `RepoLocation`/`GitRepo`, `spawn_open` signature, `-C` unification in `agent_operation.rs`, `LocationConfig` + `repo_key` | zero behavior change; `cargo test` green; all 34 `git_command()` sites route through `GitRepo::command()` or take the repo value |
| 2. WSL execution | command template, worker-side validation + `OpenFailed`, `cat`/`ls-files` replacements, distro listing + decoding, error keys, Windows integration tests | full open → status → stage → commit → diff → discard cycle works against a temp repo in a real distro |
| 3. UI | welcome/menu entry, open dialog, labels, recents with location, en-US/zh-CN strings | a non-technical user can open a WSL repo end to end; config round-trips across restart and across platforms |

Each phase lands as one reviewable PR. While touching files with legacy
Chinese comments, translate the affected comment blocks to English in the
same change (policy §1).

## 5. Testing

Unit (all platforms):

- `LocationConfig` serde round-trips: modern config, legacy config without
  `location`, WSL entry parsed on non-Windows builds.
- `repo_key` stability and cross-location uniqueness.
- UNC-input parser: `\\wsl$\Ubuntu\home\u\repo`, `\\wsl.localhost\...`,
  malformed input, trailing slashes, case of distro segment.
- `wsl -l -q` decoder fixtures: UTF-16LE with BOM, UTF-16LE without BOM
  (NUL sniff), plain UTF-8, empty output.
- WSL `GitRepo::command()` argv shape (windows-only test).
- Clean-check decision table: `ls-files` record with trailing slash →
  refuse; plain record → proceed; probe failure → refuse.

Integration (windows-only, `#[ignore]` when no distro is available):

- Create a repo under `/tmp` inside the default distro via `wsl.exe`, then:
  open, refresh, stage, commit, amend, diff (staged/unstaged/commit),
  branch compare, discard untracked (file case), patch export to a Windows
  destination.
- Non-ASCII path (`中文/目录`) survives status, staging, and diff.
- Old-vs-new wsl.exe argv behavior: arguments containing spaces and quotes
  are received verbatim by a probe script (`printf '%s\n' "$@"` via
  `--exec`) — if any current WSL build mangles them, record it in this
  document and gate on version.
- Distro names with spaces (e.g. `Ubuntu-22.04 LTS`, custom imports) list
  and open correctly.

Debugging handoff command (per the logging and debugging policy):

```bash
cargo run
rg "\[(git_view|git_command|workspace)\]" \
  debug-logs/debug-app.log debug-logs/debug-git.log > debug-logs/wsl-debug.log
```

## 6. Error surface (i18n keys)

| Key | Trigger |
| --- | --- |
| `err-wsl-unsupported` | WSL tab opened on a non-Windows platform |
| `err-wsl-distro-not-found` | `wsl.exe` reports the distro does not exist |
| `err-wsl-git-missing` | distro has no `git` on PATH |
| existing `err-path-not-exist` / `err-not-a-repo` | rev-parse probe failed for path reasons |

The dialog's empty distro list uses the dialog key `wsl-no-distros` (not an
error payload). All keys have en-US and zh-CN strings in `i18n/` with
`{ $detail }` carrying the raw wsl.exe stderr.

## 7. Known boundaries (documented, not solved)

- **Agents and Lua extensions run Windows-side.** A WSL repository passed to
  an agent terminal means a Windows process with a Linux path — broken cwd.
  Workaround for users: a custom agent profile whose executable is
  `wsl.exe`. Proper support needs location-aware `AgentLaunchSpec`, deferred.
  Extension host calls `automation::capture(Path)` directly; it will receive
  the Linux path and fail cleanly until made location-aware. Phase 2 adds an
  explicit "not available for WSL repositories" response rather than a
  confusing failure.
- **Folder picker cannot browse into `\\wsl$` reliably** across Windows
  versions, and picker results are not intercepted; UNC paste translation in
  the WSL open dialog is the supported input path.
- **`wsl.exe` argv fidelity on very old builds**: the integration test in §5
  records the minimum working version; UI surfaces a clear error when the
  probe fails.
- **Windows Git vs distro Git**: opening a repo by UNC path through the
  local picker executes Windows git against `\\wsl$`, which "works" but is
  slow and semantically wrong (case sensitivity, symlink handling). The
  UNC translator steers users to the WSL path instead.

## 8. Decisions

1. WSL repositories are recorded in the existing `recent_repos` list (now
   carrying a location), not a separate recents collection.
2. The stretch phase (Phase 4: `\\wsl$` drop and folder-picker translation)
   is dropped. UNC recognition exists only as paste parsing inside the WSL
   open dialog.
