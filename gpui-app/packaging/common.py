"""Shared helpers for the Augur Git packaging scripts."""

import re
import subprocess
from pathlib import Path


PROJECT_ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = PROJECT_ROOT / "Cargo.toml"
ASSETS_DIR = PROJECT_ROOT / "assets"
TARGET_DIR = PROJECT_ROOT / "target"


def read_version() -> str:
    """Read the package version from Cargo.toml."""
    text = CARGO_TOML.read_text(encoding="utf-8")
    match = re.search(r'(?m)^\s*version\s*=\s*"([^"]+)"', text)
    if match is None:
        raise ValueError(f"Could not read the package version from {CARGO_TOML}")
    return match.group(1)


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def run(command: list[object], **kwargs: object) -> subprocess.CompletedProcess[bytes]:
    print(f"  -> {' '.join(str(item) for item in command)}")
    return subprocess.run(command, check=True, **kwargs)


def build_bins(release: bool, extra_args: list[str] | None = None) -> Path:
    """Run `cargo build --bins` and return the profile directory holding the executables."""
    command = ["cargo", "build"]
    if release:
        command.append("--release")
    command.append("--bins")
    command.extend(extra_args or [])
    run(command, cwd=PROJECT_ROOT)
    return TARGET_DIR / ("release" if release else "debug")


def variant_suffix(no_default_features: bool) -> str:
    """Return the filename suffix appended to No AI artifacts."""
    return "-no-ai" if no_default_features else ""


def variant_label(no_default_features: bool) -> str:
    """Return a human-readable label for the build variant."""
    return "No AI (--no-default-features)" if no_default_features else "AI (default features)"
