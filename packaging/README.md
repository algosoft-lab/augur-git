# Packaging

These scripts create native distribution artifacts for Windows, macOS, and
Linux. Run them from the repository root with `uv`.

All outputs default to `packaging/out`, which is ignored by Git.

## Windows

Install [Inno Setup 6](https://jrsoftware.org/isinfo.php), then run:

```bash
uv run packaging/build-windows.py
```

The script builds the release executable and creates an Inno Setup installer.
Use `--debug` to package a debug executable or `--skip-build` to package an
existing executable.

## macOS

Run the following commands on macOS:

```bash
cargo build --release
uv run packaging/build-mac-app.py
```

The script creates both an `.app` bundle and a `.dmg`. The DMG includes
`Remove Quarantine.command`, which removes the quarantine attribute from the
installed unsigned app when Gatekeeper blocks it. Use `--no-dmg` to create only
the app bundle, or `--no-script` to omit the workaround.

The macOS icon is generated with `sips` and `iconutil`. If a PNG icon is not
available, install an SVG converter such as `rsvg-convert` or ImageMagick, or
pass a PNG with `--icon`.

## Linux

Run the following commands on Linux:

```bash
cargo build --release
uv run packaging/build-appimage.py
```

The script uses `appimagetool` from `PATH` when available and otherwise caches
the x86_64 release under the user's cache directory. Use `--appimagetool` for
an architecture-specific tool, and `--arch aarch64` when packaging an ARM64
binary.
