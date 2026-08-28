#!/usr/bin/env python3
"""Build an Augur Git macOS application bundle and DMG disk image."""

import argparse
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


APP_NAME = "Augur Git"
APP_ID = "com.augur.git"
BINARY_NAME = "augur-git"
UNQUARANTINE_SCRIPT_NAME = "Remove Quarantine.command"

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


def icon_sources(explicit_source: Path | None) -> list[Path]:
    if explicit_source is not None:
        return [explicit_source]
    return [
        ASSETS_DIR / "augur-git.png",
        ASSETS_DIR / "app_256.png",
        ASSETS_DIR / "augur-git-logo.svg",
        ASSETS_DIR / "algogit.ico",
    ]


def prepare_icon_png(source: Path, temporary_directory: Path) -> Path | None:
    """Convert a supported icon source to a 1024px PNG for iconutil."""
    if source.suffix.lower() == ".png":
        return source

    output_path = temporary_directory / "AppIcon.png"
    suffix = source.suffix.lower()
    if suffix == ".svg":
        converter = shutil.which("rsvg-convert")
        if converter:
            run([converter, "-w", "1024", "-h", "1024", str(source), "-o", str(output_path)], capture_output=True)
            return output_path if output_path.exists() else None

        for command_name in ("magick", "convert"):
            converter = shutil.which(command_name)
            if converter:
                run(
                    [converter, str(source), "-background", "none", "-resize", "1024x1024", str(output_path)],
                    capture_output=True,
                )
                return output_path if output_path.exists() else None

    if suffix == ".ico":
        sips = shutil.which("sips")
        if sips:
            run([sips, "-s", "format", "png", str(source), "--out", str(output_path)], capture_output=True)
            return output_path if output_path.exists() else None

    return None


def make_icns(png_path: Path, output_path: Path) -> bool:
    """Create an .icns file from a PNG using the macOS image tools."""
    sips = shutil.which("sips")
    iconutil = shutil.which("iconutil")
    if sips is None or iconutil is None:
        print("  [WARN] sips or iconutil is unavailable; skipping the application icon.")
        return False

    with tempfile.TemporaryDirectory(prefix="augur-git-iconset-") as temporary_directory:
        iconset_directory = Path(temporary_directory) / "AppIcon.iconset"
        ensure_dir(iconset_directory)
        sizes = [
            (16, "icon_16x16.png"),
            (32, "icon_16x16@2x.png"),
            (32, "icon_32x32.png"),
            (64, "icon_32x32@2x.png"),
            (128, "icon_128x128.png"),
            (256, "icon_128x128@2x.png"),
            (256, "icon_256x256.png"),
            (512, "icon_256x256@2x.png"),
            (512, "icon_512x512.png"),
            (1024, "icon_512x512@2x.png"),
        ]
        try:
            for size, name in sizes:
                run(
                    [sips, "-z", str(size), str(size), str(png_path), "--out", str(iconset_directory / name)],
                    capture_output=True,
                )
            run([iconutil, "-c", "icns", str(iconset_directory), "--output", str(output_path)], capture_output=True)
        except subprocess.CalledProcessError:
            print(f"  [WARN] Could not create an .icns file from {png_path}.")
            return False

    if output_path.exists():
        print(f"  [OK] Application icon: {output_path}")
        return True
    print("  [WARN] iconutil did not produce an .icns file.")
    return False


def make_unquarantine_script(output_path: Path) -> Path:
    """Copy the Finder-runnable Gatekeeper workaround into the DMG root."""
    source_path = PROJECT_ROOT / "packaging" / "remove-quarantine.command"
    shutil.copy2(source_path, output_path)
    output_path.chmod(0o755)
    print(f"  [OK] Gatekeeper workaround: {output_path}")
    return output_path


