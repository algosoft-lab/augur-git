#!/usr/bin/env python3
"""Build an AppImage for Augur Git on Linux."""

import argparse
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


APP_NAME = "Augur Git"
APP_ID = "augur-git"
BINARY_NAME = "augur-git"
APP_COMMENT = "Cross-platform Git GUI client"
APP_CATEGORIES = "Development;RevisionControl;"
APPIMAGETOOL_URL = "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"

PROJECT_ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = PROJECT_ROOT / "Cargo.toml"
ASSETS_DIR = PROJECT_ROOT / "assets"
TARGET_DIR = PROJECT_ROOT / "target"


def read_version() -> str:
    """Read the package version from Cargo.toml."""
    text = CARGO_TOML.read_text(encoding="utf-8")
    match = re.search(r"(?m)^\s*version\s*=\s*\"([^\"]+)\"", text)
    if match is None:
        raise ValueError(f"Could not read the package version from {CARGO_TOML}")
    return match.group(1)


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def run(command: list[object], **kwargs: object) -> subprocess.CompletedProcess[bytes]:
    print(f"  -> {' '.join(str(item) for item in command)}")
    return subprocess.run(command, check=True, **kwargs)


def detect_architecture() -> str:
    machine = platform.machine().lower()
    if machine in {"x86_64", "amd64"}:
        return "x86_64"
    if machine in {"aarch64", "arm64"}:
        return "aarch64"
    raise RuntimeError(f"Unsupported Linux architecture: {machine}")


def get_appimagetool(architecture: str, requested_path: Path | None) -> Path:
    """Find appimagetool or download the x86_64 release into a local cache."""
    if requested_path is not None:
        if not requested_path.exists():
            print(f"[ERROR] appimagetool was not found: {requested_path}")
            raise SystemExit(1)
        return requested_path

    installed = shutil.which("appimagetool")
    if installed:
        return Path(installed)

    if architecture != "x86_64":
        print("[ERROR] Automatic appimagetool download is available only for x86_64.")
        print("        Pass an ARM64 appimagetool with --appimagetool.")
        raise SystemExit(1)

    cache_directory = Path.home() / ".cache" / "augur-git-appimage"
    tool_path = cache_directory / "appimagetool"
    if tool_path.exists():
        print(f"  [OK] Using cached appimagetool: {tool_path}")
        return tool_path

    print(f"  Downloading appimagetool from {APPIMAGETOOL_URL}...")
    ensure_dir(cache_directory)
    import urllib.request

    urllib.request.urlretrieve(APPIMAGETOOL_URL, tool_path)
    tool_path.chmod(0o755)
    print(f"  [OK] Downloaded appimagetool: {tool_path}")
    return tool_path


def build_appimage(
    version: str,
    output_dir: Path,
    release: bool,
    architecture: str,
    appimagetool_path: Path | None,
) -> Path:
    profile = "release" if release else "debug"
    binary_source = TARGET_DIR / profile / BINARY_NAME
    if not binary_source.exists():
        print(f"[ERROR] Executable not found: {binary_source}")
        print(f"        Build it first with: cargo build{' --release' if release else ''}")
        raise SystemExit(1)

    appimage_name = f"{APP_ID}-{version}-{architecture}.AppImage"
    print("\n" + "=" * 60)
    print("  Augur Git AppImage")
    print(f"  Version: {version}  Profile: {profile}  Architecture: {architecture}")
    print("=" * 60 + "\n")

    with tempfile.TemporaryDirectory(prefix="augur-git-appimage-") as temporary_directory:
        appdir = Path(temporary_directory) / "AppDir"
        binary_directory = ensure_dir(appdir / "usr" / "bin")
        applications_directory = ensure_dir(appdir / "usr" / "share" / "applications")
        icon_directory = ensure_dir(appdir / "usr" / "share" / "icons" / "hicolor" / "scalable" / "apps")

        print("[1/5] Copying executable...")
        binary_destination = binary_directory / BINARY_NAME
        shutil.copy2(binary_source, binary_destination)
        binary_destination.chmod(0o755)
        print(f"  [OK] {binary_source.stat().st_size / 1024 / 1024:.1f} MB")

        print("[2/5] Installing application icon...")
        icon_source = ASSETS_DIR / "augur-git-logo.svg"
        if icon_source.exists():
            shutil.copy2(icon_source, icon_directory / f"{APP_ID}.svg")
            shutil.copy2(icon_source, appdir / f"{APP_ID}.svg")
            shutil.copy2(icon_source, appdir / ".DirIcon")
            print(f"  [OK] {icon_source}")
        else:
            print(f"  [WARN] Icon source not found: {icon_source}")

        print("[3/5] Generating desktop entry...")
        desktop_entry = f"""[Desktop Entry]
Type=Application
Name={APP_NAME}
GenericName=Git Client
Comment={APP_COMMENT}
Exec={BINARY_NAME}
Icon={APP_ID}
Terminal=false
StartupNotify=true
Categories={APP_CATEGORIES}
Keywords=git;repository;version control;development;
"""
        desktop_path = applications_directory / f"{APP_ID}.desktop"
        desktop_path.write_text(desktop_entry, encoding="utf-8")
        shutil.copy2(desktop_path, appdir / desktop_path.name)
        print(f"  [OK] {desktop_path}")

        print("[4/5] Generating AppRun...")
        apprun = appdir / "AppRun"
        apprun.write_text(
            f"""#!/bin/sh
SELF=$(readlink -f "$0")
HERE=${{SELF%/*}}
export PATH="$HERE/usr/bin:${{PATH}}"
export APPDIR="$HERE"
exec "$HERE/usr/bin/{BINARY_NAME}" "$@"
""",
            encoding="utf-8",
        )
        apprun.chmod(0o755)
        print(f"  [OK] {apprun}")

        print("[5/5] Building AppImage...")
        appimagetool = get_appimagetool(architecture, appimagetool_path)
        ensure_dir(output_dir)
        output_path = output_dir / appimage_name
        environment = os.environ.copy()
        environment["ARCH"] = architecture
        run([appimagetool, "--no-appstream", appdir, output_path], env=environment)

    if not output_path.exists():
        print("[ERROR] appimagetool did not create the AppImage.")
        raise SystemExit(1)

    print("\n" + "=" * 60)
    print(f"  [OK] AppImage: {output_path}")
    print(f"  [OK] Size: {output_path.stat().st_size / 1024 / 1024:.1f} MB")
    print(f"  Run with: chmod +x '{output_path}' && '{output_path}'")
    print("=" * 60 + "\n")
    return output_path


def main() -> int:
    parser = argparse.ArgumentParser(description="Build the Augur Git AppImage")
    parser.add_argument("--version", "-v", default=None, help="Version (default: read from Cargo.toml)")
    parser.add_argument("--output", "-o", default="packaging/out", help="Output directory (default: packaging/out)")
    parser.add_argument("--debug", action="store_true", help="Use the debug executable")
    parser.add_argument("--arch", choices=("x86_64", "aarch64"), default=None, help="AppImage architecture")
    parser.add_argument("--appimagetool", default=None, help="Path to an appimagetool executable")
    args = parser.parse_args()

    if not sys.platform.startswith("linux"):
        print("[ERROR] The AppImage packaging script must run on Linux.")
        return 1

    version = args.version or read_version()
    output_dir = Path(args.output).resolve()
    architecture = args.arch or detect_architecture()
    appimagetool_path = Path(args.appimagetool).resolve() if args.appimagetool else None
    build_appimage(version, output_dir, release=not args.debug, architecture=architecture, appimagetool_path=appimagetool_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
