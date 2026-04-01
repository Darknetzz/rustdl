from __future__ import annotations

import json
import re
import shutil
import subprocess
from collections.abc import AsyncIterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

import httpx

from pydl.models import VideoPreview

YTDLP_EXE = "yt-dlp"
FFMPEG_EXE = "ffmpeg"
FFPROBE_EXE = "ffprobe"
PLAYLIST_PREVIEW_CAP = 20
JSON_TIMEOUT = 120
THUMB_HTTP_TIMEOUT = 20.0
PROGRESS_PREFIX = "progress:"
_SIZE_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"]


def normalize_url_for_dedupe(url: str) -> str:
    """
    Return a stable key so alternate forms of the same link match (e.g. youtu.be vs watch?v=).
    YouTube IDs keep original casing. Non-YouTube URLs use lowercase host + path + query.
    """
    s = (url or "").strip()
    if not s:
        return ""
    try:
        p = urlparse(s)
    except Exception:  # noqa: BLE001
        return s

    scheme = (p.scheme or "https").lower()
    netloc = (p.netloc or "").lower()
    if netloc.startswith("www."):
        netloc = netloc[4:]

    path = (p.path or "").rstrip("/")
    query = parse_qs(p.query, keep_blank_values=False)

    # youtu.be/<id>
    if netloc == "youtu.be" and path:
        vid = path.split("/")[-1]
        if vid:
            return f"youtube:{vid}"

    if "youtube.com" in netloc or "youtube-nocookie.com" in netloc or "music.youtube.com" in netloc:
        if "v" in query and query["v"]:
            return f"youtube:{query['v'][0]}"
        if path.startswith("/shorts/"):
            vid = path.removeprefix("/shorts/").split("/")[0]
            if vid:
                return f"youtube:{vid}"
        if path.startswith("/live/"):
            vid = path.removeprefix("/live/").split("/")[0]
            if vid:
                return f"youtube:{vid}"

    # Generic: ignore fragment; stable query order
    q_flat = sorted((k, v[0]) for k, v in query.items() if v)
    q_part = "&".join(f"{k}={v}" for k, v in q_flat)
    base = f"{scheme}://{netloc}{path}"
    return f"{base}?{q_part}" if q_part else base


def ytdlp_binary() -> str:
    return shutil.which(YTDLP_EXE) or YTDLP_EXE


@dataclass(frozen=True)
class ExternalTools:
    """Resolved PATH locations for binaries pydl and yt-dlp may invoke."""

    ytdlp: str | None
    ffmpeg: str | None
    ffprobe: str | None


def get_external_tools() -> ExternalTools:
    return ExternalTools(
        ytdlp=shutil.which(YTDLP_EXE),
        ffmpeg=shutil.which(FFMPEG_EXE),
        ffprobe=shutil.which(FFPROBE_EXE),
    )


def _best_thumbnail_url(data: dict[str, Any]) -> str | None:
    thumbs = data.get("thumbnails")
    if isinstance(thumbs, list) and thumbs:
        with_height = [t for t in thumbs if isinstance(t, dict) and t.get("url")]
        if with_height:
            best = max(
                with_height,
                key=lambda t: (t.get("height") or 0, t.get("width") or 0),
            )
            u = best.get("url")
            if isinstance(u, str):
                return u
    t = data.get("thumbnail")
    return t if isinstance(t, str) else None


def _parse_entry(entry: dict[str, Any], source_line: str) -> VideoPreview:
    vid = entry.get("id")
    video_id = str(vid) if vid is not None else ""
    title = entry.get("title") or "(no title)"
    if not isinstance(title, str):
        title = str(title)
    web = entry.get("webpage_url") or entry.get("url") or source_line
    if not isinstance(web, str):
        web = str(web)
    dur = entry.get("duration")
    duration = int(dur) if isinstance(dur, (int, float)) else None
    up = entry.get("uploader") or entry.get("channel")
    uploader = up if isinstance(up, str) else None
    return VideoPreview(
        video_id=video_id,
        title=title,
        webpage_url=web,
        thumbnail_url=_best_thumbnail_url(entry),
        duration=duration,
        uploader=uploader,
        source_line=source_line,
    )


def parse_dump_to_previews(data: dict[str, Any], source_line: str) -> tuple[list[VideoPreview], bool]:
    """
    Turn yt-dlp -J JSON into preview rows. Returns (previews, playlist_was_capped).
    """
    entries = data.get("entries")
    if isinstance(entries, list) and entries:
        capped = len(entries) > PLAYLIST_PREVIEW_CAP
        slice_entries = entries[:PLAYLIST_PREVIEW_CAP]
        out: list[VideoPreview] = []
        for e in slice_entries:
            if e is None:
                continue
            if isinstance(e, dict):
                pv = _parse_entry(e, source_line)
                pv.playlist_capped = capped
                out.append(pv)
        return out, capped

    if data.get("_type") == "playlist" and isinstance(entries, list) and not entries:
        return (
            [
                VideoPreview(
                    video_id="",
                    title="Empty playlist",
                    webpage_url=source_line,
                    thumbnail_url=None,
                    duration=None,
                    uploader=None,
                    source_line=source_line,
                    error=None,
                )
            ],
            False,
        )

    return [_parse_entry(data, source_line)], False


