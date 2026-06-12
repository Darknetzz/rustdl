//! Saved downloader queue snapshots (name + URLs + per-item overrides).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::QueueItem;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueTemplateItem {
    pub source_line: String,
    pub format_override: Option<String>,
    pub profile_override: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueTemplate {
    pub name: String,
    pub items: Vec<QueueTemplateItem>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueueTemplateStore {
    pub templates: Vec<QueueTemplate>,
}

pub fn queue_templates_dir() -> PathBuf {
    crate::config::rustdl_config_dir().join("queue_templates")
}

fn template_path(name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    queue_templates_dir().join(format!("{safe}.json"))
}

pub fn save_queue_template(template: &QueueTemplate) -> Result<()> {
    fs::create_dir_all(queue_templates_dir()).context("create queue_templates dir")?;
    let path = template_path(&template.name);
    let raw = serde_json::to_string_pretty(template).context("serialize queue template")?;
    fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load_queue_template(name: &str) -> Result<QueueTemplate> {
    let path = template_path(name);
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).context("parse queue template JSON")
}

pub fn list_queue_templates() -> Vec<String> {
    let dir = queue_templates_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_owned());
            }
        }
    }
    names.sort();
    names
}

pub fn queue_template_from_items(name: &str, items: &[QueueItem]) -> QueueTemplate {
    QueueTemplate {
        name: name.to_owned(),
        items: items
            .iter()
            .filter(|it| !it.source_line.trim().is_empty())
            .map(|it| QueueTemplateItem {
                source_line: it.source_line.clone(),
                format_override: it.format_override.clone(),
                profile_override: it.profile_override.clone(),
            })
            .collect(),
    }
}

pub fn template_item_urls(template: &QueueTemplate) -> Vec<String> {
    template
        .items
        .iter()
        .map(|it| it.source_line.clone())
        .collect()
}
