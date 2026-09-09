# Diagnostic logging

Augur Git uses file-only diagnostic logging. The logger keeps the existing
`RUST_LOG` filtering behavior while routing records by functional ownership.
The application does not write normal diagnostic records to stdout or stderr.

## Files

Debug builds write the following files under `debug-logs/` relative to the
working directory. The directory is created automatically when logging starts:

| File | Contents |
| --- | --- |
| `debug-logs/debug.log` | Application-owned warnings, errors, panics, and the session marker |
| `debug-logs/debug-app.log` | Workspace, settings, configuration, theme, and UI lifecycle |
| `debug-logs/debug-git.log` | Git worker, repository panels, commits, branches, and comparisons |
| `debug-logs/debug-agent.log` | Agent settings, probes, connectivity, and agent operations |
| `debug-logs/debug-extension.log` | Extension loading, management, runtime, and extension diagnostics |
| `debug-logs/debug-terminal.log` | Terminal UI and terminal interaction |
| `debug-logs/debug-system.log` | GPUI, platform backends, and other external-library records |

Release builds use the same suffixes under the application data log directory,
with `augur-git.log` as the summary file and `augur-git-*.log` as category
files.

The extension API `augur.log_file(...)` is separate. It continues to append to
the absolute file path selected by the extension and is not redirected into the
diagnostic category files.

## Retention and filtering

At startup, an existing current file is moved to its matching
`.previous.log` file. Each current file is limited to 2 MiB; if the limit is
reached during a session, the current file is rotated again and the previous
copy is replaced. Rotation failures do not prevent application startup.

Debug builds default to `warn` and release builds default to `info`, matching
the previous logger. Set `RUST_LOG` to enable detailed records for a targeted
module. For example, set
`RUST_LOG=warn,augur_git::workspace=debug,augur_git::workspace::settings=debug` before running the
application, then inspect `debug-logs/debug-agent.log` or
`debug-logs/debug-app.log` as appropriate.

Generated log files are ignored by Git. For a focused Agent settings
investigation, set
`RUST_LOG=warn,augur_git::workspace=debug,augur_git::workspace::settings=debug`
before running the application, then run:

```bash
cargo run
rg "\[(agent_settings|panic)\]" \
  debug-logs/debug-agent.log debug-logs/debug-app.log > debug-logs/agent-settings-debug.log
```
