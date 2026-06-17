#!/usr/bin/env python3
"""Capture README screenshots for the desktop app and LAN web UI.

Requires a release binary (scripts/build_binary.ps1 or build_binary.sh),
Pillow, and on Windows pywin32 for desktop window capture. Optional:
playwright for automated web UI capture (`pip install playwright` then
`playwright install chromium`).

Usage (from repo root):
  python scripts/capture_readme_screenshots.py
  python scripts/capture_readme_screenshots.py --skip-desktop
  python scripts/capture_readme_screenshots.py --skip-web

Desktop capture sizes the rustdl window to 1440×900 (matching the web UI), hides the
activity log, and pre-seeds queue thumbnails from YouTube CDN. Uses a screen grab so
GPU-rendered card previews are visible (PrintWindow often shows black thumbnails).
Web capture uses Playwright when available; otherwise it prints the URL and waits for a manual save.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
BINARY = REPO_ROOT / "target" / "release" / ("rustdl.exe" if sys.platform == "win32" else "rustdl")
OUT_DIR = REPO_ROOT / "assets" / "screenshots"
DEMO_QUEUE = OUT_DIR / "demo-queue.json"
DESKTOP_OUT = OUT_DIR / "desktop-app.png"
WEB_OUT = OUT_DIR / "web-ui.png"
THUMB_WAIT_S = 12.0
WEB_VIEWPORT = (1440, 900)
DESKTOP_WINDOW = (1440, 1000)


def config_dir() -> Path:
    if sys.platform == "win32":
        appdata = Path.home() / "AppData" / "Roaming"
    else:
        appdata = Path.home() / ".config"
    return appdata / "rustdl"


def thumbnail_source_key(item: dict[str, Any]) -> str:
    return "|".join(
        [
            str(item.get("video_id", "")).strip(),
            str(item.get("thumbnail_url") or "").strip(),
            str(item.get("local_path") or "").strip(),
            str(item.get("webpage_url", "")).strip(),
            str(item.get("source_line", "")).strip(),
        ]
    )


def downloader_thumb_dir() -> Path:
    return config_dir() / "thumbnails" / "downloader"


def backup_demo_thumbnails(item_ids: list[int]) -> dict[int, tuple[Path | None, Path | None]]:
    """Backup existing on-disk thumbnails for demo item ids (img + meta)."""
    base = downloader_thumb_dir()
    backups: dict[int, tuple[Path | None, Path | None]] = {}
    for item_id in item_ids:
        img = base / f"{item_id}.img"
        meta = base / f"{item_id}.json"
        img_backup = None
        meta_backup = None
        if img.exists():
            img_backup = img.with_suffix(".img.readme-backup")
            shutil.copy2(img, img_backup)
        if meta.exists():
            meta_backup = meta.with_suffix(".json.readme-backup")
            shutil.copy2(meta, meta_backup)
        backups[item_id] = (img_backup, meta_backup)
    return backups


def restore_demo_thumbnails(
    item_ids: list[int], backups: dict[int, tuple[Path | None, Path | None]]
) -> None:
    base = downloader_thumb_dir()
    for item_id in item_ids:
        img = base / f"{item_id}.img"
        meta = base / f"{item_id}.json"
        img_backup, meta_backup = backups.get(item_id, (None, None))
        for path in (img, meta):
            if path.exists():
                path.unlink()
        if img_backup and img_backup.exists():
            shutil.copy2(img_backup, img)
            img_backup.unlink()
        if meta_backup and meta_backup.exists():
            shutil.copy2(meta_backup, meta)
            meta_backup.unlink()


def seed_demo_thumbnails(items: list[dict[str, Any]]) -> None:
    """Download YouTube preview images into the rustdl thumbnail cache."""
    base = downloader_thumb_dir()
    base.mkdir(parents=True, exist_ok=True)
    for item in items:
        item_id = int(item["item_id"])
        thumb_url = str(item.get("thumbnail_url") or "").strip()
        if not thumb_url:
            vid = str(item.get("video_id", "")).strip()
            if not vid:
                continue
            thumb_url = f"https://i.ytimg.com/vi/{vid}/hqdefault.jpg"
            item["thumbnail_url"] = thumb_url
        req = urllib.request.Request(
            thumb_url,
            headers={"User-Agent": "rustdl-readme-capture/1", "Referer": "https://www.youtube.com/"},
        )
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = resp.read()
        if len(data) < 32:
            raise RuntimeError(f"Thumbnail too small for item {item_id}")
        rel_path = f"thumbnails/downloader/{item_id}.img"
        (base / f"{item_id}.img").write_bytes(data)
        record = {
            "source_key": thumbnail_source_key(item),
            "content_type": "image/jpeg",
            "webpage_url": item.get("webpage_url", ""),
            "thumbnail_url": thumb_url,
            "source_line": item.get("source_line", ""),
            "image_path": rel_path,
        }
        (base / f"{item_id}.json").write_text(
            json.dumps(record, indent=2) + "\n", encoding="utf-8"
        )
        item["thumbnail_path"] = rel_path


def load_demo_items() -> list[dict[str, Any]]:
    return json.loads(DEMO_QUEUE.read_text(encoding="utf-8"))


def seed_demo_queue() -> tuple[Path, Path | None, list[int], dict[int, tuple[Path | None, Path | None]]]:
    queue_path = config_dir() / "rustdl_queue.json"
    backup = queue_path.with_suffix(".json.readme-backup")
    if queue_path.exists():
        shutil.copy2(queue_path, backup)
    else:
        backup = None
        queue_path.parent.mkdir(parents=True, exist_ok=True)
    items = load_demo_items()
    item_ids = [int(it["item_id"]) for it in items]
    thumb_backups = backup_demo_thumbnails(item_ids)
    seed_demo_thumbnails(items)
    queue_path.write_text(json.dumps(items, indent=2) + "\n", encoding="utf-8")
    return queue_path, backup, item_ids, thumb_backups


def restore_queue(
    queue_path: Path,
    backup: Path | None,
    item_ids: list[int],
    thumb_backups: dict[int, tuple[Path | None, Path | None]],
) -> None:
    restore_demo_thumbnails(item_ids, thumb_backups)
    if backup and backup.exists():
        shutil.copy2(backup, queue_path)
        backup.unlink()
    elif queue_path.exists():
        queue_path.write_text("[]\n", encoding="utf-8")


def patch_config_for_capture() -> Path:
    config_path = config_dir() / "rustdl_config.json"
    backup = config_path.with_suffix(".json.readme-backup")
    if not config_path.exists():
        raise SystemExit(f"Missing rustdl config: {config_path}")
    shutil.copy2(config_path, backup)
    cfg = json.loads(config_path.read_text(encoding="utf-8"))
    cfg["session_restore_preference"] = "always"
    cfg["show_first_run_hint"] = False
    cfg["logs_open"] = False
    cfg["web_ui_enabled"] = False
    cfg["videos_dock_height"] = 520.0
    whitelist = list(cfg.get("web_auth_ip_whitelist") or [])
    if "127.0.0.1" not in whitelist:
        whitelist.append("127.0.0.1")
    cfg["web_auth_ip_whitelist"] = whitelist
    config_path.write_text(json.dumps(cfg, indent=2) + "\n", encoding="utf-8")
    return backup


def restore_config(backup: Path, config_path: Path) -> None:
    if backup.exists():
        shutil.copy2(backup, config_path)
        backup.unlink()


def kill_existing_rustdl() -> None:
    if sys.platform == "win32":
        subprocess.run(
            ["taskkill", "/IM", "rustdl.exe", "/F"],
            capture_output=True,
            check=False,
        )
        time.sleep(0.5)


def find_rustdl_hwnd_for_pid(pid: int) -> int | None:
    import win32gui
    import win32process

    found: list[int] = []

    def cb(hwnd: int, _: int) -> bool:
        if not win32gui.IsWindowVisible(hwnd):
            return True
        if win32gui.GetWindowText(hwnd) != "rustdl":
            return True
        _, win_pid = win32process.GetWindowThreadProcessId(hwnd)
        if win_pid == pid:
            found.append(hwnd)
        return True

    win32gui.EnumWindows(cb, None)
    return found[0] if found else None


def find_rustdl_hwnd() -> int | None:
    import win32gui

    found: list[int] = []

    def cb(hwnd: int, _: int) -> bool:
        if win32gui.IsWindowVisible(hwnd) and win32gui.GetWindowText(hwnd) == "rustdl":
            found.append(hwnd)
        return True

    win32gui.EnumWindows(cb, None)
    return found[0] if found else None


def enable_per_monitor_dpi_awareness() -> None:
    try:
        import ctypes

        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        try:
            import ctypes

            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass


def force_foreground(hwnd: int) -> None:
    import ctypes

    import win32con
    import win32gui
    import win32process

    if win32gui.GetForegroundWindow() == hwnd:
        return
    try:
        win32gui.ShowWindow(hwnd, win32con.SW_SHOW)
        fg = win32gui.GetForegroundWindow()
        if fg and fg != hwnd:
            fg_tid, _ = win32process.GetWindowThreadProcessId(fg)
            target_tid, _ = win32process.GetWindowThreadProcessId(hwnd)
            ctypes.windll.user32.AttachThreadInput(fg_tid, target_tid, True)
            try:
                win32gui.SetForegroundWindow(hwnd)
            finally:
                ctypes.windll.user32.AttachThreadInput(fg_tid, target_tid, False)
        else:
            win32gui.SetForegroundWindow(hwnd)
    except Exception:
        pass
    time.sleep(0.4)


def size_window_for_capture(hwnd: int, width: int, height: int) -> None:
    import win32api
    import win32con
    import win32gui

    win32gui.ShowWindow(hwnd, win32con.SW_RESTORE)
    screen_w = win32api.GetSystemMetrics(0)
    screen_h = win32api.GetSystemMetrics(1)
    x = max(0, (screen_w - width) // 2)
    y = max(0, (screen_h - height) // 2)
    win32gui.SetWindowPos(
        hwnd,
        win32con.HWND_TOP,
        x,
        y,
        width,
        height,
        win32con.SWP_SHOWWINDOW,
    )
    force_foreground(hwnd)
    time.sleep(0.6)


def capture_desktop_window_printwindow(hwnd: int, path: Path) -> None:
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


def capture_desktop_window(hwnd: int, path: Path) -> None:
    """Grab the on-screen pixels for `hwnd` (works with GPU textures; PrintWindow often does not)."""
    import win32gui
    from PIL import ImageGrab

    enable_per_monitor_dpi_awareness()
    force_foreground(hwnd)
    if win32gui.GetForegroundWindow() != hwnd:
        raise RuntimeError("rustdl window is not in the foreground")
    left, top, right, bottom = win32gui.GetWindowRect(hwnd)
    if right <= left or bottom <= top:
        raise RuntimeError("Invalid window rectangle for screenshot")
    img = ImageGrab.grab(bbox=(left, top, right, bottom), all_screens=True)
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)


def capture_desktop() -> None:
    if sys.platform != "win32":
        raise SystemExit("Desktop capture is supported on Windows only.")

    try:
        import win32con  # noqa: F401
        import win32gui
    except ImportError as exc:
        raise SystemExit("Desktop capture requires pywin32: pip install pywin32") from exc

    kill_existing_rustdl()
    queue_path, queue_backup, item_ids, thumb_backups = seed_demo_queue()
    config_path = config_dir() / "rustdl_config.json"
    config_backup = patch_config_for_capture()
    proc = subprocess.Popen([str(BINARY)])
    try:
        hwnd = None
        deadline = time.time() + 30
        while time.time() < deadline:
            hwnd = find_rustdl_hwnd_for_pid(proc.pid)
            if hwnd:
                break
            time.sleep(0.3)
        if not hwnd:
            raise RuntimeError("rustdl GUI window did not appear for launched process")
        size_window_for_capture(hwnd, *DESKTOP_WINDOW)
        print(f"Waiting {THUMB_WAIT_S:.0f}s for queue thumbnails…")
        time.sleep(THUMB_WAIT_S)
        try:
            capture_desktop_window(hwnd, DESKTOP_OUT)
        except Exception as exc:
            print(f"Screen grab failed ({exc}); falling back to PrintWindow…")
            capture_desktop_window_printwindow(hwnd, DESKTOP_OUT)
        print(f"Saved desktop screenshot to {DESKTOP_OUT}")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        restore_config(config_backup, config_path)
        restore_queue(queue_path, queue_backup, item_ids, thumb_backups)


def playwright_available() -> bool:
    try:
        import playwright  # noqa: F401

        return True
    except ImportError:
        return False


def capture_web_playwright(port: int) -> None:
    from playwright.sync_api import Error as PlaywrightError
    from playwright.sync_api import sync_playwright

    url = f"http://127.0.0.1:{port}/"
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch()
            try:
                page = browser.new_page(
                    viewport={"width": WEB_VIEWPORT[0], "height": WEB_VIEWPORT[1]}
                )
                page.goto(url, wait_until="domcontentloaded", timeout=30_000)
                page.wait_for_function(
                    "() => !document.getElementById('app-main')?.classList.contains('hidden')",
                    timeout=30_000,
                )
                page.wait_for_function(
                    """() => {
                  const imgs = document.querySelectorAll('.card-thumb img:not(.hidden)');
                  return imgs.length >= 3;
                }""",
                    timeout=45_000,
                )
                time.sleep(0.5)
                page.screenshot(path=str(WEB_OUT), full_page=False)
                print(f"Saved web UI screenshot to {WEB_OUT}")
            finally:
                browser.close()
    except PlaywrightError as exc:
        print(f"Playwright capture failed: {exc}")
        print(f"Open {url} (127.0.0.1 is whitelisted during capture).")
        print(
            f"Save a Downloader screenshot to {WEB_OUT} "
            f"({WEB_VIEWPORT[0]}×{WEB_VIEWPORT[1]} viewport)."
        )
        input("Press Enter when the web screenshot is saved...")


def capture_web(port: int) -> None:
    kill_existing_rustdl()
    queue_path, queue_backup, item_ids, thumb_backups = seed_demo_queue()
    config_path = config_dir() / "rustdl_config.json"
    config_backup = patch_config_for_capture()
    proc = subprocess.Popen(
        [str(BINARY), "--web-only", "--host", "127.0.0.1", "--port", str(port)]
    )
    try:
        deadline = time.time() + 20
        ready = False
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=2) as resp:
                    ready = resp.status == 200
                    break
            except OSError:
                time.sleep(0.3)
        if not ready:
            raise RuntimeError(f"Web UI did not start on http://127.0.0.1:{port}/")

        if playwright_available():
            capture_web_playwright(port)
        else:
            print(f"Web UI: http://127.0.0.1:{port}/")
            print(f"Save a Downloader screenshot to {WEB_OUT} ({WEB_VIEWPORT[0]}×{WEB_VIEWPORT[1]} viewport).")
            print("Tip: pip install playwright && playwright install chromium for automated capture.")
            input("Press Enter when the web screenshot is saved...")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        restore_config(config_backup, config_path)
        restore_queue(queue_path, queue_backup, item_ids, thumb_backups)


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
        capture_web(args.port)


if __name__ == "__main__":
    main()
