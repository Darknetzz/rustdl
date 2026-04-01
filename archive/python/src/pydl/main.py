from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path

import flet as ft

from pydl import ytdlp
from pydl.config import default_downloads, load_last_output_dir, save_last_output_dir
from pydl.models import QueueItem, VideoPreview

RESOLVE_CONCURRENCY = 3
THUMB_CONCURRENCY = 5
CARD_MIN_WIDTH = 280
CARD_MAX_WIDTH = 420
CARD_GAP = 12
MAX_LOG_CHARS = 28000

WINDOW_DEFAULT_WIDTH = 1280
WINDOW_DEFAULT_HEIGHT = 880

# Flet desktop scroll views use Material canvas/scaffold behind flex children; match page + scroll to it.
_APP_BG = "#121212"
_VIDEOS_EMPTY_HEIGHT = 180

# Cap for the video grid; grows with card count up to this (also scaled by window height).
VIDEOS_VIEWPORT_MAX_CAP = 700
# Approximate vertical space in each card below the thumbnail (text, progress, buttons, padding).
_CARD_BELOW_THUMB = 210


FORMAT_PRESETS: dict[str, tuple[str, list[str]]] = {
    "best": ("Best (recommended)", ["-f", "bestvideo+bestaudio/best"]),
    "mp4": ("Best MP4 (h264/aac)", ["-f", "bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/best"]),
    "audio_mp3": ("Audio only (MP3 192k)", ["-x", "--audio-format", "mp3", "--audio-quality", "192K"]),
    "audio_m4a": ("Audio only (M4A)", ["-f", "bestaudio[ext=m4a]/bestaudio", "-x", "--audio-format", "m4a"]),
}


def _format_duration(sec: int | None) -> str:
    if sec is None:
        return "—"
    m, s = divmod(int(sec), 60)
    h, m = divmod(m, 60)
    if h:
        return f"{h}:{m:02d}:{s:02d}"
    return f"{m}:{s:02d}"


def _grid_layout(page_width: int | float | None) -> tuple[int, int, int]:
    width = int(page_width or 1100)
    usable = max(width - 100, 640)
    columns = 1 if usable < 780 else 2 if usable < 1160 else 3
    card_w = int((usable - (columns - 1) * CARD_GAP) / columns)
    card_w = max(CARD_MIN_WIDTH, min(CARD_MAX_WIDTH, card_w))
    thumb_h = int(card_w * 9 / 16)
    return columns, card_w, thumb_h


def _card_size(page_width: int | float | None) -> tuple[int, int]:
    _, card_w, thumb_h = _grid_layout(page_width)
    return card_w, thumb_h


def _videos_viewport_cap(page: ft.Page) -> int:
    win_h = page.window.height or WINDOW_DEFAULT_HEIGHT
    return int(max(260, min(win_h * 0.58, VIDEOS_VIEWPORT_MAX_CAP)))


def _dark_color_scheme() -> ft.ColorScheme:
    s = _APP_BG
    return ft.ColorScheme(
        primary=ft.Colors.BLUE_400,
        surface=s,
        surface_dim=s,
        surface_bright=s,
        surface_container=s,
        surface_container_high=s,
        surface_container_highest=s,
        surface_container_low=s,
        surface_container_lowest=s,
        on_surface="#ececec",
    )


def _app_theme() -> ft.Theme:
    return ft.Theme(
        color_scheme=_dark_color_scheme(),
        canvas_color=_APP_BG,
        scaffold_bgcolor=_APP_BG,
    )


def _queue_dedupe_keys(queue: list[QueueItem]) -> set[str]:
    keys: set[str] = set()
    for it in queue:
        keys.add(ytdlp.normalize_url_for_dedupe(it.source_line))
        if it.webpage_url:
            keys.add(ytdlp.normalize_url_for_dedupe(it.webpage_url))
        if it.video_id:
            keys.add(f"vid:{it.video_id}")
    return {k for k in keys if k}


def _sync_videos_viewport(page: ft.Page, items: list[QueueItem], previews_panel: ft.Container, previews_rows: ft.Column) -> None:
    """Size the grid to content; scroll internally only when taller than the cap."""
    cols, _, thumb_h = _grid_layout(page.width)
    n = len(items)
    cap = _videos_viewport_cap(page)
    row_h = thumb_h + _CARD_BELOW_THUMB

    if n == 0:
        previews_panel.height = _VIDEOS_EMPTY_HEIGHT
        previews_rows.scroll = ft.ScrollMode.HIDDEN
        return

    rows = (n + cols - 1) // cols
    content_h = rows * row_h + max(0, rows - 1) * CARD_GAP + 8

    if content_h > cap:
        previews_panel.height = cap
        previews_rows.scroll = ft.ScrollMode.AUTO
    else:
        previews_panel.height = content_h
        previews_rows.scroll = ft.ScrollMode.HIDDEN


