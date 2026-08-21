//! Host CPU, RAM, and (on Windows) GPU utilization for the header status row.

use std::time::{Duration, Instant};

use sysinfo::System;

/// Latest sampled utilization percentages (0–100).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SystemUsageSnapshot {
    pub cpu_percent: Option<f32>,
    pub ram_percent: Option<f32>,
    pub gpu_percent: Option<f32>,
}

pub struct SystemUsageMonitor {
    system: System,
    snapshot: SystemUsageSnapshot,
    last_poll: Option<Instant>,
    #[cfg(windows)]
    gpu: WindowsGpuUsage,
}

impl SystemUsageMonitor {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_memory();
        Self {
            system,
            snapshot: SystemUsageSnapshot::default(),
            last_poll: None,
            #[cfg(windows)]
            gpu: WindowsGpuUsage::new(),
        }
    }

    pub fn snapshot(&self) -> SystemUsageSnapshot {
        self.snapshot
    }

    pub fn maybe_poll(&mut self) {
        const INTERVAL: Duration = Duration::from_millis(1500);
        let now = Instant::now();
        let due = match self.last_poll {
            None => true,
            Some(t) => now.saturating_duration_since(t) >= INTERVAL,
        };
        if !due {
            return;
        }
        self.last_poll = Some(now);

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();

        let cpu = self.system.global_cpu_usage();
        let cpu_percent = if cpu.is_finite() && cpu >= 0.0 {
            Some(cpu.clamp(0.0, 100.0))
        } else {
            None
        };

        let total = self.system.total_memory();
        let ram_percent = if total > 0 {
            let used = self.system.used_memory();
            Some(((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32)
        } else {
            None
        };

        #[cfg(windows)]
        let gpu_percent = self.gpu.poll();
        #[cfg(not(windows))]
        let gpu_percent = None;

        self.snapshot = SystemUsageSnapshot {
            cpu_percent,
            ram_percent,
            gpu_percent,
        };
    }
}

impl Default for SystemUsageMonitor {
    fn default() -> Self {
        Self::new()
    }
}

pub fn usage_level_color(percent: f32) -> eframe::egui::Color32 {
    use eframe::egui::Color32;
    if percent >= 90.0 {
        Color32::from_rgb(229, 57, 53)
    } else if percent >= 75.0 {
        Color32::from_rgb(255, 167, 38)
    } else {
        Color32::from_rgb(129, 199, 132)
    }
}

pub fn format_usage_percent(value: Option<f32>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{:.0}%", v.clamp(0.0, 100.0)),
        Some(_) | None => "…".to_owned(),
    }
}

#[cfg(windows)]
struct WindowsGpuUsage {
    query: isize,
    counter: isize,
    active: bool,
    /// PDH needs two samples before utilization values are valid.
    primed: bool,
}

#[cfg(windows)]
impl WindowsGpuUsage {
    fn new() -> Self {
        use windows_sys::Win32::System::Performance::{
            PdhAddCounterW, PdhCloseQuery, PdhOpenQueryW,
        };

        let mut query = 0isize;
        let mut counter = 0isize;
        let path = windows_sys::core::w!("\\GPU Engine(*)\\Utilization Percentage");
        let mut active = false;
        unsafe {
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) == 0
                && PdhAddCounterW(query, path, 0, &mut counter) == 0
            {
                active = true;
            } else if query != 0 {
                PdhCloseQuery(query);
                query = 0;
            }
        }
        Self {
            query,
            counter,
            active,
            primed: false,
        }
    }

    fn poll(&mut self) -> Option<f32> {
        if !self.active {
            return None;
        }
        use windows_sys::Win32::System::Performance::{
            PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_CSTATUS_VALID_DATA,
            PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
        };

        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            if !self.primed {
                self.primed = true;
                return None;
            }

            let mut buffer_size = 0u32;
            let mut item_count = 0u32;
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                std::ptr::null_mut(),
            );
            if status != PDH_MORE_DATA || buffer_size == 0 || item_count == 0 {
                return None;
            }

            // Buffer holds items plus the null-terminated instance name strings.
            let mut buffer = vec![0u8; buffer_size as usize];
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                buffer.as_mut_ptr().cast(),
            );
            if status != 0 {
                return None;
            }

            let items = std::slice::from_raw_parts(
                buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                item_count as usize,
            );
            let mut max_usage = 0.0f32;
            for item in items {
                if item.FmtValue.CStatus != PDH_CSTATUS_VALID_DATA {
                    continue;
                }
                let value = item.FmtValue.Anonymous.doubleValue;
                if value.is_finite() && value > 0.0 {
                    max_usage = max_usage.max(value as f32);
                }
            }
            Some(max_usage.clamp(0.0, 100.0))
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsGpuUsage {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Performance::PdhCloseQuery;
        if self.query != 0 {
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_usage_percent_rounds() {
        assert_eq!(format_usage_percent(Some(42.4)), "42%");
        assert_eq!(format_usage_percent(None), "…");
    }

    #[test]
    fn usage_level_color_thresholds() {
        use eframe::egui::Color32;
        assert_eq!(usage_level_color(50.0), Color32::from_rgb(129, 199, 132));
        assert_eq!(usage_level_color(80.0), Color32::from_rgb(255, 167, 38));
        assert_eq!(usage_level_color(95.0), Color32::from_rgb(229, 57, 53));
    }
}
