"""Entry point for `flet pack` / PyInstaller (run from repo root)."""
from __future__ import annotations

import sys
from pathlib import Path

# Allow `python pack_app.py` and packaging without an editable install.
_root = Path(__file__).resolve().parent
_src = _root / "src"
if str(_src) not in sys.path:
    sys.path.insert(0, str(_src))

from pydl.main import run_app  # noqa: E402

if __name__ == "__main__":
    run_app()
