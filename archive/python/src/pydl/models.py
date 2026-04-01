from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class VideoPreview:
    """One row in the preview grid (single video or one playlist entry)."""

    video_id: str
    title: str
    webpage_url: str
    thumbnail_url: str | None
    duration: int | None
    uploader: str | None
    source_line: str
    error: str | None = None
    playlist_capped: bool = False
    extra: dict[str, Any] = field(default_factory=dict)


@dataclass
class QueueItem:
    """One card: metadata + download state (shown under the thumbnail)."""

    item_id: int
    source_line: str
    video_id: str = ""
    title: str = ""
    webpage_url: str = ""
    thumbnail_url: str | None = None
    duration: int | None = None
    uploader: str | None = None
    error: str | None = None
    playlist_capped: bool = False
    thumbnail_bytes: bytes | None = None
    status: str = "idle"  # idle | queued | downloading | done | failed
    percent: float = 0.0
    size_text: str = "—"
    detail: str = ""
    extra_args: list[str] = field(default_factory=list)

    @staticmethod
    def from_preview(item_id: int, pv: VideoPreview) -> QueueItem:
        return QueueItem(
            item_id=item_id,
            source_line=pv.source_line,
            video_id=pv.video_id,
            title=pv.title or "(no title)",
            webpage_url=pv.webpage_url,
            thumbnail_url=pv.thumbnail_url,
            duration=pv.duration,
            uploader=pv.uploader,
            error=pv.error,
            playlist_capped=pv.playlist_capped,
        )
