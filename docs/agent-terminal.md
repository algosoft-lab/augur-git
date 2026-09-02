# External Agent terminal

Augur Git does not ship an AI model, provider SDK, tool loop, account UI,
approval policy, or sandbox. It only verifies that a user-installed coding-agent
CLI can be started from the desktop application's environment. The CLI remains
responsible for its own login, model selection, tools, permissions, and
provider-specific recovery behavior.

## Supported profiles

The first-party profiles are:

| Profile | Executable | Initial prompt position |
| --- | --- | --- |
| Codex | `codex` | trailing argument |
| Claude Code | `claude` | trailing argument |
| OpenCode | `opencode` | `--prompt <prompt>` |

The executable can be overridden in Agent settings. A custom profile consists
of an executable path, a fixed argument array, and either a trailing prompt or
a prompt flag. Arguments are passed directly to the process; Augur Git never
builds a shell command string, expands variables, or accepts a custom working
directory.

The settings page probes each resolved executable with `--version` in a
background task. OpenCode also receives `--help` so the page can detect whether
the installed root TUI advertises `--variant`. On Windows, executable lookup
also checks the `.exe`, `.cmd`,
and `.bat` suffixes used by interactive shells, so npm-installed CLI shims can
be launched from the PTY. A missing CLI is reported as unavailable and does not
prevent other profiles from being used. The Agents section lets users add,
edit, and remove custom profiles; fixed arguments are entered one per line and
the prompt can be a trailing argument or a named flag. Invalid custom profiles
are shown with an error and are not launchable. Each valid profile also has a
**Test launch** action. It opens a separate, visible interactive terminal and
runs a fixed connectivity challenge in a new empty per-user directory. On
Linux this is below `$XDG_DATA_HOME/augur-git/agent-tests` (normally
`~/.local/share/augur-git/agent-tests`); on macOS it is below
`~/Library/Application Support/augur-git/agent-tests`. Other platforms use the
system temporary directory. Keeping the directory under the user's home tree
allows Agent CLIs that restrict access to system temporary locations to start
normally. The test never uses the current repository, does not initialize Git,
and does not offer an editable prompt. The terminal remains available for
provider login, approvals, and follow-up input. Augur Git marks the test as
having received a response only after the challenge's per-run reversed token
appears in the bounded terminal buffer.

Built-in profiles also support optional launch-time model overrides. Leave the
model field empty to inherit the Agent CLI's normal environment and
configuration. Codex receives `--model` and, when selected, a
`--config model_reasoning_effort="..."` override; Claude Code receives
`--model` and `--effort`. OpenCode's interactive TUI receives
`--model provider/model`; Augur adds `--variant <name>` only when that
installed TUI explicitly advertises the flag. Variant
names are model-specific rather than a universal low/high scale. Use
`opencode models` or the TUI `/models` command to find the model, then use
the Variant names defined by that model's OpenCode configuration. Custom
profiles continue to use their fixed arguments and do not receive typed model
or reasoning options.
Explicit values apply only to new sessions, and unsupported values are shown
as a visible launch error rather than silently falling back.

## Choosing an OpenCode Variant

OpenCode variants are selected per model, so Augur Git keeps the value as free
text instead of presenting a misleading universal list. To configure one:

1. Run `opencode models` in a shell, or use the `/models` command inside
   OpenCode, and copy the exact `provider/model` identifier into the **Model**
   field in Settings → Agents.
2. Check that model's available variants in the OpenCode model configuration or
   model documentation. Enter the exact variant name in the **Variant** field.
3. Leave **Variant** empty to inherit the model's OpenCode configuration. A
   saved value is passed to new interactive sessions as `--variant <name>`;
   existing sessions are not changed.

Augur Git does not validate the variant against a remote model catalog. This
keeps startup offline and works with local providers and gateways. If the
installed OpenCode version advertises the root interactive `--variant` flag,
Augur Git passes it directly. Current OpenCode releases expose the flag for
`opencode run --variant`, but not for the root interactive TUI. Augur detects
this from `opencode --help`; the visible test window explains that the
interactive override is unavailable instead of starting a command with an
invalid flag. If the selected model still rejects
the value, the visible Agent terminal shows the CLI's error and the launch is
not silently retried with different arguments.

