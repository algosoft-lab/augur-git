#!/usr/bin/env python3
"""Build an AppImage for Augur Git on Linux."""

import argparse
import os
import platform
import shutil
import sys
import tempfile
from pathlib import Path

from common import ASSETS_DIR, TARGET_DIR, build_bins, ensure_dir, read_version, run, variant_label, variant_suffix


APP_NAME = "Augur Git"
APP_ID = "augur-git"
BINARY_NAME = "augur-git"
APP_COMMENT = "Cross-platform Git GUI client"
APP_CATEGORIES = "Development;RevisionControl;"
APPIMAGETOOL_URL = "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"


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
    no_default_features: bool,
    skip_build: bool,
) -> Path:
    profile_dir = TARGET_DIR / ("release" if release else "debug")
    binary_source = profile_dir / BINARY_NAME
    variant = variant_suffix(no_default_features)
    appimage_name = f"{APP_ID}-{version}-{architecture}{variant}.AppImage"

    print("\n" + "=" * 60)
    print("  Augur Git AppImage")
    print(f"  Version: {version}  Profile: {'release' if release else 'debug'}  Architecture: {architecture}")
    print(f"  Variant: {variant_label(no_default_features)}")
    print("=" * 60 + "\n")

    if not skip_build:
        print("[1/6] Building executables...")
        profile_dir = build_bins(release, ["--no-default-features"] if no_default_features else None)
    else:
        print("[1/6] Skipping build (--skip-build).")

    if not binary_source.exists():
        print(f"[ERROR] Executable not found: {binary_source}")
        print("        Build it first without --skip-build, or with: cargo build --release --bins")
        raise SystemExit(1)

    with tempfile.TemporaryDirectory(prefix="augur-git-appimage-") as temporary_directory:
        appdir = Path(temporary_directory) / "AppDir"
        binary_directory = ensure_dir(appdir / "usr" / "bin")
        applications_directory = ensure_dir(appdir / "usr" / "share" / "applications")
        icon_directory = ensure_dir(appdir / "usr" / "share" / "icons" / "hicolor" / "scalable" / "apps")

        print("[2/6] Copying executable...")
        binary_destination = binary_directory / BINARY_NAME
        shutil.copy2(binary_source, binary_destination)
        binary_destination.chmod(0o755)
        print(f"  [OK] {binary_source.stat().st_size / 1024 / 1024:.1f} MB")

        alias_source = binary_source.parent / "augurgit"
        if alias_source.exists():
            alias_destination = binary_directory / "augurgit"
            shutil.copy2(alias_source, alias_destination)
            alias_destination.chmod(0o755)
            print(f"  [OK] {alias_destination} (CLI alias)")
        else:
            print(f"  [WARN] CLI alias executable not found: {alias_source}")

        print("[3/6] Installing application icon...")
        icon_source = ASSETS_DIR / "augur-git-logo.svg"
        if icon_source.exists():
            shutil.copy2(icon_source, icon_directory / f"{APP_ID}.svg")
            shutil.copy2(icon_source, appdir / f"{APP_ID}.svg")
            shutil.copy2(icon_source, appdir / ".DirIcon")
            print(f"  [OK] {icon_source}")
        else:
            print(f"  [WARN] Icon source not found: {icon_source}")

        print("[4/6] Generating desktop entry...")
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

        print("[5/6] Generating AppRun...")
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

        print("[6/6] Building AppImage...")
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
    parser.add_argument("--skip-build", action="store_true", help="Package an existing executable")
    parser.add_argument("--arch", choices=("x86_64", "aarch64"), default=None, help="AppImage architecture")
    parser.add_argument("--appimagetool", default=None, help="Path to an appimagetool executable")
    parser.add_argument(
        "--no-default-features",
        action="store_true",
        help="Build the No AI variant without default cargo features (plain Git GUI without the agent integration)",
    )
    args = parser.parse_args()

    if not sys.platform.startswith("linux"):
        print("[ERROR] The AppImage packaging script must run on Linux.")
        return 1

    version = args.version or read_version()
    output_dir = Path(args.output).resolve()
    architecture = args.arch or detect_architecture()
    appimagetool_path = Path(args.appimagetool).resolve() if args.appimagetool else None
    build_appimage(
        version,
        output_dir,
        release=not args.debug,
        architecture=architecture,
        appimagetool_path=appimagetool_path,
        no_default_features=args.no_default_features,
        skip_build=args.skip_build,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
