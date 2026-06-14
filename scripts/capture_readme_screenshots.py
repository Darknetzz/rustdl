#!/usr/bin/env python3
"""Capture README screenshots for the desktop app and LAN web UI.

Requires a release binary (scripts/build_binary.ps1 or build_binary.sh),
Pillow, and on Windows pywin32 for desktop window capture.

Usage (from repo root):
  python scripts/capture_readme_screenshots.py
  python scripts/capture_readme_screenshots.py --skip-desktop
  python scripts/capture_readme_screenshots.py --skip-web

Web UI capture prints the local URL and token; open the page in a browser,
authenticate, and save assets/screenshots/web-ui.png manually or with browser
automation. Desktop capture is fully automated on Windows.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
BINARY = REPO_ROOT / "target" / ("rustdl.exe" if sys.platform == "win32" else "rustdl")
OUT_DIR = REPO_ROOT / "assets" / "screenshots"
DEMO_QUEUE = OUT_DIR / "demo-queue.json"
DESKTOP_OUT = OUT_DIR / "desktop-app.png"
WEB_OUT = OUT_DIR / "web-ui.png"


def config_dir() -> Path:
    if sys.platform == "win32":
        appdata = Path.home() / "AppData" / "Roaming"
    else:
        appdata = Path.home() / ".config"
    return appdata / "rustdl"


def seed_demo_queue() -> tuple[Path, Path | None]:
    queue_path = config_dir() / "rustdl_queue.json"
    backup = queue_path.with_suffix(".json.readme-backup")
    if queue_path.exists():
        shutil.copy2(queue_path, backup)
    else:
        backup = None
        queue_path.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(DEMO_QUEUE, queue_path)
    return queue_path, backup


def restore_queue(queue_path: Path, backup: Path | None) -> None:
    if backup and backup.exists():
        shutil.copy2(backup, queue_path)
        backup.unlink()
    elif queue_path.exists():
        queue_path.write_text("[]", encoding="utf-8")


def patch_session_restore(always: bool) -> tuple[Path, str]:
    config_path = config_dir() / "rustdl_config.json"
    backup = config_path.with_suffix(".json.readme-backup")
    shutil.copy2(config_path, backup)
    cfg = json.loads(config_path.read_text(encoding="utf-8"))
    original = cfg.get("session_restore_preference", "ask")
    cfg["session_restore_preference"] = "always" if always else original
    config_path.write_text(json.dumps(cfg, indent=2) + "\n", encoding="utf-8")
    return backup, original


def restore_config(backup: Path, config_path: Path) -> None:
    if backup.exists():
        shutil.copy2(backup, config_path)
        backup.unlink()


def find_rustdl_hwnd() -> int | None:
    import win32gui

    found: list[int] = []

    def cb(hwnd: int, _: int) -> bool:
        if win32gui.IsWindowVisible(hwnd) and win32gui.GetWindowText(hwnd) == "rustdl":
            found.append(hwnd)
        return True

    win32gui.EnumWindows(cb, None)
    return found[0] if found else None


def capture_desktop_window(hwnd: int, path: Path) -> None:
    import ctypes

    import win32gui
    import win32ui
    from PIL import Image

    pw_render_full = 2
    left, top, right, bottom = win32gui.GetWindowRect(hwnd)
    width = right - left
    height = bottom - top
    hwnd_dc = win32gui.GetWindowDC(hwnd)
    mfc_dc = win32ui.CreateDCFromHandle(hwnd_dc)
    save_dc = mfc_dc.CreateCompatibleDC()
    bitmap = win32ui.CreateBitmap()
    bitmap.CreateCompatibleBitmap(mfc_dc, width, height)
    save_dc.SelectObject(bitmap)
    ok = ctypes.windll.user32.PrintWindow(hwnd, save_dc.GetSafeHdc(), pw_render_full)
    if not ok:
        ok = ctypes.windll.user32.PrintWindow(hwnd, save_dc.GetSafeHdc(), 0)
    if not ok:
        raise RuntimeError("PrintWindow failed")
    info = bitmap.GetInfo()
    bits = bitmap.GetBitmapBits(True)
    img = Image.frombuffer(
        "RGB",
        (info["bmWidth"], info["bmHeight"]),
        bits,
        "raw",
        "BGRX",
        0,
        1,
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)
    win32gui.DeleteObject(bitmap.GetHandle())
    save_dc.DeleteDC()
    mfc_dc.DeleteDC()
    win32gui.ReleaseDC(hwnd, hwnd_dc)


def capture_desktop() -> None:
    if sys.platform != "win32":
        raise SystemExit("Desktop capture is supported on Windows only.")

    try:
        import win32con  # noqa: F401
        import win32gui
    except ImportError as exc:
        raise SystemExit("Desktop capture requires pywin32: pip install pywin32") from exc

    queue_path, queue_backup = seed_demo_queue()
    config_path = config_dir() / "rustdl_config.json"
    config_backup, _ = patch_session_restore(always=True)
    proc = subprocess.Popen([str(BINARY)])
    try:
        hwnd = None
        deadline = time.time() + 30
        while time.time() < deadline:
            hwnd = find_rustdl_hwnd()
            if hwnd:
                break
            time.sleep(0.3)
        if not hwnd:
            raise RuntimeError("rustdl GUI window did not appear")
        win32gui.ShowWindow(hwnd, win32con.SW_RESTORE)
        time.sleep(4)
        capture_desktop_window(hwnd, DESKTOP_OUT)
        print(f"Saved desktop screenshot to {DESKTOP_OUT}")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        restore_config(config_backup, config_path)
        restore_queue(queue_path, queue_backup)


def print_web_capture_instructions(port: int) -> None:
    queue_path, queue_backup = seed_demo_queue()
    config_path = config_dir() / "rustdl_config.json"
    cfg = json.loads(config_path.read_text(encoding="utf-8"))
    token = cfg.get("web_auth_token", "").strip()
    proc = subprocess.Popen(
        [str(BINARY), "--web-only", "--host", "127.0.0.1", "--port", str(port)]
    )
    try:
        deadline = time.time() + 20
        ready = False
        while time.time() < deadline:
            try:
                import urllib.request

                with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=2) as resp:
                    ready = resp.status == 200
                    break
            except OSError:
                time.sleep(0.3)
        if not ready:
            raise RuntimeError(f"Web UI did not start on http://127.0.0.1:{port}/")
        print(f"Web UI: http://127.0.0.1:{port}/")
        print(f"API token: {token or '(generate in Settings → Web UI)'}")
        print(f"Save a Downloader screenshot to {WEB_OUT} (1440×900 viewport recommended).")
        input("Press Enter when the web screenshot is saved...")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        restore_queue(queue_path, queue_backup)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-desktop", action="store_true")
    parser.add_argument("--skip-web", action="store_true")
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()

    if not BINARY.is_file():
        raise SystemExit(f"Missing {BINARY} — build the release binary first.")
    if not DEMO_QUEUE.is_file():
        raise SystemExit(f"Missing demo queue fixture: {DEMO_QUEUE}")

    OUT_DIR.mkdir(parents=True, exist_ok=True)

    if not args.skip_desktop:
        capture_desktop()
    if not args.skip_web:
        print_web_capture_instructions(args.port)


if __name__ == "__main__":
    main()