The three CLIs retain responsibility for their own model catalogs, account
configuration, permissions, and agent mode. Augur Git does not inject generic
`auto`, `build`, approval, or sandbox flags. See the providers' current
references for details: [Codex CLI](https://learn.chatgpt.com/docs/developer-commands?surface=cli),
[Codex configuration](https://learn.chatgpt.com/docs/config-file/config-reference),
[Claude Code CLI](https://code.claude.com/docs/en/cli-usage), and
[OpenCode CLI](https://dev.opencode.ai/docs/cli/) and
[OpenCode models and variants](https://dev.opencode.ai/docs/models/).

The temporary directory is shown in the test window and is removed after the
child exits. The per-user parent directory is retained for later tests, while
each test directory itself starts empty and is private to the current user.
Closing the window or pressing **Stop test** sends Ctrl-C and then closes the
PTY; cleanup failures are non-fatal and never target a user-selected
directory. Test prompts, transcript contents, and provider session IDs are not
persisted.

The test window keeps the profile, status, and stop control in a compact header.
Executable, arguments, working directory, and diagnostic prompt remain visible
inside a bounded scrollable details area so wrapped metadata cannot consume the
entire terminal viewport at small window sizes. The terminal is given a minimum
usable height and still receives its actual measured rows and columns.

The profile editor stores the same structured values in `config.json`. For
example, a profile that runs a local wrapper with two fixed arguments uses:

```json
{
  "id": "my-reviewer",
  "name": "My reviewer",
  "executable": "/opt/tools/reviewer",
  "args": ["--interactive", "--format", "terminal"],
  "prompt_mode": "trailing-argument"
}
```

For a flag-based CLI, set `prompt_mode` to `{ "flag": "--prompt" }`. The
executable and every argument remain separate process values; no shell syntax
is interpreted.

## Test lifecycle

The test window remains interactive after the challenge response, allowing
provider login, approvals, and follow-up input. Closing the window or pressing
**Stop test** sends Ctrl-C and then closes the PTY. The temporary directory is
removed after the child exits; cleanup failures are non-fatal and never target a
user-selected directory. Test prompts, transcript contents, and provider
session IDs are not persisted. Closing a repository tab does not affect an
independent connectivity-test window; closing the application asks before
stopping active tests.

## Commit by AI

The Commit menu's **Commit by AI** action delegates one fixed commit operation
to the current Agent profile selected at the top of Settings → Agents. It
opens the same visible PTY in the repository root; no temporary directory or
`AUGUR_GIT_TASK_FILE` is used. The optional text in the commit editor is only a
commit-message hint and is validated before it is included in the fixed
operation prompt.

The operation instructs the Agent to inspect staged, unstaged, and untracked
changes, stop when conflicts or no changes are present, run `git add --all`,
review the staged diff, create one concise Conventional Commit, and report the
result. A per-session completion marker is requested after the operation; the
Agent remains in its interactive TUI and Augur Git stops it after the marker is
observed. It must not edit file contents, amend, merge, rebase, reset,
checkout, or push. Augur Git disables other repository operations while this
session runs. When the Git probe observes a changed HEAD, the repository status
and history refresh immediately, while controls stay locked until completion.
The process exit code is shown separately from the refreshed Git state; an exit
code alone is not treated as proof that a commit was created.

On a verified commit, Augur Git stops the interactive process and closes the
Agent window automatically. If the operation reports no changes, encounters a
conflict, fails, is cancelled, or exits without verification, Augur Git also
stops the process and refreshes the repository but keeps the terminal open for
diagnostics; the button becomes **Close**. If a changed HEAD is observed but
the marker is missing, a bounded fallback accepts the commit only after 30
seconds and at least 3 seconds without PTY activity.

Only one active Commit by AI session is allowed per repository. Starting it
again focuses the existing window. Closing the window, repository tab, or
application stops the session after the normal Ctrl-C grace period. The
operation prompt, terminal transcript, and provider session identifiers are
never persisted.

## Merge by AI and conflict recovery

The local-branch context menu contains **Merge by AI** for a branch other than
the current branch. Augur Git resolves that branch to an immutable commit ID
before launching the selected Agent, then opens the visible terminal in the
current repository. The preflight requires an entirely clean working tree (no
staged, unstaged, untracked, or unresolved changes) and no other stateful Git
operation, so a merge cannot accidentally overwrite work that appeared after
the menu action. The Agent is
asked to run a normal fast-forward-allowed merge, resolve only files that Git
marks as conflicted, and create the merge commit. It may not push, reset,
checkout, amend, rebase, abort, or edit unrelated files.

The result is verified from Git state rather than from Agent prose. Augur Git
records the starting `HEAD`, watches for a new `HEAD`, checks that the target
is an ancestor, and confirms that `MERGE_HEAD` and unmerged entries are gone.
An already-up-to-date target is reported separately. A verified merge refreshes
the repository immediately, stops the interactive process, and closes the
Agent window. Conflicts, failed or cancelled sessions, and exits without a
verified result stop the process but keep the terminal visible for diagnosis.

If an ordinary **Merge** or **Merge (--no-ff)** leaves `MERGE_HEAD`, the UI
shows the complete Git error and offers **Abort merge** or **Resolve conflicts
by AI**. The latter starts the current Agent in the existing merge state; it
does not run a second merge and commits with Git's prepared message after all
unmerged entries are resolved. Augur rechecks the saved `HEAD` and
`MERGE_HEAD` before starting that handoff, so changes made while the dialog
was open are reported instead of being handed to the Agent. Closing the dialog
leaves the merge untouched, so it can be resolved in an external editor. While
unmerged entries remain (and while `MERGE_HEAD` is still present after files
are staged), checkout, ordinary merge, rebase, pull, and stash-pop actions are
disabled;
**Merge by AI** remains available so the same source can enter conflict-
resolution mode. Viewing files, staging resolutions, and ordinary commits
remain available. If the merge has already been resolved externally, the next
status refresh restores the normal actions.

Only one visible Agent Git operation (Commit by AI, Merge by AI, Rebase by AI,
or conflict resolution) may run for a repository at a time. Settings changes affect new
sessions only. No Git operation, terminal transcript, or provider session ID
is persisted.

## Rebase by AI and pull --rebase recovery

The local-branch context menu also contains **Rebase current branch by AI**.
It rebases the current branch onto the selected local branch's immutable commit
ID. The preflight requires a clean working tree and no other Git operation. The
Agent runs the normal `git rebase` flow in the visible repository terminal,
resolving only files that Git marks as conflicted and continuing until the
rebase completes. It may not push, checkout, reset, abort, amend, merge, or
edit unrelated files. Augur verifies the resulting `HEAD`, clean index, and
absence of rebase state before reporting success; already-up-to-date results
are reported separately.

The toolbar's ordinary **Pull (Rebase)** remains a Git-owned
`git pull --rebase` operation. If ordinary **Rebase** or **Pull (Rebase)**
fails while Git leaves a rebase in progress, Augur shows the complete command
output and offers **Abort rebase** or **Resolve conflicts by AI**. The latter
attaches the selected Agent to the existing rebase; it never starts a second
rebase or aborts the current one. Closing the dialog leaves the rebase state
untouched for manual resolution in an external editor. The handoff rechecks
`REBASE_HEAD`/rebase state and the saved `HEAD` baseline before launching.

Successful Agent rebases refresh the repository immediately, stop the visible
PTY, and close its window. Conflicts, failed or cancelled sessions, and exits
without a verified result stop the process, release the repository busy state,
refresh Git, and keep the terminal visible for diagnosis. Only one Agent Git
operation (commit, merge, rebase, or conflict resolution) can run for a
repository at a time.

## Terminal boundary

The embedded view uses the Apache-2.0 `alacritty_terminal` state machine and
PTY implementation. It parses ANSI output, alternate-screen applications,
truecolor, Unicode/wide characters, cursor movement, and a bounded 10,000-line
scrollback buffer. Input is encoded as terminal bytes for common control,
navigation, function, and text keys. OSC 52 clipboard access, notifications,
downloads, hyperlinks, and embedded-image side effects are deliberately not
forwarded to the host application.

Rendering is driven by a coordinate-preserving snapshot of the parsed grid.
The view measures the actual terminal element bounds and active monospace font,
then synchronizes both the local Alacritty grid and the PTY whenever the cell
grid changes. The local grid is resized before the PTY notification, so an
Agent redraw and the parser observe the same rows and columns. Each canvas
frame captures its snapshot after that synchronization and rejects a frame
whose grid dimensions do not match its geometry.

Glyphs are shaped with the measured per-cell width and painted at explicit row
and column origins; backgrounds and selection rectangles are painted from the
same grid before glyphs. This keeps wide characters, combining marks, ANSI
backgrounds, alternate-screen layouts, and mouse coordinates aligned when the
window is resized or displayed at high DPI. If a styled paint operation fails,
the view keeps the coordinate grid visible through a plain-text fallback.

The view is not a general shell. It should be treated as a host for the three
supported coding-agent CLIs and compatible custom profiles during connectivity
testing. Terminal behavior that depends on a vendor's private TUI protocol is
intentionally left to the CLI itself.

## Coordination boundaries

The Git status probe and porcelain-v2 parser live in
`src/core/git/agent_operation.rs`, keeping repository inspection on the Git
worker boundary. Commit outcomes and probe classification are pure logic in
`src/workspace/agent_commit.rs`; merge outcomes are in
`src/workspace/agent_merge.rs`. The visible session coordinator owns the
shared PTY window, monitor, marker handling, and lifecycle callbacks; it does
not duplicate Git parsing or provider-specific command construction.

## Troubleshooting

1. Open Settings → Agents and check the executable probe result.
2. If the probe is unavailable, install the CLI and ensure the GUI process can
   see the same `PATH` as the shell where it works, or set an absolute path
   override.
3. Click **Test launch** for the profile. The separate terminal shows the
   complete startup, login, approval, and response flow. Complete any provider
   login prompt there; a process that starts but exits before the reversed
   challenge token is shown as an incomplete test.
4. For lifecycle diagnostics, run the application in a debug build and filter
   the feature log:

   ```bash
   cargo run
   rg "\[agent_terminal\]" debug.log > agent-terminal-debug.log
   ```

Task text, terminal input/output, credentials, and complete repository paths
are not written to the log.