def dump_json_skip_download(url: str) -> tuple[dict[str, Any] | None, str | None]:
    """Run yt-dlp -J --skip-download. Returns (json_dict, stderr_or_error_message)."""
    cmd = [
        ytdlp_binary(),
        "-J",
        "--no-warnings",
        "--skip-download",
        url,
    ]
    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=JSON_TIMEOUT,
            check=False,
        )
    except FileNotFoundError:
        return None, f"not found: {ytdlp_binary()!r} — install yt-dlp and ensure it is on PATH"
    except subprocess.TimeoutExpired:
        return None, "yt-dlp timed out while fetching metadata"

    if proc.returncode != 0:
        err = (proc.stderr or proc.stdout or "").strip() or f"exit {proc.returncode}"
        return None, err

    raw = (proc.stdout or "").strip()
    if not raw:
        return None, "empty response from yt-dlp"

    try:
        parsed: Any = json.loads(raw)
    except json.JSONDecodeError as e:
        return None, f"invalid JSON from yt-dlp: {e}"
    if parsed is None:
        return None, "yt-dlp returned no metadata (null)"
    if not isinstance(parsed, dict):
        return None, "unexpected JSON type from yt-dlp"
    return parsed, None


def resolve_url_to_previews(url: str) -> tuple[list[VideoPreview], str | None]:
    """Resolve one user URL line to a list of VideoPreview (playlist expands to many, capped)."""
    line = url.strip()
    if not line:
        return [], None
    data, err = dump_json_skip_download(line)
    if err:
        return [
            VideoPreview(
                video_id="",
                title="",
                webpage_url=line,
                thumbnail_url=None,
                duration=None,
                uploader=None,
                source_line=line,
                error=err,
            )
        ], None
    assert data is not None
    previews, _capped = parse_dump_to_previews(data, line)
    return previews, None


def fetch_thumbnail_bytes(url: str) -> bytes | None:
    try:
        with httpx.Client(
            timeout=THUMB_HTTP_TIMEOUT,
            follow_redirects=True,
            headers={"User-Agent": "pydl/0.1"},
        ) as client:
            r = client.get(url)
            r.raise_for_status()
            return r.content
    except httpx.HTTPError:
        return None


def _human_bytes(value: int | float | None) -> str | None:
    if value is None:
        return None
    n = float(value)
    if n < 0:
        return None
    unit_idx = 0
    while n >= 1024 and unit_idx < len(_SIZE_UNITS) - 1:
        n /= 1024
        unit_idx += 1
    if unit_idx == 0:
        return f"{int(n)}{_SIZE_UNITS[unit_idx]}"
    return f"{n:.1f}{_SIZE_UNITS[unit_idx]}"


def parse_progress_line(line: str) -> tuple[float | None, str | None]:
    """
    Parse yt-dlp progress output.
    Returns (percent, size_text) where size_text is like '12.3MiB/48.8MiB'.
    """
    clean = line.strip()
    if clean.startswith(PROGRESS_PREFIX):
        payload = clean[len(PROGRESS_PREFIX) :]
        parts = payload.split("|")
        if len(parts) >= 4:
            p_raw = parts[0].replace("%", "").strip()
            d_raw = parts[1].strip()
            t_raw = parts[2].strip()
            e_raw = parts[3].strip()
            try:
                percent = float(p_raw)
            except ValueError:
                percent = None
            downloaded = int(d_raw) if d_raw.isdigit() else None
            total = int(t_raw) if t_raw.isdigit() else (int(e_raw) if e_raw.isdigit() else None)
            d_text = _human_bytes(downloaded)
            t_text = _human_bytes(total)
            if d_text and t_text:
                return percent, f"{d_text}/{t_text}"
            if t_text:
                return percent, t_text
            return percent, None

    percent_match = re.search(r"(\d+(?:\.\d+)?)%", clean)
    percent = float(percent_match.group(1)) if percent_match else None
    size_match = re.search(r"of\s+([0-9.]+\s*[KMGTP]?i?B)", clean, flags=re.IGNORECASE)
    size_text = size_match.group(1).replace(" ", "") if size_match else None
    return percent, size_text


def build_download_command(
    urls: list[str],
    output_dir: str,
    extra_args: list[str] | None = None,
) -> list[str]:
    out_tmpl = str(Path(output_dir) / "%(title)s [%(id)s].%(ext)s")
    cmd: list[str] = [
        ytdlp_binary(),
        "--newline",
        "--progress-template",
        f"{PROGRESS_PREFIX}%(progress._percent_str)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s",
        "-o",
        out_tmpl,
    ]
    if extra_args:
        cmd.extend(extra_args)
    cmd.extend(urls)
    return cmd


async def stream_download(
    urls: list[str],
    output_dir: str,
    extra_args: list[str] | None = None,
) -> AsyncIterator[str]:
    """Run yt-dlp download; yield each line of combined stdout/stderr."""
    import asyncio

    cmd = build_download_command(urls, output_dir, extra_args)
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
    )
    assert proc.stdout is not None
    while True:
        line = await proc.stdout.readline()
        if not line:
            break
        yield line.decode("utf-8", errors="replace")
    rc = await proc.wait()
    if rc != 0:
        yield f"\n[process exited with code {rc}]\n"
