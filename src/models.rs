use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct VideoPreview {
    pub video_id: String,
    pub title: String,
    pub webpage_url: String,
    pub thumbnail_url: Option<String>,
    pub duration: Option<i64>,
    pub uploader: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub expected_size_bytes: Option<u64>,
    pub expected_size_approx: bool,
    pub source_line: String,
    pub error: Option<String>,
    pub playlist_capped: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueItem {
    pub item_id: u64,
    pub source_line: String,
    pub video_id: String,
    pub title: String,
    pub webpage_url: String,
    pub thumbnail_url: Option<String>,
    pub duration: Option<i64>,
    pub uploader: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub error: Option<String>,
    pub status: ItemStatus,
    pub percent: f32,
    pub size_text: String,
    pub speed_text: String,
    pub eta_text: String,
    pub detail: String,
}

impl Default for QueueItem {
    fn default() -> Self {
        Self {
            item_id: 0,
            source_line: String::new(),
            video_id: String::new(),
            title: String::new(),
            webpage_url: String::new(),
            thumbnail_url: None,
            duration: None,
            uploader: None,
            width: None,
            height: None,
            error: None,
            status: ItemStatus::Idle,
            percent: 0.0,
            size_text: "-".to_owned(),
            speed_text: "-".to_owned(),
            eta_text: "-".to_owned(),
            detail: String::new(),
        }
    }
}

impl QueueItem {
    pub fn pending_metadata(item_id: u64, source_line: String) -> Self {
        Self {
            item_id,
            source_line: source_line.clone(),
            video_id: String::new(),
            title: source_line,
            webpage_url: String::new(),
            thumbnail_url: None,
            duration: None,
            uploader: None,
            width: None,
            height: None,
            error: None,
            status: ItemStatus::Resolving,
            percent: 0.0,
            size_text: "-".to_owned(),
            speed_text: "-".to_owned(),
            eta_text: "-".to_owned(),
            detail: "Fetching metadata...".to_owned(),
        }
    }

    pub fn from_preview(item_id: u64, p: VideoPreview) -> Self {
        let mut size_text = "-".to_owned();
        if let Some(bytes) = p.expected_size_bytes {
            let rendered = human_bytes(bytes);
            size_text = if p.expected_size_approx {
                format!("~{rendered}")
            } else {
                rendered
            };
        }
        Self {
            item_id,
            source_line: p.source_line,
            video_id: p.video_id,
            title: if p.title.is_empty() {
                "(no title)".to_owned()
            } else {
                p.title
            },
            webpage_url: p.webpage_url,
            thumbnail_url: p.thumbnail_url,
            duration: p.duration,
            uploader: p.uploader,
            width: p.width,
            height: p.height,
            error: p.error,
            status: ItemStatus::Idle,
            percent: 0.0,
            size_text,
            speed_text: "-".to_owned(),
            eta_text: "-".to_owned(),
            detail: String::new(),
        }
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut n = bytes as f64;
    let mut idx = 0usize;
    while n >= 1024.0 && idx < UNITS.len() - 1 {
        n /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{}{}", n as u64, UNITS[idx])
    } else {
        format!("{n:.1}{}", UNITS[idx])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemStatus {
    Resolving,
    Idle,
    Queued,
    Downloading,
    Done,
    Failed,
}

impl ItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemStatus::Resolving => "Resolving",
            ItemStatus::Idle => "Idle",
            ItemStatus::Queued => "Queued",
            ItemStatus::Downloading => "Downloading",
            ItemStatus::Done => "Done",
            ItemStatus::Failed => "Failed",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Av1QueueItem {
    pub item_id: u64,
    pub source_path: String,
    pub output_path: String,
    pub status: ItemStatus,
    pub percent: f32,
    pub detail: String,
    pub input_bytes: u64,
    pub output_bytes: Option<u64>,
    pub video_codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub bitrate_bps: Option<u64>,
}

impl Default for Av1QueueItem {
    fn default() -> Self {
        Self {
            item_id: 0,
            source_path: String::new(),
            output_path: String::new(),
            status: ItemStatus::Idle,
            percent: 0.0,
            detail: String::new(),
            input_bytes: 0,
            output_bytes: None,
            video_codec: String::new(),
            width: None,
            height: None,
            fps: None,
            bitrate_bps: None,
        }
    }
}
