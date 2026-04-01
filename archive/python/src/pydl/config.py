from __future__ import annotations

import json
import shutil
from pathlib import Path

_CONFIG_NAME = "pydl_config.json"
_LEGACY_DIR = "yt_glp"
_LEGACY_CONFIG_NAME = "yt_glp_config.json"


def config_path() -> Path:
    base = Path.home() / ".config"
    try:
        base.mkdir(parents=True, exist_ok=True)
    except OSError:
        return Path.cwd() / _CONFIG_NAME
    return base / "pydl" / _CONFIG_NAME


def _legacy_config_paths() -> list[Path]:
    base = Path.home() / ".config"
    return [
        base / _LEGACY_DIR / _LEGACY_CONFIG_NAME,
        Path.cwd() / _LEGACY_CONFIG_NAME,
    ]


def _maybe_migrate_legacy(new_path: Path) -> None:
    if new_path.is_file():
        return
    for old in _legacy_config_paths():
        if old.is_file():
            try:
                new_path.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(old, new_path)
            except OSError:
                pass
            return


def load_last_output_dir() -> Path | None:
    p = config_path()
    _maybe_migrate_legacy(p)
    if not p.is_file():
        return None
    try:
        data = json.loads(p.read_text(encoding="utf-8"))
        out = data.get("output_dir")
        if not out:
            return None
        path = Path(out)
        return path if path.is_dir() else None
    except (OSError, json.JSONDecodeError, TypeError):
        return None


def save_last_output_dir(path: Path) -> None:
    p = config_path()
    try:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(
            json.dumps({"output_dir": str(path.resolve())}, indent=2),
            encoding="utf-8",
        )
    except OSError:
        pass


def default_downloads() -> Path:
    home = Path.home()
    d = home / "Downloads"
    return d if d.is_dir() else home
