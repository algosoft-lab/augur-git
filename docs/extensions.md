# Lua extensions

Augur Git loads trusted Lua 5.4 extensions through `mlua` with the vendored
Lua runtime. Extension code runs on a dedicated worker thread with a long-lived
VM; all invocations are serialized by one global FIFO queue and a second
trigger for the same extension is coalesced while it is queued or running.

Enabling an extension is a trust decision. The VM intentionally has the full
Lua standard library (`os`, `io`, `package`, and `debug`) and package loading
can expose native modules. The runtime applies a 64 MiB Lua memory limit and
checks cancellation at Lua instruction and host-operation boundaries. Native
code and `os.execute` cannot be made reliably interruptible, so only packages
the user has reviewed should be trusted.

## Package layout

An extension directory contains:

```text
my-extension/
├── manifest.toml
├── main.lua
└── optional-modules.lua
```

The manifest requires an id, semantic version, `api_version = 1`, display
metadata, and an entrypoint. It may declare `string`, `integer`, `boolean`,
`time`, and `select` settings, one manual handler, and event handlers. The
legacy `[[daily]]` declaration is accepted and normalized to a
`schedule.daily` event. The bundled `extensions/sync-open-tabs` package is a
complete reference package.

Events are independent subscriptions. A package can expose a manual handler
and any combination of `schedule.daily`, `schedule.interval`,
`workspace.repository_opened`, `workspace.repository_closed`,
`repository.branch_changed`, and `repository.status_changed` handlers:

```toml
[[events]]
id = "daily-sync"
type = "schedule.daily"
label = "Daily sync"
handler = "on_schedule"
time_setting = "sync_time"
```

`Run once` invokes the manifest's manual handler and does not change event
subscriptions. Event subscriptions remain active after the management window
is closed; they are only evaluated while the application process is running.

Local packages are installed under the platform data directory:

* Windows: `%LOCALAPPDATA%\\augur-git\\extensions\\<id>`
* macOS: `~/Library/Application Support/augur-git/extensions/<id>`
* Linux: `$XDG_DATA_HOME/augur-git/extensions/<id>`, defaulting to
  `~/.local/share/augur-git/extensions/<id>`

Installation copies into a staging directory, rejects symlinks and invalid
entrypoints, then promotes the staging directory atomically. A package is
fingerprinted for display and audit history. Reloading re-discovers packages,
recreates idle workers, and keeps compatible settings, trust, and per-event
subscriptions. Reload is refused while an extension run is active. Open
`Extensions` from the application menu to show a singleton, resizable native
window. It provides a manual `Run once` action, per-event subscription
switches, generated setting controls, trust warnings, queue/cancellation
status, and recent history. Closing this window does not stop the runtime or
event subscriptions. There is no online marketplace in v1.

Per-extension settings are stored in application configuration. Private Lua
storage is a JSON file below the platform data directory's
`augur-git/extension-data` directory. The last 50 run records are kept there;
records contain repository display names and step summaries, never Agent
transcripts or complete repository paths.

## Lua API

An entrypoint returns a table containing `on_run(ctx)` and/or
`on_schedule(ctx)`:

```lua
local augur = require("augur")

return {
  on_run = function(ctx)
    local now = augur.time.now()
    for _, repo in ipairs(ctx.repositories) do
      local state = repo:status()
      augur.log("info", "repository checked", {name = repo:display_name()})
    end
    return {ok = true, summary = "checked"}
  end,
}
```

The context contains `run_id`, trigger metadata, ISO-8601 schedule/start
timestamps, read-only setting values, captured repository handles, and a
`cancelled()` predicate. Repository handles provide `snapshot`, `status`,
`wait_until_ready`, structured `git(args, options)`, `pull_rebase`, `push`, `agent_commit`,
`agent_merge`/`merge`, `agent_rebase`/`rebase`, and merge/rebase recovery
operations. Git arguments are passed as separate `Command` arguments; no shell
command string is constructed.

`augur.system.info`, `augur.time.now`, `augur.log`, `augur.notify`,
`augur.storage.get/set/delete`, and `augur.workspace.repository_tabs()` are
also available. Git and Agent failures return `{ok = false, code, summary}`;
invalid API use, a missing handler, cancellation, or a disconnected host is a
Lua error. `augur.agent.prompt(repo, options)` returns completion state, exit
code, and at most 1 MiB of in-memory transcript. Transcripts are not logged or
written to run history.

## Scheduling and synchronization sample

Daily triggers use the local timezone while the application process is alive.
The scheduler does not backfill a task missed before startup, de-duplicates a
repeated daylight-saving-time occurrence by local date, and fires once after a
sleep interval that crosses the configured time. A run captures the open tabs
and then processes them sequentially. The bundled sync sample:

1. rejects closed tabs and branch/HEAD changes;
2. resolves an existing merge or rebase with the configured Agent and rejects
   unsupported cherry-pick, revert, bisect, or sequencer states;
3. asks the Agent to commit a dirty worktree and verifies a changed HEAD and a
   clean state;
4. runs `git pull --rebase`, recovers a rebase conflict with the Agent, and
   verifies that no operation or conflict remains; and
5. pushes without force. Existing upstreams use `git push`; otherwise the
   configured remote and captured branch are passed to
   `git push --set-upstream` and the resulting upstream/ahead state is checked.

One repository failure is recorded and processing continues with the next
repository. Host events refresh inactive repository tabs and the page displays
the latest queue, run, notification, and per-repository history status.

For focused development logs, run the application and then filter the debug
log:

```text
cargo run
rg "\\[(extensions|extension_events|extension_runtime|extension_sync|agent_operation|git_command)\\]" debug.log > extension-debug.log
```
