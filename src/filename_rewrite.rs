//! Post-download / post-encode find-and-replace in file stems.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const SIDECAR_EXTENSIONS: &[&str] = &[
    "info.json",
    "description",
    "annotations.xml",
    "meta.json",
    "vtt",
    "srt",
    "ass",
    "lrc",
    "sha256.txt",
];

/// Replace every occurrence of `find` in the file stem; empty `find` is a no-op.
pub fn apply_filename_find_replace(
    path: &Path,
    find: &str,
    replace: &str,
) -> Result<Option<PathBuf>> {
    let find = find.trim();
    if find.is_empty() || !path.is_file() {
        return Ok(None);
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(None);
    };
    if !stem.contains(find) {
        return Ok(None);
    }
    let new_stem = stem.replace(find, replace);
    if new_stem == stem {
        return Ok(None);
    }
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let target = unique_path_for_stem(parent, &new_stem, &ext);
    fs::rename(path, &target)
        .with_context(|| format!("rename {} -> {}", path.display(), target.display()))?;
    rename_matching_sidecars(parent, stem, parent, &new_stem);
    Ok(Some(target))
}

fn unique_path_for_stem(parent: &Path, stem: &str, ext: &str) -> PathBuf {
    let base = parent.join(format!("{stem}{ext}"));
    if !base.exists() {
        return base;
    }
    for i in 2..=999 {
        let candidate = parent.join(format!("{stem}-dup{i}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-dup{ext}"))
}

fn rename_matching_sidecars(
    old_parent: &Path,
    old_stem: &str,
    new_parent: &Path,
    new_stem: &str,
) {
    let Ok(entries) = fs::read_dir(old_parent) else {
        return;
    };
    let prefix = format!("{old_stem}.");
    for entry in entries.flatten() {
        let sidecar = entry.path();
        if !sidecar.is_file() {
            continue;
        }
        let Some(fname) = sidecar.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !fname.starts_with(&prefix) {
            continue;
        }
        let suffix = fname.strip_prefix(&prefix).unwrap_or(fname);
        if !SIDECAR_EXTENSIONS
            .iter()
            .any(|ext| suffix.eq_ignore_ascii_case(ext))
        {
            continue;
        }
        let dest = new_parent.join(format!("{new_stem}.{suffix}"));
        if sidecar == dest {
            continue;
        }
        if dest.exists() {
            let _ = fs::remove_file(&dest);
        }
        let _ = fs::rename(&sidecar, &dest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn replace_in_stem_renames_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foo-abc123.mp4");
        File::create(&path).unwrap();
        let out = apply_filename_find_replace(&path, "-abc123", "").unwrap();
        let renamed = out.expect("renamed");
        assert_eq!(renamed.file_name().unwrap(), "foo.mp4");
        assert!(!path.exists());
        assert!(renamed.is_file());
    }

    #[test]
    fn empty_find_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("video.mp4");
        File::create(&path).unwrap();
        assert!(apply_filename_find_replace(&path, "", "x").unwrap().is_none());
        assert!(path.is_file());
    }

    #[test]
    fn renames_matching_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip-av1.mp4");
        File::create(&path).unwrap();
        let sidecar = dir.path().join("clip-av1.srt");
        File::create(&sidecar).unwrap();
        let out = apply_filename_find_replace(&path, "-av1", "").unwrap();
        let renamed = out.expect("renamed");
        assert_eq!(renamed.file_name().unwrap(), "clip.mp4");
        assert!(dir.path().join("clip.srt").is_file());
    }
}
