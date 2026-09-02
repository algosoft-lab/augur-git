# External Agent connectivity terminal

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