def _status_color(status: str) -> str:
    return (
        ft.Colors.BLUE_300
        if status == "downloading"
        else ft.Colors.GREEN_400
        if status == "done"
        else ft.Colors.RED_400
        if status == "failed"
        else ft.Colors.AMBER_400
        if status == "queued"
        else ft.Colors.GREY_500
    )


@dataclass
class _CardUi:
    bar: ft.ProgressBar
    pct: ft.Text
    size: ft.Text
    status: ft.Text
    detail: ft.Text


async def main(page: ft.Page) -> None:
    page.title = "pydl"
    page.theme_mode = ft.ThemeMode.DARK
    page.bgcolor = _APP_BG
    page.decoration = ft.BoxDecoration(bgcolor=_APP_BG)
    page.theme = _app_theme()
    page.dark_theme = _app_theme()
    page.window.min_width = 860
    page.window.min_height = 760
    page.window.width = WINDOW_DEFAULT_WIDTH
    page.window.height = WINDOW_DEFAULT_HEIGHT
    page.window.bgcolor = _APP_BG
    page.padding = 0
    page.scroll = None
    if not page.web:
        await page.window.center()

    items: list[QueueItem] = []
    image_refs: list[ft.Image] = []
    card_ui: dict[int, _CardUi] = {}
    download_queue: asyncio.Queue[int] = asyncio.Queue()
    worker_tasks: list[asyncio.Task[None]] = []
    workers_started = False
    next_item_id = 1

    def show_snack(msg: str) -> None:
        page.show_dialog(ft.SnackBar(msg))

    last_dir = load_last_output_dir() or default_downloads()
    out_field = ft.TextField(
        label="Output folder",
        value=str(last_dir),
        read_only=False,
        expand=True,
        dense=True,
    )
    format_preset = ft.Dropdown(
        label="Format preset",
        value="best",
        expand=True,
        dense=True,
        options=[ft.dropdown.Option(k, text=v[0]) for k, v in FORMAT_PRESETS.items()],
    )
    subtitle_mode = ft.Dropdown(
        label="Subtitles",
        value="off",
        width=180,
        dense=True,
        options=[
            ft.dropdown.Option("off", text="Off"),
            ft.dropdown.Option("auto", text="Auto subtitles"),
            ft.dropdown.Option("regular", text="Regular subtitles"),
        ],
    )
    write_thumbnail_chk = ft.Checkbox(label="Write thumbnail file", value=False)
    keep_part_chk = ft.Checkbox(label="Keep .part files", value=False)
    url_field = ft.TextField(
        label="URLs (one per line)",
        multiline=True,
        min_lines=5,
        max_lines=12,
        expand=True,
        dense=True,
    )
    log_field = ft.TextField(
        label="Log",
        multiline=True,
        min_lines=8,
        read_only=True,
        dense=True,
    )
    worker_count = ft.Dropdown(
        label="Parallel downloads",
        width=190,
        dense=True,
        value="3",
        options=[ft.dropdown.Option(str(i)) for i in range(1, 7)],
    )
    playlist_note = ft.Text(
        "",
        color=ft.Colors.AMBER_400,
        visible=False,
    )
    deps_status = ft.Text(
        "",
        size=12,
        color=ft.Colors.GREY_500,
    )

    def refresh_deps_banner() -> None:
        t = ytdlp.get_external_tools()
        deps_status.value = " · ".join(
            [
                f"yt-dlp: {'found' if t.ytdlp else 'missing'}",
                f"ffmpeg: {'found' if t.ffmpeg else 'missing'}",
                f"ffprobe: {'found' if t.ffprobe else 'missing'}",
            ]
        )
        if not t.ytdlp:
            deps_status.color = ft.Colors.RED_400
        elif not t.ffmpeg or not t.ffprobe:
            deps_status.color = ft.Colors.AMBER_400
        else:
            deps_status.color = ft.Colors.GREEN_400

    cards_wrap = ft.Row(wrap=True, spacing=12, run_spacing=12)
    previews_rows = ft.Column(
        scroll=ft.ScrollMode.HIDDEN,
        spacing=0,
        tight=True,
    )
    # Same bgcolor as page so scroll/clipping never reveals a light Material surface.
    previews_panel = ft.Container(
        content=previews_rows,
        bgcolor=_APP_BG,
        clip_behavior=ft.ClipBehavior.HARD_EDGE,
        height=_VIDEOS_EMPTY_HEIGHT,
    )

    def append_log(line: str) -> None:
        combined = (log_field.value or "") + line
        if len(combined) > MAX_LOG_CHARS:
            combined = combined[-MAX_LOG_CHARS:]
        log_field.value = combined

    def selected_extra_args() -> list[str]:
        preset_key = format_preset.value or "best"
        _, preset_args = FORMAT_PRESETS.get(preset_key, FORMAT_PRESETS["best"])
        args = list(preset_args)
        sub_mode = subtitle_mode.value or "off"
        if sub_mode == "auto":
            args.extend(["--write-auto-subs", "--sub-langs", "all"])
        elif sub_mode == "regular":
            args.extend(["--write-subs", "--sub-langs", "all"])
        if write_thumbnail_chk.value:
            args.append("--write-thumbnail")
        if keep_part_chk.value:
            args.append("--keep-part")
        return args

    def update_status_summary() -> None:
        idle = sum(1 for it in items if it.status == "idle")
        q = sum(1 for it in items if it.status == "queued")
        active = sum(1 for it in items if it.status == "downloading")
        done = sum(1 for it in items if it.status == "done")
        failed = sum(1 for it in items if it.status == "failed")
        status_text.value = f"Downloads: {idle} ready, {q} queued, {active} active, {done} done, {failed} failed"

    def item_by_id(iid: int) -> QueueItem | None:
        for it in items:
            if it.item_id == iid:
                return it
        return None

    def update_item_card(iid: int) -> None:
        it = item_by_id(iid)
        ui = card_ui.get(iid)
        if not it or not ui:
            return
        pct = max(0.0, min(it.percent, 100.0))
        ui.bar.value = pct / 100.0 if it.status in ("downloading", "done") else 0.0
        ui.pct.value = f"{pct:.1f}%"
        ui.size.value = it.size_text
        ui.status.value = it.status.capitalize()
        ui.status.color = _status_color(it.status)
        ui.detail.value = (it.detail or "")[:160] + ("…" if len(it.detail or "") > 160 else "")

    def refresh_previews_container() -> None:
        if not cards_wrap.controls:
            previews_rows.controls = [
                ft.Container(
                    padding=ft.Padding.symmetric(vertical=12),
                    alignment=ft.Alignment(0, 0),
                    content=ft.Column(
                        [
                            ft.Icon(ft.Icons.VIDEO_LIBRARY_OUTLINED, size=28, color=ft.Colors.GREY_500),
                            ft.Text("Nothing here yet", size=14, color=ft.Colors.GREY_300),
                            ft.Text(
                                "Paste URL(s) and click Add URLs to fetch previews.",
                                size=12,
                                color=ft.Colors.GREY_500,
                            ),
                        ],
                        tight=True,
                        spacing=4,
                        horizontal_alignment=ft.CrossAxisAlignment.CENTER,
                    ),
                )
            ]
        else:
            previews_rows.controls = [cards_wrap]

        _sync_videos_viewport(page, items, previews_panel, previews_rows)

    def rebuild_cards() -> None:
        card_ui.clear()
        card_w, thumb_h = _card_size(page.width)
        cards_wrap.controls.clear()
        image_refs.clear()
        for it in items:
            if it.error:
                img = ft.Image(
                    src="",
                    width=card_w,
                    height=thumb_h,
                    fit=ft.BoxFit.CONTAIN,
                    border_radius=8,
                    visible=False,
                )
                image_refs.append(img)
                thumb_area = ft.Container(
                    width=card_w,
                    height=thumb_h,
                    bgcolor=ft.Colors.GREY_900,
                    border_radius=8,
                    alignment=ft.Alignment(0, 0),
                    content=ft.Column(
                        [
                            ft.Icon(ft.Icons.ERROR_OUTLINE, color=ft.Colors.RED_400, size=40),
                            ft.Text(
                                it.error[:500] + ("…" if len(it.error) > 500 else ""),
                                size=12,
                                color=ft.Colors.GREY_400,
                                text_align=ft.TextAlign.CENTER,
                            ),
                        ],
                        tight=True,
                        horizontal_alignment=ft.CrossAxisAlignment.CENTER,
                    ),
                )
            else:
                has_thumb = it.thumbnail_bytes is not None
                img = ft.Image(
                    src=it.thumbnail_bytes or "",
                    width=card_w,
                    height=thumb_h,
                    fit=ft.BoxFit.CONTAIN,
                    border_radius=8,
                    visible=has_thumb,
                )
                image_refs.append(img)
                thumb_area = ft.Stack(
                    [
                        ft.Container(
                            width=card_w,
                            height=thumb_h,
                            bgcolor=ft.Colors.GREY_900,
                            border_radius=8,
                            alignment=ft.Alignment(0, 0),
                            content=ft.Icon(
                                ft.Icons.VIDEO_LIBRARY_OUTLINED,
                                size=48,
                                color=ft.Colors.GREY_600,
                            ),
                        ),
                        img,
                    ],
                    width=card_w,
                    height=thumb_h,
                )

            subtitle = ft.Text(
                f"{_format_duration(it.duration)}"
                + (f" · {it.uploader}" if it.uploader else ""),
                size=12,
                color=ft.Colors.GREY_500,
            )

            pct = max(0.0, min(it.percent, 100.0))
            bar = ft.ProgressBar(value=pct / 100.0 if it.status in ("downloading", "done") else 0.0)
            pct_t = ft.Text(f"{pct:.1f}%", size=12)
            size_t = ft.Text(it.size_text, size=12, color=ft.Colors.GREY_400)
            status_t = ft.Text(it.status.capitalize(), size=12, color=_status_color(it.status))
            detail_t = ft.Text((it.detail or "")[:120], size=11, color=ft.Colors.GREY_500)
            card_ui[it.item_id] = _CardUi(bar=bar, pct=pct_t, size=size_t, status=status_t, detail=detail_t)

            dl_url = it.webpage_url or it.source_line
            url_hint = ft.Text(
                dl_url[:70] + ("…" if len(dl_url) > 70 else ""),
                size=10,
                color=ft.Colors.GREY_600,
            )

            card = ft.Container(
                width=card_w + 20,
                padding=10,
                bgcolor=ft.Colors.with_opacity(0.06, ft.Colors.WHITE),
                border_radius=12,
                content=ft.Column(
                    [
                        thumb_area,
                        ft.Text(
                            it.title[:120] + ("…" if len(it.title) > 120 else ""),
                            size=14,
                            weight=ft.FontWeight.W_500,
                        ),
                        subtitle,
                        url_hint,
                        ft.Row([status_t, ft.Container(expand=True), pct_t, size_t], spacing=6),
                        bar,
                        detail_t,
                        ft.Row(
                            [
                                ft.TextButton(
                                    "Remove",
                                    on_click=lambda e, iid=it.item_id: remove_item(iid),
                                ),
                            ],
                        ),
                    ],
                    tight=True,
                    spacing=6,
                ),
            )
            cards_wrap.controls.append(card)

        capped = any(it.playlist_capped for it in items)
        playlist_note.visible = capped
        playlist_note.value = (
            "Playlist preview: showing first "
            f"{ytdlp.PLAYLIST_PREVIEW_CAP} videos only. Download still uses the full playlist URL."
            if capped
            else ""
        )
        update_status_summary()
        refresh_previews_container()

    def remove_item(iid: int) -> None:
        it = item_by_id(iid)
        if not it:
            return
        if it.status in ("queued", "downloading"):
            show_snack("Cannot remove while queued or downloading.")
            return
        nonlocal items
        items = [x for x in items if x.item_id != iid]
        rebuild_cards()
        page.update()
        asyncio.create_task(load_thumbnails())

    async def ensure_workers() -> None:
        nonlocal workers_started
        if workers_started:
            return
        workers_started = True
        workers = int(worker_count.value or "3")
        worker_count.disabled = True
        for idx in range(workers):
            worker_tasks.append(asyncio.create_task(download_worker(idx + 1)))

    async def download_worker(worker_id: int) -> None:
        while True:
            iid = await download_queue.get()
            it = item_by_id(iid)
            if it is None:
                download_queue.task_done()
                continue
            out = (out_field.value or "").strip()
            if not out or not Path(out).is_dir():
                it.status = "failed"
                it.detail = "Invalid output folder"
                it.percent = 0.0
                update_item_card(iid)
                page.update()
                download_queue.task_done()
                continue

            save_last_output_dir(Path(out))
            if it.error:
                it.status = "failed"
                it.detail = "Metadata error — fix URL and re-add"
                update_item_card(iid)
                page.update()
                download_queue.task_done()
                continue

            it.status = "downloading"
            it.detail = f"Worker {worker_id}"
            update_item_card(iid)
            page.update()
            target = it.webpage_url or it.source_line
            try:
                last_line = ""
                async for line in ytdlp.stream_download([target], out, it.extra_args):
                    last_line = line.strip()
                    append_log(f"[item {it.item_id}] {line}")
                    percent, size_text = ytdlp.parse_progress_line(line)
                    if percent is not None:
                        it.percent = percent
                    if size_text:
                        it.size_text = size_text
                    if last_line:
                        it.detail = last_line
                    update_item_card(iid)
                    page.update()

                if it.status != "failed":
                    it.status = "done"
                    it.percent = max(it.percent, 100.0)
                    it.detail = "Completed"
            except FileNotFoundError:
                it.status = "failed"
                it.detail = "yt-dlp not found on PATH"
            except Exception as e:  # noqa: BLE001
                it.status = "failed"
                it.detail = str(e)
            finally:
                update_item_card(iid)
                update_status_summary()
                page.update()
                download_queue.task_done()

    async def load_thumbnails() -> None:
        sem = asyncio.Semaphore(THUMB_CONCURRENCY)

        async def _one(i: int) -> None:
            async with sem:
                if i >= len(items) or i >= len(image_refs):
                    return
                it = items[i]
                if it.error or not it.thumbnail_url:
                    return
                data = await asyncio.to_thread(ytdlp.fetch_thumbnail_bytes, it.thumbnail_url)
                if data and i < len(image_refs):
                    it.thumbnail_bytes = data
                    image_refs[i].src = data
                    image_refs[i].visible = True
                    page.update()

        await asyncio.gather(*(_one(i) for i in range(len(items))))

    async def on_add_urls(_: ft.ControlEvent) -> None:
        nonlocal items, next_item_id
        raw = url_field.value or ""
        lines = [ln.strip() for ln in raw.splitlines() if ln.strip()]
        if not lines:
            show_snack("Add at least one URL.")
            return
        if not ytdlp.get_external_tools().ytdlp:
            refresh_deps_banner()
            show_snack("yt-dlp not found on PATH. Install it and ensure it is on PATH.")
            page.update()
            return

        existing_keys = _queue_dedupe_keys(items)
        to_fetch: list[str] = []
        seen_keys: set[str] = set()
        skipped = 0
        for ln in lines:
            key = ytdlp.normalize_url_for_dedupe(ln)
            if not key:
                continue
            if key in seen_keys:
                skipped += 1
                continue
            seen_keys.add(key)
            if key in existing_keys:
                skipped += 1
                continue
            to_fetch.append(ln)

        if not to_fetch:
            show_snack(
                "Nothing new to add — those URLs are already in the list, or repeated in the input."
            )
            return

        url_field.value = ""
        preview_btn.disabled = True
        page.update()
        try:
            sem = asyncio.Semaphore(RESOLVE_CONCURRENCY)

            async def _resolve_line(line: str) -> tuple[list[VideoPreview], None]:
                async with sem:
                    return await asyncio.to_thread(ytdlp.resolve_url_to_previews, line)

            results = await asyncio.gather(*(_resolve_line(ln) for ln in to_fetch))
            accum_keys = set(_queue_dedupe_keys(items))
            for rows, _ in results:
                for pv in rows:
                    k_web = ytdlp.normalize_url_for_dedupe(pv.webpage_url)
                    k_vid = f"vid:{pv.video_id}" if pv.video_id else ""
                    if (k_vid and k_vid in accum_keys) or (k_web and k_web in accum_keys):
                        skipped += 1
                        continue
                    items.append(QueueItem.from_preview(next_item_id, pv))
                    next_item_id += 1
                    if k_vid:
                        accum_keys.add(k_vid)
                    if k_web:
                        accum_keys.add(k_web)
            rebuild_cards()
            page.update()
            await load_thumbnails()
            if skipped:
                show_snack(f"Fetched previews. Skipped {skipped} duplicate line(s).")
        finally:
            preview_btn.disabled = False
            refresh_deps_banner()
            page.update()

    async def start_downloads(_: ft.ControlEvent) -> None:
        if not items:
            show_snack("Add URLs first.")
            return
        if not ytdlp.get_external_tools().ytdlp:
            refresh_deps_banner()
            show_snack("yt-dlp not found on PATH. Install it and ensure it is on PATH.")
            page.update()
            return
        out = (out_field.value or "").strip()
        if not out or not Path(out).is_dir():
            show_snack("Choose a valid output folder.")
            return
        pending = [it for it in items if it.status == "idle" and not it.error]
        if not pending:
            show_snack("Nothing to download (all started, finished, or have errors).")
            return
        await ensure_workers()
        for it in pending:
            it.status = "queued"
            it.extra_args = selected_extra_args()
            it.detail = "Queued"
            update_item_card(it.item_id)
            await download_queue.put(it.item_id)
        update_status_summary()
        refresh_deps_banner()
        page.update()
        tools = ytdlp.get_external_tools()
        n = len(pending)
        if not tools.ffmpeg or not tools.ffprobe:
            show_snack(
                f"Started {n} download(s). ffmpeg and/or ffprobe missing on PATH — "
                "merging or audio extraction may fail. Install ffmpeg (includes ffprobe)."
            )
        else:
            show_snack(f"Started {n} download(s).")

    async def on_pick_folder(_: ft.ControlEvent) -> None:
        out_field.value = str(default_downloads())
        page.update()

    def on_clear(_: ft.ControlEvent) -> None:
        nonlocal items, next_item_id
        busy = any(it.status in ("queued", "downloading") for it in items)
        if busy:
            show_snack("Wait for downloads to finish or fail before clearing.")
            return
        items.clear()
        next_item_id = 1
        rebuild_cards()
        page.update()

    preview_btn = ft.FilledButton("Add URLs", on_click=on_add_urls)
    start_btn = ft.FilledButton("Start downloads", on_click=start_downloads)
    clear_btn = ft.TextButton("Clear list", on_click=on_clear)
    reset_dir_btn = ft.TextButton("Use Downloads", on_click=on_pick_folder)
    clear_log_btn = ft.TextButton("Clear log", on_click=lambda _: setattr(log_field, "value", "") or page.update())
    status_text = ft.Text(
        "Downloads: 0 ready, 0 queued, 0 active, 0 done, 0 failed",
        size=12,
        color=ft.Colors.GREY_500,
    )

    def on_resize(_: ft.ControlEvent) -> None:
        rebuild_cards()
        page.update()

    page.on_resize = on_resize
    rebuild_cards()
    refresh_deps_banner()

    page.add(
        ft.Stack(
            [
                ft.Container(
                    left=0,
                    top=0,
                    right=0,
                    bottom=0,
                    bgcolor=_APP_BG,
                ),
                ft.Container(
                    left=0,
                    top=0,
                    right=0,
                    bottom=0,
                    padding=20,
                    content=ft.Column(
                        [
                            ft.Text("pydl", size=28, weight=ft.FontWeight.BOLD),
                            ft.Text(
                                "Add URLs to load previews; start downloads to see progress on each card.",
                                size=13,
                                color=ft.Colors.GREY_400,
                            ),
                            deps_status,
                            ft.Row([url_field]),
                            ft.Row(
                                [
                                    preview_btn,
                                    start_btn,
                                    clear_btn,
                                    playlist_note,
                                    ft.Container(expand=True),
                                    status_text,
                                ],
                                alignment=ft.MainAxisAlignment.START,
                                vertical_alignment=ft.CrossAxisAlignment.CENTER,
                                wrap=True,
                            ),
                            ft.Divider(),
                            ft.Text("Videos", size=16, weight=ft.FontWeight.W_600),
                            previews_panel,
                            ft.Divider(),
                            ft.Row(
                                [
                                    out_field,
                                    reset_dir_btn,
                                    worker_count,
                                ],
                                vertical_alignment=ft.CrossAxisAlignment.CENTER,
                            ),
                            ft.Row([format_preset, subtitle_mode]),
                            ft.Row([write_thumbnail_chk, keep_part_chk], wrap=True),
                            ft.Row([clear_log_btn], alignment=ft.MainAxisAlignment.END),
                            ft.Container(content=log_field, height=160),
                        ],
                        spacing=10,
                        tight=True,
                    ),
                ),
            ],
            expand=True,
            fit=ft.StackFit.EXPAND,
        )
    )
    # Re-apply viewport sizing once controls are mounted; avoids oversized empty panel on startup.
    refresh_previews_container()
    page.update()


def run_app() -> None:
    ft.run(main, view=ft.AppView.FLET_APP)
