# External Agent session terminal

Augur Git follows an **Agent coding, Augur review** workflow. It does not ship
an AI model, provider SDK, tool loop, account UI, approval policy, or sandbox.
Instead, a repository tab can launch a user-installed coding-agent CLI in a
dedicated interactive terminal. The CLI remains responsible for its own login,
model selection, tools, permissions, and provider-specific recovery behavior.

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
background task. A missing CLI is reported as unavailable and does not prevent
other profiles from being used. The Agents section lets users add, edit, and
remove custom profiles; fixed arguments are entered one per line and the prompt
can be a trailing argument or a named flag. Invalid custom profiles are shown
with an error and are not launchable. Each valid profile also has a **Test
launch** action. It opens a separate, visible interactive terminal and runs a
fixed connectivity challenge in a new empty system temporary directory. The
test never uses the current repository, does not initialize Git, does not write
`AUGUR_GIT_TASK_FILE`, and does not offer an editable prompt. The terminal
remains available for provider login, approvals, and follow-up input. Augur Git
marks the test as having received a response only after the challenge's
per-run reversed token appears in the bounded terminal buffer.

The temporary directory is shown in the test window and is removed after the
child exits. Closing the window or pressing **Stop test** sends Ctrl-C and then
closes the PTY; cleanup failures are non-fatal and never target a user-selected
directory. Test prompts, transcript contents, and provider session IDs are not
persisted.

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

## Starting a session

Use **New Agent Task** in a repository toolbar or an Agent subtab. The composer
captures the current branch and the selected review reference (working-tree
file, commit, commit file, or revision comparison). It records references only;
it does not copy a complete diff into the task.

The task body is written to a private, application-owned temporary directory.
The child receives the file path through `AUGUR_GIT_TASK_FILE` and a fixed
bootstrap prompt telling it to read that file. Keeping the user text in the
file avoids shell quoting, process-list exposure, and command-line length
limits. The task file is removed when the session exits or is dropped. Startup
also removes only old files matching Augur Git's generated filename format.

## Session and repository lifecycle

Each repository can contain multiple Agent subtabs. A session keeps reading
PTY output while hidden and does not infer provider-specific states such as
“thinking” or “waiting for approval”; the UI reports process/terminal states
only. Returning to Review after an Agent exits schedules the normal repository
refresh. If Git is busy, the existing refresh logic retries when it is ready.

All sessions in a repository share its current working tree. Starting another
session while one is active requires an explicit confirmation. Closing an
active session, repository tab, or the application also requires confirmation;
confirmation sends an interrupt/shutdown request and lets the PTY backend reap
the child and descendants that remain attached to the PTY. A provider that
deliberately daemonizes a detached process is outside the v1 lifecycle. No
daemon, transcript store, vendor session ID, worktree isolation,
or cross-restart reconnect is provided in v1.

## Terminal boundary

The embedded view uses the Apache-2.0 `alacritty_terminal` state machine and
PTY implementation. It parses ANSI output, alternate-screen applications,
truecolor, Unicode/wide characters, cursor movement, and a bounded 10,000-line
scrollback buffer. Input is encoded as terminal bytes for common control,
navigation, function, and text keys. OSC 52 clipboard access, notifications,
downloads, hyperlinks, and embedded-image side effects are deliberately not
forwarded to the host application.

The view is not a general shell. It should be treated as a host for the three
supported coding-agent CLIs and compatible custom profiles. Terminal behavior
that depends on a vendor's private TUI protocol is intentionally left to the
CLI itself.

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
