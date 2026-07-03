//! Named Video Converter setting bundles (local JSON, no cloud).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{load_json_file, AppSettings};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConvertPresetFields {
    pub convert_target_codec: String,
    pub convert_target_bitrate: String,
    pub convert_max_width: u32,
    pub convert_size_preset: String,
    pub convert_size_limit_kind: String,
    pub convert_size_limit_value: String,
    pub convert_size_limit_violation: String,
    pub convert_min_shrink_percent: f32,
    pub convert_cpu_threads: u32,
    pub convert_parallel: usize,
    pub convert_encoder_override: String,
    pub convert_recursive: bool,
    pub convert_dry_run: bool,
    pub convert_overwrite: bool,
    pub convert_reencode_target: bool,
    pub convert_use_recommended_container: bool,
    pub convert_delete_original: bool,
    pub convert_rename_original: bool,
    pub convert_output_dir: String,
    pub convert_post_move_subfolder: String,
    pub convert_post_filename_find: String,
    pub convert_post_filename_replace: String,
    pub convert_copy_subtitles: bool,
    pub convert_write_checksum: bool,
    pub convert_audio_extract: String,
    pub convert_subtitle_mode: String,
    pub convert_max_hw_encodes: usize,
}

impl Default for ConvertPresetFields {
    fn default() -> Self {
        Self {
            convert_target_codec: "av1".to_owned(),
            convert_target_bitrate: String::new(),
            convert_max_width: 1920,
            convert_size_preset: "balanced".to_owned(),
            convert_size_limit_kind: String::new(),
            convert_size_limit_value: String::new(),
            convert_size_limit_violation: crate::convert_size_limit::VIOLATION_SKIP.to_owned(),
            convert_min_shrink_percent: 0.0,
            convert_cpu_threads: 0,
            convert_parallel: 1,
            convert_encoder_override: String::new(),
            convert_recursive: true,
            convert_dry_run: false,
            convert_overwrite: false,
            convert_reencode_target: false,
            convert_use_recommended_container: true,
            convert_delete_original: false,
            convert_rename_original: false,
            convert_output_dir: String::new(),
            convert_post_move_subfolder: String::new(),
            convert_post_filename_find: String::new(),
            convert_post_filename_replace: String::new(),
            convert_copy_subtitles: false,
            convert_write_checksum: false,
            convert_audio_extract: "none".to_owned(),
            convert_subtitle_mode: String::new(),
            convert_max_hw_encodes: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConvertPreset {
    pub name: String,
    #[serde(flatten)]
    pub fields: ConvertPresetFields,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConvertPresetStore {
    pub presets: Vec<ConvertPreset>,
}

pub fn convert_presets_path() -> PathBuf {
    crate::config::rustdl_config_dir().join("rustdl_convert_presets.json")
}

pub fn load_convert_presets() -> ConvertPresetStore {
    load_json_file(convert_presets_path(), "convert presets")
}

pub fn save_convert_presets(store: &ConvertPresetStore) -> Result<()> {
    let path = convert_presets_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let raw = serde_json::to_string_pretty(store).context("serialize convert presets")?;
    fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

impl ConvertPresetFields {
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            convert_target_codec: settings.convert_target_codec.clone(),
            convert_target_bitrate: settings.convert_target_bitrate.clone(),
            convert_max_width: settings.convert_max_width,
            convert_size_preset: settings.convert_size_preset.clone(),
            convert_size_limit_kind: settings.convert_size_limit_kind.clone(),
            convert_size_limit_value: settings.convert_size_limit_value.clone(),
            convert_size_limit_violation: settings.convert_size_limit_violation.clone(),
            convert_min_shrink_percent: settings.convert_min_shrink_percent,
            convert_cpu_threads: settings.convert_cpu_threads,
            convert_parallel: settings.convert_parallel,
            convert_encoder_override: settings.convert_encoder_override.clone(),
            convert_recursive: settings.convert_recursive,
            convert_dry_run: settings.convert_dry_run,
            convert_overwrite: settings.convert_overwrite,
            convert_reencode_target: settings.convert_reencode_target,
            convert_use_recommended_container: settings.convert_use_recommended_container,
            convert_delete_original: settings.convert_delete_original,
            convert_rename_original: settings.convert_rename_original,
            convert_output_dir: settings.convert_output_dir.clone(),
            convert_post_move_subfolder: settings.convert_post_move_subfolder.clone(),
            convert_post_filename_find: settings.convert_post_filename_find.clone(),
            convert_post_filename_replace: settings.convert_post_filename_replace.clone(),
            convert_copy_subtitles: settings.convert_copy_subtitles,
            convert_write_checksum: settings.convert_write_checksum,
            convert_audio_extract: settings.convert_audio_extract.clone(),
            convert_subtitle_mode: settings.convert_subtitle_mode.clone(),
            convert_max_hw_encodes: settings.convert_max_hw_encodes,
        }
    }

    pub fn apply_to(&self, settings: &mut AppSettings) {
        settings.convert_target_codec = self.convert_target_codec.clone();
        settings.convert_target_bitrate = self.convert_target_bitrate.clone();
        settings.convert_max_width = self.convert_max_width;
        settings.convert_size_preset = self.convert_size_preset.clone();
        settings.convert_size_limit_kind = self.convert_size_limit_kind.clone();
        settings.convert_size_limit_value = self.convert_size_limit_value.clone();
        settings.convert_size_limit_violation = self.convert_size_limit_violation.clone();
        settings.convert_min_shrink_percent = self.convert_min_shrink_percent;
        settings.convert_cpu_threads = self.convert_cpu_threads;
        settings.convert_parallel = self.convert_parallel.clamp(1, 6);
        settings.convert_encoder_override = self.convert_encoder_override.clone();
        settings.convert_recursive = self.convert_recursive;
        settings.convert_dry_run = self.convert_dry_run;
        settings.convert_overwrite = self.convert_overwrite;
        settings.convert_reencode_target = self.convert_reencode_target;
        settings.convert_use_recommended_container = self.convert_use_recommended_container;
        settings.convert_delete_original = self.convert_delete_original;
        settings.convert_rename_original = self.convert_rename_original;
        settings.convert_output_dir = self.convert_output_dir.clone();
        settings.convert_post_move_subfolder = self.convert_post_move_subfolder.clone();
        settings.convert_post_filename_find = self.convert_post_filename_find.clone();
        settings.convert_post_filename_replace = self.convert_post_filename_replace.clone();
        settings.convert_copy_subtitles = self.convert_copy_subtitles;
        settings.convert_write_checksum = self.convert_write_checksum;
        settings.convert_audio_extract = self.convert_audio_extract.clone();
        settings.convert_subtitle_mode = self.convert_subtitle_mode.clone();
        settings.convert_max_hw_encodes = self.convert_max_hw_encodes;
    }
}

pub fn builtin_convert_presets() -> Vec<ConvertPreset> {
    vec![
        ConvertPreset {
            name: "Fast AV1".to_owned(),
            fields: ConvertPresetFields {
                convert_target_codec: "av1".to_owned(),
                convert_size_preset: "fast".to_owned(),
                convert_parallel: 2,
                ..Default::default()
            },
        },
        ConvertPreset {
            name: "Quality H.265".to_owned(),
            fields: ConvertPresetFields {
                convert_target_codec: "hevc".to_owned(),
                convert_size_preset: "quality".to_owned(),
                convert_parallel: 1,
                ..Default::default()
            },
        },
    ]
}

pub fn save_user_convert_preset(
    store: &mut ConvertPresetStore,
    preset: ConvertPreset,
) -> Result<()> {
    if let Some(idx) = store.presets.iter().position(|p| p.name == preset.name) {
        store.presets[idx] = preset;
    } else {
        store.presets.push(preset);
    }
    save_convert_presets(store)
}
