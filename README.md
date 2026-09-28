# Augur Git

Augur Git is an open-source desktop Git client. The Tauri application is the
primary version under active development. The GPUI application remains
available as a separate implementation.

The two applications are maintained independently. They do not share source
code, assets, dependency manifests or lock files, build configuration, or
runtime settings. Their configuration formats are incompatible, and neither
application reads, imports, migrates, or writes the other's settings.

## Applications

- **[Augur Git Tauri](tauri-app/README.md)** — the primary application, built
  with Tauri 2, React, and Vite. Its frontend and Rust backend have independent
  dependencies, build commands, and application data.
- **[Augur Git GPUI](gpui-app/README.md)** — the separate Rust and GPUI
  application, with its own dependencies, resources, packaging scripts, and
  optional coding-agent and Lua extension features.

Each application directory contains its own setup, test, build, and license
information. Start commands from within the corresponding directory.

## Repository-wide guidance

The root [AGENTS.md](AGENTS.md) contains instructions that apply to both
applications. Application-specific agent instructions live in each application
directory. GitHub Actions are kept in [`.github/`](.github/).

The repository is licensed under the [Apache License 2.0](LICENSE). Each
application also carries its own copy for standalone packaging.
