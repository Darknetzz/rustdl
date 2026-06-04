//! Free and total capacity for the volume backing a filesystem path.

use std::path::{Path, PathBuf};

use crate::app_parsing::human_bytes_ui;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskSpace {
    pub available_bytes: u64,
    pub total_bytes: u64,
    /// Drive letter (`D:`) or mount hint when known.
    pub volume_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskSpaceLevel {
    Ok,
    Low,
    Critical,
}

impl DiskSpace {
    pub fn percent_free(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.available_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    pub fn level(&self) -> DiskSpaceLevel {
        const TWO_GIB: u64 = 2 * 1024 * 1024 * 1024;
        const TEN_GIB: u64 = 10 * 1024 * 1024 * 1024;
        let pct = self.percent_free();
        if self.available_bytes < TWO_GIB || pct < 3.0 {
            DiskSpaceLevel::Critical
        } else if self.available_bytes < TEN_GIB || pct < 10.0 {
            DiskSpaceLevel::Low
        } else {
            DiskSpaceLevel::Ok
        }
    }

    pub fn format_available_total(&self) -> String {
        let suffix = self
            .volume_label
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|label| format!(" ({label})"))
            .unwrap_or_default();
        format!(
            "{} free / {}{}",
            human_bytes_ui(self.available_bytes),
            human_bytes_ui(self.total_bytes),
            suffix
        )
    }
}

/// Resolve a path suitable for querying the destination volume (walks up to an existing directory).
pub fn resolve_query_path(output_dir: &str) -> PathBuf {
    let trimmed = output_dir.trim();
    if trimmed.is_empty() {
        return PathBuf::new();
    }
    let mut path = PathBuf::from(trimmed);
    loop {
        if path.is_dir() {
            return path;
        }
        if !path.pop() {
            return PathBuf::from(trimmed);
        }
    }
}

/// Query available and total bytes for the volume containing `output_dir`.
pub fn query_disk_space(output_dir: &str) -> Option<DiskSpace> {
    let query_path = resolve_query_path(output_dir);
    if query_path.as_os_str().is_empty() {
        return None;
    }
    query_disk_space_at(&query_path).map(|mut space| {
        space.volume_label = volume_label_for_path(output_dir, &query_path);
        space
    })
}

fn volume_label_for_path(output_dir: &str, query_path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        let _ = query_path;
        let trimmed = output_dir.trim();
        if trimmed.len() >= 2 {
            let bytes = trimmed.as_bytes();
            if bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
                return Some(format!("{}:", bytes[0] as char));
            }
        }
        if let Some(prefix) = query_path.components().next() {
            let s = prefix.as_os_str().to_string_lossy();
            if s.len() >= 2 && s.as_bytes().get(1) == Some(&b':') {
                return Some(s.to_string());
            }
        }
        return None;
    }
    #[cfg(not(windows))]
    {
        let _ = output_dir;
        let mount = query_path.to_string_lossy();
        if mount.is_empty() {
            None
        } else {
            Some(mount.into_owned())
        }
    }
}

#[cfg(windows)]
fn query_disk_space_at(path: &Path) -> Option<DiskSpace> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_available = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    // SAFETY: `wide` is NUL-terminated; API reads the path string only.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_available,
            &mut total,
            &mut total_free,
        )
    };
    if ok == 0 || total == 0 {
        return None;
    }
    Some(DiskSpace {
        available_bytes: free_available,
        total_bytes: total,
        volume_label: None,
    })
}

#[cfg(unix)]
fn query_disk_space_at(path: &Path) -> Option<DiskSpace> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    let c_path = CString::new(bytes).ok()?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c_path` is NUL-terminated; `stat` is fully written on success.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    if stat.f_frsize == 0 {
        return None;
    }
    let frsize = stat.f_frsize as u64;
    let available_bytes = stat.f_bavail.saturating_mul(frsize);
    let total_bytes = stat.f_blocks.saturating_mul(frsize);
    if total_bytes == 0 {
        return None;
    }
    Some(DiskSpace {
        available_bytes,
        total_bytes,
        volume_label: None,
    })
}

#[cfg(all(not(windows), not(unix)))]
fn query_disk_space_at(_path: &Path) -> Option<DiskSpace> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_available_total_includes_label() {
        let space = DiskSpace {
            available_bytes: 120 * 1024 * 1024 * 1024,
            total_bytes: 2 * 1024 * 1024 * 1024 * 1024,
            volume_label: Some("D:".to_owned()),
        };
        let line = space.format_available_total();
        assert!(line.contains("free /"));
        assert!(line.contains("(D:)"));
    }

    #[test]
    fn disk_space_level_thresholds() {
        let ok = DiskSpace {
            available_bytes: 200 * 1024 * 1024 * 1024,
            total_bytes: 1000 * 1024 * 1024 * 1024,
            volume_label: None,
        };
        assert_eq!(ok.level(), DiskSpaceLevel::Ok);

        let low = DiskSpace {
            available_bytes: 8 * 1024 * 1024 * 1024,
            total_bytes: 200 * 1024 * 1024 * 1024,
            volume_label: None,
        };
        assert_eq!(low.level(), DiskSpaceLevel::Low);

        let critical = DiskSpace {
            available_bytes: 1024,
            total_bytes: 1000 * 1024 * 1024 * 1024,
            volume_label: None,
        };
        assert_eq!(critical.level(), DiskSpaceLevel::Critical);
    }
}
