use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{profiles_path, AppSettings};

/// Downloader-related settings captured by a named profile.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DownloadProfileFields {
    pub yt_dlp_extra_args: String,
    pub yt_dlp_unlimited_retries: bool,
    pub yt_dlp_retry_count: u32,
    pub yt_ignore_errors: bool,
    pub yt_restrict_filenames: bool,
    pub yt_write_info_json: bool,
    pub yt_write_auto_subs: bool,
    pub yt_embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub ffmpeg_post_args: String,
    pub ffmpeg_faststart: bool,
    pub ffmpeg_remux_mp4: bool,
    pub ffmpeg_extract_audio_mp3: bool,
    pub quality_preset: String,
    pub quality_format_custom: String,
    pub merge_container: String,
    pub output_filename_template: String,
    pub yt_download_archive: String,
    pub yt_proxy: String,
    pub yt_sponsorblock_remove: bool,
    pub yt_sponsorblock_mark: String,
}

impl Default for DownloadProfileFields {
    fn default() -> Self {
        Self {
            yt_dlp_extra_args: "--merge-output-format mp4".to_owned(),
            yt_dlp_unlimited_retries: true,
            yt_dlp_retry_count: 10,
            yt_ignore_errors: false,
            yt_restrict_filenames: false,
            yt_write_info_json: false,
            yt_write_auto_subs: false,
            yt_embed_metadata: false,
            embed_thumbnail: false,
            ffmpeg_post_args: String::new(),
            ffmpeg_faststart: true,
            ffmpeg_remux_mp4: false,
            ffmpeg_extract_audio_mp3: false,
            quality_preset: "best".to_owned(),
            quality_format_custom: String::new(),
            merge_container: "default".to_owned(),
            output_filename_template: crate::config::DEFAULT_OUTPUT_FILENAME_TEMPLATE.to_owned(),
            yt_download_archive: String::new(),
            yt_proxy: String::new(),
            yt_sponsorblock_remove: false,
            yt_sponsorblock_mark: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadProfile {
    pub name: String,
    #[serde(default)]
    pub builtin: bool,
    #[serde(flatten)]
    pub fields: DownloadProfileFields,
}

impl DownloadProfile {
    pub fn apply_to(&self, settings: &mut AppSettings) {
        let f = &self.fields;
        settings.yt_dlp_extra_args = f.yt_dlp_extra_args.clone();
        settings.yt_dlp_unlimited_retries = f.yt_dlp_unlimited_retries;
        settings.yt_dlp_retry_count = f.yt_dlp_retry_count;
        settings.yt_ignore_errors = f.yt_ignore_errors;
        settings.yt_restrict_filenames = f.yt_restrict_filenames;
        settings.yt_write_info_json = f.yt_write_info_json;
        settings.yt_write_auto_subs = f.yt_write_auto_subs;
        settings.yt_embed_metadata = f.yt_embed_metadata;
        settings.embed_thumbnail = f.embed_thumbnail;
        settings.ffmpeg_post_args = f.ffmpeg_post_args.clone();
        settings.ffmpeg_faststart = f.ffmpeg_faststart;
        settings.ffmpeg_remux_mp4 = f.ffmpeg_remux_mp4;
        settings.ffmpeg_extract_audio_mp3 = f.ffmpeg_extract_audio_mp3;
        settings.quality_preset = f.quality_preset.clone();
        settings.quality_format_custom = f.quality_format_custom.clone();
        settings.merge_container = f.merge_container.clone();
        settings.output_filename_template = f.output_filename_template.clone();
        settings.yt_download_archive = f.yt_download_archive.clone();
        settings.yt_proxy = f.yt_proxy.clone();
        settings.yt_sponsorblock_remove = f.yt_sponsorblock_remove;
        settings.yt_sponsorblock_mark = f.yt_sponsorblock_mark.clone();
        settings.active_profile = self.name.clone();
    }

    pub fn from_settings(name: &str, settings: &AppSettings, builtin: bool) -> Self {
        Self {
            name: name.to_owned(),
            builtin,
            fields: DownloadProfileFields {
                yt_dlp_extra_args: settings.yt_dlp_extra_args.clone(),
                yt_dlp_unlimited_retries: settings.yt_dlp_unlimited_retries,
                yt_dlp_retry_count: settings.yt_dlp_retry_count,
                yt_ignore_errors: settings.yt_ignore_errors,
                yt_restrict_filenames: settings.yt_restrict_filenames,
                yt_write_info_json: settings.yt_write_info_json,
                yt_write_auto_subs: settings.yt_write_auto_subs,
                yt_embed_metadata: settings.yt_embed_metadata,
                embed_thumbnail: settings.embed_thumbnail,
                ffmpeg_post_args: settings.ffmpeg_post_args.clone(),
                ffmpeg_faststart: settings.ffmpeg_faststart,
                ffmpeg_remux_mp4: settings.ffmpeg_remux_mp4,
                ffmpeg_extract_audio_mp3: settings.ffmpeg_extract_audio_mp3,
                quality_preset: settings.quality_preset.clone(),
                quality_format_custom: settings.quality_format_custom.clone(),
                merge_container: settings.merge_container.clone(),
                output_filename_template: settings.output_filename_template.clone(),
                yt_download_archive: settings.yt_download_archive.clone(),
                yt_proxy: settings.yt_proxy.clone(),
                yt_sponsorblock_remove: settings.yt_sponsorblock_remove,
                yt_sponsorblock_mark: settings.yt_sponsorblock_mark.clone(),
            },
        }
    }
}

pub fn builtin_profiles() -> Vec<DownloadProfile> {
    vec![
        DownloadProfile {
            name: "Best quality".to_owned(),
            builtin: true,
            fields: DownloadProfileFields {
                yt_dlp_extra_args: "--merge-output-format mp4".to_owned(),
                ffmpeg_faststart: true,
                ..DownloadProfileFields::default()
            },
        },
        DownloadProfile {
            name: "Audio only".to_owned(),
            builtin: true,
            fields: DownloadProfileFields {
                yt_dlp_extra_args: String::new(),
                ffmpeg_extract_audio_mp3: true,
                ffmpeg_faststart: false,
                ffmpeg_remux_mp4: false,
                quality_preset: "audio".to_owned(),
                ..DownloadProfileFields::default()
            },
        },
        DownloadProfile {
            name: "Fast download".to_owned(),
            builtin: true,
            fields: DownloadProfileFields {
                yt_dlp_extra_args: "--concurrent-fragments 4".to_owned(),
                yt_ignore_errors: true,
                ffmpeg_faststart: false,
                ..DownloadProfileFields::default()
            },
        },
        DownloadProfile {
            name: "Archive mode".to_owned(),
            builtin: true,
            fields: DownloadProfileFields {
                yt_dlp_extra_args: "--write-description".to_owned(),
                yt_write_info_json: true,
                yt_embed_metadata: true,
                yt_write_auto_subs: true,
                ..DownloadProfileFields::default()
            },
        },
    ]
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProfileStore {
    #[serde(default)]
    pub user_profiles: Vec<DownloadProfile>,
}

pub fn load_profiles() -> ProfileStore {
    let path = profiles_path();
    let raw = match fs::read_to_string(&path) {
        Ok(v) => v,
        Err(_) => return ProfileStore::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_profiles(store: &ProfileStore) -> Result<()> {
    let path = profiles_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create profiles directory: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    let raw = serde_json::to_string_pretty(store).context("failed to serialize profiles")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
    Ok(())
}

pub fn all_profiles(store: &ProfileStore) -> Vec<DownloadProfile> {
    let mut out = builtin_profiles();
    for p in &store.user_profiles {
        if !out.iter().any(|b| b.name == p.name) {
            out.push(p.clone());
        }
    }
    out
}

pub fn find_profile(store: &ProfileStore, name: &str) -> Option<DownloadProfile> {
    all_profiles(store).into_iter().find(|p| p.name == name)
}

pub fn save_user_profile(store: &mut ProfileStore, profile: DownloadProfile) -> Result<()> {
    if profile.builtin {
        return Ok(());
    }
    if let Some(idx) = store
        .user_profiles
        .iter()
        .position(|p| p.name == profile.name)
    {
        store.user_profiles[idx] = profile;
    } else {
        store.user_profiles.push(profile);
    }
    save_profiles(store)
}

#[allow(dead_code)]
pub fn delete_user_profile(store: &mut ProfileStore, name: &str) -> Result<()> {
    store.user_profiles.retain(|p| p.name != name);
    save_profiles(store)
}

pub fn export_profiles_json(store: &ProfileStore, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).ok();
        }
    }
    let raw = serde_json::to_string_pretty(store).context("failed to serialize profiles")?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
    Ok(())
}

pub fn import_profiles_json(path: &Path) -> Result<ProfileStore> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.to_string_lossy()))?;
    serde_json::from_str(&raw).context("invalid profiles JSON")
}
