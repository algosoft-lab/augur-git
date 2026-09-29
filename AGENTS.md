# Repository instructions

This repository contains two independent desktop applications: the primary
Tauri application in `tauri-app/`, which is under active development, and the
legacy GPUI application in `gpui-app/`. They share no application source,
assets, dependency manifests, lock files, build configuration, or runtime
configuration. Do not add cross-directory dependencies or configuration
import paths.

Unless a request explicitly names the GPUI application, interpret application
development, feature, and bug-fix requests as targeting the Tauri application.
Treat GPUI as legacy and make changes to it only when the request explicitly
asks for the GPUI version.

Use each application's own README and nested `AGENTS.md` for its commands,
architecture, and implementation rules. Keep application changes within that
application's directory. Shared repository-level content is limited to these
instructions, the top-level README and license, and GitHub Actions under
`.github/`.

## Writing and code

- Write all repository-facing documentation, comments, commit messages, and
  generated project content in English.
- Do not add comments that only restate the code. Add comments for design
  decisions, constraints, workarounds, or subtle edge cases.
- Before adding responsibility to a source file over 1,000 lines, consider
  splitting or refactoring it.

## Git

- Use Conventional Commits with concise, imperative English descriptions.
- Preserve unrelated working-tree changes.
