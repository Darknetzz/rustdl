#!/usr/bin/env python3
"""
Build a single-file pydl desktop executable.

Uses `flet pack`, which is the supported route for Flet apps: it wraps PyInstaller
with the right `--collect-*` / hidden imports so the GUI runtime bundles correctly.
A plain `pyinstaller pack_app.py` one-liner often misses those and fails at runtime.

**This module is the single source of truth** for the build command. Thin wrappers
only `cd` to the repo root and invoke Python:

- Windows: ``scripts/build_binary.ps1``
- macOS / Linux: ``scripts/build_binary.sh`` (optional: ``chmod +x``)

Usage (from repository root):

    python scripts/build_binary.py
    ./scripts/build_binary.sh
    .\\scripts\\build_binary.ps1

Examples:

    python scripts/build_binary.py --name pydl --distpath dist
    python scripts/build_binary.py --onedir
    python scripts/build_binary.py -- --icon path/to/icon.ico

Arguments after a lone ``--`` are forwarded to ``flet pack`` verbatim.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACK_SCRIPT = ROOT / "pack_app.py"


def _flet_pack_cmd() -> list[str]:
    exe = shutil.which("flet")
    if exe:
        return [exe, "pack"]
    return [sys.executable, "-m", "flet", "pack"]


def main() -> int:
    raw = sys.argv[1:]
    if "--" in raw:
        split = raw.index("--")
        our_argv = raw[:split]
        flet_extra = raw[split + 1 :]
    else:
        our_argv = raw
        flet_extra = []

    parser = argparse.ArgumentParser(
        description="Build pydl via flet pack (single-file exe by default on Windows).",
    )
    parser.add_argument("--name", "-n", default="pydl", help="Executable base name (default: pydl)")
    parser.add_argument(
        "--distpath",
        default="dist",
        help="Output directory (default: dist)",
    )
    parser.add_argument(
        "--onedir",
        "-D",
        action="store_true",
        help="One-folder bundle instead of a single executable",
    )
    parser.add_argument(
        "--no-yes",
        dest="yes",
        action="store_false",
        help="Do not pass -y (allow flet pack prompts)",
    )
    parser.set_defaults(yes=True)
    args = parser.parse_args(our_argv)

    if not PACK_SCRIPT.is_file():
        print(f"Expected {PACK_SCRIPT!s} — run from repo root.", file=sys.stderr)
        return 1

    cmd = [
        *_flet_pack_cmd(),
        str(PACK_SCRIPT),
        "-n",
        args.name,
        "--distpath",
        args.distpath,
    ]
    if args.yes:
        cmd.append("-y")
    if args.onedir:
        cmd.append("--onedir")
    cmd.extend(flet_extra)

    print("Command:", " ".join(cmd), flush=True)
    return subprocess.call(cmd, cwd=ROOT)


if __name__ == "__main__":
    raise SystemExit(main())
