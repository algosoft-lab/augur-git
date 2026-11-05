# HTTP/HTTPS credentials

Augur Git answers Git's HTTPS credential prompts from storage instead of asking
every time. This document describes what is stored, where the secret lives, how
a prompt is matched to an entry, and which limits are deliberate.

## What a prompt does

Git reaches the application through its askpass bridge: every spawned Git
command inherits `GIT_ASKPASS`, and Git's per-operation prompt is forwarded over
a loopback socket. Git executes that program itself and appends the prompt as
its only argument, so the value holds a path and nothing else; the application
recognises the helper call from the bridge variables every Git child inherits.
The same dispatch runs in `augur-server`, otherwise a prompt would start a
second server instead of an answer.

Three outcomes are possible, in this order:

1. **Answered from storage.** When `auto-fill` is on and an entry matches the
   prompt, the value is returned without a dialog. The username and password
   prompts of one Git operation are separate helper processes that share an
   operation id, so a password is only ever taken from the entry whose username
   was just used.
2. **Answered by the form.** Otherwise the interface shows one dialog naming the
   host, the repository, and where the secret may be kept. One form replaces what
   used to be two prompts (Git asks for a username, then a password).
3. **Answered by the raw prompt.** A prompt this module cannot interpret — an
   SSH passphrase, anything without a quoted URL — keeps the original
   single-field, remember-nothing dialog.

The remote and clone dialogs call `credential_lookup` on the address being
edited, so an address that already has an account says so before the first
fetch instead of surprising the user with a prompt.

Cloning asks for the same thing in the same place. The clone wizard is reachable
from **More** in the repository toolbar as well as from the welcome page, and it
collects the remote URL first and the destination path second, so the only
questions on screen are the two a person has an answer for. When Git needs an
account the form appears in that card rather than over it: the wizard answers
the prompt itself, and the standalone credential dialog stays out of the way
while the wizard is open. Closing the wizard hands an unanswered prompt back to
the standalone dialog.

A refused password is detected, not guessed: if the same Git operation asks for
a password a second time, the entry that answered the first time is marked
`lastFailureAt` and is left out of automatic answers for two minutes. Without
that rule a stale password would be retried in a loop and the real prompt would
never appear.

## Where a secret lives

| Storage | Backend | Written without asking |
| --- | --- | --- |
| `keychain` | macOS Keychain, Windows Credential Manager, Linux Secret Service (`keyring` crate) | yes, it is the default |
| `session` | process memory | yes, but nothing reaches the disk |
| `plaintext` | `credential-secrets.json` beside `settings.json`, mode `0600` on Unix | no; the settings page has to enable it |

Only the metadata lives in the settings document, under the `credentials` key.
A password never enters that document, a log line, an event payload, or a
response to the interface: editing an entry shows an empty password field, and
leaving it empty keeps the stored secret.

When no keychain is reachable — a headless server, a container without a session
bus, a sandbox that blocks Secret Service — the backend reports its reason
instead of silently demoting the secret. The plaintext file then becomes the only
persistent option, and every surface that shows such an entry labels it.

## Matching

An entry stores `protocol`, `host` (with a non-default port), an optional
`pathPrefix`, a `username`, and a `scope` (`global` or one repository).

A prompt matches when:

- the protocol is equal (`http` never answers `https`);
- the host is equal after lowercasing and dropping `:443`/`:80`;
- the username in the URL, when Git names one, is equal;
- the scope is the same repository, or the entry is global;
- the path prefix is a segment boundary of the prompt's path.

The most specific entry wins: repository scope above a path prefix above a
global entry, then the most recently updated. Two accounts on one host are
therefore possible with a path prefix or a repository scope.

## Commands

Shared by both frontends (`augur-service`, then the Tauri wrapper and the server
dispatcher):

| Command | Purpose |
| --- | --- |
| `credential_snapshot` | Backends, settings, and the entry table |
| `update_credential_settings` | `autoFill`, `defaultStorage`, `allowPlaintext`, `useHelper` |
| `save_credential` | Create or update one entry; an absent password keeps the stored secret |
| `delete_credential` | Remove an entry and its secret |
| `credential_login_response` | Answer a `login` prompt event |
| `import_git_credentials` | Read `~/.git-credentials`, never modify it |
| `credential_lookup` | Whether a remote URL already has an account, without reading its secret |
| `test_credential` | Run `git ls-remote` against an entry's path, capped at 30 s |

The prompt event keeps its existing name (`augur://lfs-credential-request`) and
gains fields, so an older payload shape still parses.

## The terminal helper

`useHelper` registers one extra `credential.helper` for the Git commands Augur
Git spawns, through `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n`.
Git reads those entries after the user's files, so an existing keychain helper
keeps priority and this helper only runs for hosts the user saved here. Nothing
is written into the user's Git configuration.

The helper is **read-only**: `get` answers, `store` and `erase` do nothing. Writes
stay with the running application so a `git push` in a terminal cannot rewrite a
document the application holds in memory. The consequence is that credentials
entered in a terminal are not captured, and a credential a terminal's Git
rejects is not removed here.

## Limits

- Local repositories only. A WSL repository keeps using the credentials
  configured inside its distribution, as before.
- SSH keys, HTTP proxy authentication, client certificates, and OAuth device
  flows are out of scope.
- The server frontend shares one vault per process: every browser holding the
  access token sees and uses the same entries. Its plaintext backend is the
  server's own file, so enabling it there protects nothing beyond that host's
  filesystem permissions.
- A stale `credential.helper=store` in the user's own configuration answers
  before this vault does, because Git asks helpers in order. The settings page
  explains the priority rather than fighting it.

## Tests

- `crates/augur-service/tests/askpass_credentials.rs` plays Git's part over the
  real framed protocol: a saved password answers; a second prompt for the same
  operation marks the entry refused and is *not* answered again; one form
  answers both the username and the password prompt of a fresh host; and an SSH
  passphrase never becomes an entry.
- `crates/augur-core` checks the injected helper runs after the configured ones
  using the real `git` binary.
- Rust unit tests cover prompt parsing, matching precedence, the document
  schema, the plaintext file's permissions, and the helper's protocol output.
- `src/features/settings/credentialsForm.test.ts` covers the form rules, and
  `tests/credentials.spec.ts` drives the page, the add form, the delete
  confirmation, and the import against the stub backend.