def build_app(version: str, output_dir: Path, release: bool, explicit_icon: Path | None) -> Path:
    profile = "release" if release else "debug"
    binary_source = TARGET_DIR / profile / BINARY_NAME
    if not binary_source.exists():
        print(f"[ERROR] Executable not found: {binary_source}")
        print(f"        Build it first with: cargo build{' --release' if release else ''}")
        raise SystemExit(1)

    print("\n" + "=" * 60)
    print("  Augur Git macOS application bundle")
    print(f"  Version: {version}  Profile: {profile}")
    print("=" * 60 + "\n")

    ensure_dir(output_dir)
    app_bundle = output_dir / f"{APP_NAME}.app"
    if app_bundle.exists():
        print(f"  Removing existing application bundle: {app_bundle}")
        shutil.rmtree(app_bundle)

    macos_directory = ensure_dir(app_bundle / "Contents" / "MacOS")
    resources_directory = ensure_dir(app_bundle / "Contents" / "Resources")

    print("[1/4] Copying executable...")
    binary_destination = macos_directory / BINARY_NAME
    shutil.copy2(binary_source, binary_destination)
    binary_destination.chmod(0o755)
    print(f"  [OK] {binary_source.stat().st_size / 1024 / 1024:.1f} MB")

    print("[2/4] Generating application icon...")
    icon_created = False
    with tempfile.TemporaryDirectory(prefix="augur-git-icon-") as temporary_directory:
        temporary_path = Path(temporary_directory)
        for source in icon_sources(explicit_icon):
            if not source.exists():
                continue
            try:
                png_path = prepare_icon_png(source, temporary_path)
            except subprocess.CalledProcessError:
                print(f"  [WARN] Could not convert icon source: {source}")
                continue
            if png_path is not None and make_icns(png_path, resources_directory / "AppIcon.icns"):
                icon_created = True
                break
    if not icon_created:
        print("  [WARN] No compatible PNG icon source was found; the app will use the default icon.")

    print("[3/4] Generating Info.plist...")
    plist = {
        "CFBundleDevelopmentRegion": "en_US",
        "CFBundleDisplayName": APP_NAME,
        "CFBundleExecutable": BINARY_NAME,
        "CFBundleIdentifier": APP_ID,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": APP_NAME,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "11.0",
        "NSHighResolutionCapable": True,
        "NSHumanReadableCopyright": "Copyright © 2026 Augur Git contributors",
        "NSPrincipalClass": "NSApplication",
    }
    if icon_created:
        plist["CFBundleIconFile"] = "AppIcon"
    with (app_bundle / "Contents" / "Info.plist").open("wb") as plist_file:
        plistlib.dump(plist, plist_file)
    print("  [OK] Contents/Info.plist")

    print("[4/4] Generating PkgInfo...")
    (app_bundle / "Contents" / "PkgInfo").write_text("APPL????", encoding="ascii")
    print("  [OK] Contents/PkgInfo")

    app_size = sum(file.stat().st_size for file in app_bundle.rglob("*") if file.is_file())
    print("\n" + "=" * 60)
    print(f"  [OK] Application bundle: {app_bundle}")
    print(f"  [OK] Size: {app_size / 1024 / 1024:.1f} MB")
    print("  Open with:")
    print(f"    open '{app_bundle}'")
    print(f"    or: '{app_bundle}/Contents/MacOS/{BINARY_NAME}'")
    print("=" * 60 + "\n")
    return app_bundle


def build_dmg(app_bundle: Path, version: str, output_dir: Path, include_script: bool) -> Path:
    """Package the app, an Applications link, and the Gatekeeper workaround."""
    volume_name = f"{APP_NAME} v{version}"
    dmg_path = output_dir / f"{APP_NAME} v{version}.dmg"
    if dmg_path.exists():
        print(f"  Removing existing DMG: {dmg_path}")
        dmg_path.unlink()

    with tempfile.TemporaryDirectory(prefix="augur-git-dmg-") as temporary_directory:
        dmg_root = Path(temporary_directory) / "root"
        ensure_dir(dmg_root)

        print("\n[DMG/1] Copying application bundle...")
        shutil.copytree(app_bundle, dmg_root / app_bundle.name)
        print(f"  [OK] {app_bundle.name}")

        print("[DMG/2] Adding Applications shortcut...")
        os.symlink("/Applications", dmg_root / "Applications")
        print("  [OK] Applications -> /Applications")

        if include_script:
            print("[DMG/3] Adding Gatekeeper workaround...")
            make_unquarantine_script(dmg_root / UNQUARANTINE_SCRIPT_NAME)
        else:
            print("[DMG/3] Skipping Gatekeeper workaround (--no-script).")

        print("[DMG/4] Creating DMG...")
        run(
            [
                "hdiutil",
                "create",
                "-volname",
                volume_name,
                "-srcfolder",
                str(dmg_root),
                "-format",
                "UDZO",
                "-ov",
                "-quiet",
                str(dmg_path),
            ]
        )

    if not dmg_path.exists():
        print("[ERROR] hdiutil did not create the DMG.")
        raise SystemExit(1)

    print("\n" + "=" * 60)
    print(f"  [OK] DMG: {dmg_path}")
    print(f"  [OK] Size: {dmg_path.stat().st_size / 1024 / 1024:.1f} MB")
    print(f"  Open the DMG and drag {APP_NAME}.app to Applications.")
    if include_script:
        print(f"  Then run '{UNQUARANTINE_SCRIPT_NAME}' if macOS blocks the unsigned app.")
    print("=" * 60 + "\n")
    return dmg_path


def main() -> int:
    parser = argparse.ArgumentParser(description="Build the Augur Git macOS app and DMG")
    parser.add_argument("--version", "-v", default=None, help="Version (default: read from Cargo.toml)")
    parser.add_argument("--output", "-o", default="packaging/out", help="Output directory (default: packaging/out)")
    parser.add_argument("--debug", action="store_true", help="Use the debug executable")
    parser.add_argument("--no-dmg", action="store_true", help="Build only the .app bundle")
    parser.add_argument("--no-script", action="store_true", help="Do not include the Gatekeeper workaround in the DMG")
    parser.add_argument("--icon", default=None, help="Optional PNG, SVG, or ICO icon source")
    args = parser.parse_args()

    if sys.platform != "darwin":
        print("[ERROR] The macOS packaging script must run on macOS.")
        return 1

    version = args.version or read_version()
    output_dir = Path(args.output).resolve()
    explicit_icon = Path(args.icon).resolve() if args.icon else None
    if explicit_icon is not None and not explicit_icon.exists():
        print(f"[ERROR] Icon source not found: {explicit_icon}")
        return 1

    app_bundle = build_app(version, output_dir, release=not args.debug, explicit_icon=explicit_icon)
    if not args.no_dmg:
        build_dmg(app_bundle, version, output_dir, include_script=not args.no_script)
    return 0


if __name__ == "__main__":
    sys.exit(main())
