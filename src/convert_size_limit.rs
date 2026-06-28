//! Output size limits for the video converter (global defaults + per-queue-item overrides).

use crate::app_parsing::{human_bytes_ui, parse_human_size};
use crate::config::AppSettings;
use crate::models::ConvertQueueItem;

pub const KIND_NONE: &str = "none";
pub const KIND_MIN_SHRINK_PERCENT: &str = "min_shrink_percent";
pub const KIND_MAX_PERCENT_OF_SOURCE: &str = "max_percent_of_source";
pub const KIND_MAX_OUTPUT_BYTES: &str = "max_output_bytes";

pub const VIOLATION_SKIP: &str = "skip";
pub const VIOLATION_FAIL: &str = "fail";
pub const VIOLATION_ENCODE_DELETE: &str = "encode_delete";
pub const VIOLATION_KEEP: &str = "keep";

#[derive(Clone, Debug, PartialEq)]
pub struct ConvertSizeLimit {
    pub kind: String,
    pub value: f64,
    pub violation: String,
}

impl Default for ConvertSizeLimit {
    fn default() -> Self {
        Self {
            kind: KIND_NONE.to_owned(),
            value: 0.0,
            violation: VIOLATION_SKIP.to_owned(),
        }
    }
}

impl ConvertSizeLimit {
    pub fn is_active(&self) -> bool {
        self.kind != KIND_NONE && self.value > 0.0
    }

    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            kind: normalize_kind(&settings.convert_size_limit_kind),
            value: parse_limit_value(
                &settings.convert_size_limit_kind,
                &settings.convert_size_limit_value,
            )
            .unwrap_or(0.0),
            violation: normalize_violation(&settings.convert_size_limit_violation),
        }
    }

    pub fn for_item(settings: &AppSettings, item: &ConvertQueueItem) -> Self {
        let global = Self::from_settings(settings);
        let kind = item.size_limit_kind_override.as_deref().map(normalize_kind);
        let Some(kind) = kind else {
            return global;
        };
        if kind == KIND_NONE {
            return Self::default();
        }
        let value = item
            .size_limit_value_override
            .as_deref()
            .and_then(|raw| parse_limit_value(&kind, raw))
            .or_else(|| {
                if kind == global.kind {
                    Some(global.value)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        let violation = item
            .size_limit_violation_override
            .as_deref()
            .map(normalize_violation)
            .unwrap_or_else(|| global.violation.clone());
        Self {
            kind,
            value,
            violation,
        }
    }

    pub fn max_allowed_bytes(&self, input_bytes: u64) -> Option<u64> {
        if !self.is_active() || input_bytes == 0 {
            return None;
        }
        match self.kind.as_str() {
            KIND_MIN_SHRINK_PERCENT => {
                let pct = self.value.clamp(0.0, 95.0);
                Some((input_bytes as f64 * (1.0 - pct / 100.0)).max(1.0) as u64)
            }
            KIND_MAX_PERCENT_OF_SOURCE => {
                let pct = self.value.clamp(0.0, 1000.0);
                Some((input_bytes as f64 * (pct / 100.0)).max(1.0) as u64)
            }
            KIND_MAX_OUTPUT_BYTES => Some(self.value.max(1.0) as u64),
            _ => None,
        }
    }

    pub fn violates(&self, input_bytes: u64, output_bytes: u64) -> bool {
        let Some(max_allowed) = self.max_allowed_bytes(input_bytes) else {
            return false;
        };
        output_bytes > max_allowed
    }

    pub fn violation_message(&self, input_bytes: u64, output_bytes: u64) -> String {
        let limit_label = self.limit_label(input_bytes);
        format!(
            "Output {} exceeds limit ({limit_label})",
            human_bytes_ui(output_bytes)
        )
    }

    pub fn limit_label(&self, input_bytes: u64) -> String {
        match self.kind.as_str() {
            KIND_MIN_SHRINK_PERCENT => format!("≥{:.0}% shrink from source", self.value),
            KIND_MAX_PERCENT_OF_SOURCE => format!(
                "≤{:.0}% of source ({})",
                self.value,
                human_bytes_ui(input_bytes)
            ),
            KIND_MAX_OUTPUT_BYTES => format!("≤{}", human_bytes_ui(self.value as u64)),
            _ => "none".to_owned(),
        }
    }

    pub fn summary_label(&self) -> String {
        if !self.is_active() {
            return "off".to_owned();
        }
        match self.kind.as_str() {
            KIND_MIN_SHRINK_PERCENT => format!("min shrink {:.0}%", self.value),
            KIND_MAX_PERCENT_OF_SOURCE => format!("max {:.0}% of source", self.value),
            KIND_MAX_OUTPUT_BYTES => format!("max {}", human_bytes_ui(self.value as u64)),
            _ => "off".to_owned(),
        }
    }

    pub fn violation_label(&self) -> &'static str {
        match self.violation.as_str() {
            VIOLATION_FAIL => "fail",
            VIOLATION_ENCODE_DELETE => "encode then delete",
            VIOLATION_KEEP => "keep anyway",
            _ => "skip",
        }
    }

    pub fn uses_pre_encode_gate(&self) -> bool {
        matches!(self.violation.as_str(), VIOLATION_SKIP | VIOLATION_FAIL)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreEncodeDecision {
    Proceed,
    Skip,
    Fail,
}

pub fn pre_encode_decision(
    limit: &ConvertSizeLimit,
    input_bytes: u64,
    estimated_output_bytes: u64,
) -> PreEncodeDecision {
    if !limit.is_active() || !limit.uses_pre_encode_gate() || input_bytes == 0 {
        return PreEncodeDecision::Proceed;
    }
    if !limit.violates(input_bytes, estimated_output_bytes) {
        return PreEncodeDecision::Proceed;
    }
    match limit.violation.as_str() {
        VIOLATION_FAIL => PreEncodeDecision::Fail,
        _ => PreEncodeDecision::Skip,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostEncodeDecision {
    Keep,
    Skip,
    Fail,
}

pub fn post_encode_decision(
    limit: &ConvertSizeLimit,
    input_bytes: u64,
    output_bytes: u64,
) -> PostEncodeDecision {
    if !limit.is_active() || input_bytes == 0 || !limit.violates(input_bytes, output_bytes) {
        return PostEncodeDecision::Keep;
    }
    match limit.violation.as_str() {
        VIOLATION_FAIL => PostEncodeDecision::Fail,
        VIOLATION_SKIP | VIOLATION_ENCODE_DELETE => PostEncodeDecision::Skip,
        _ => PostEncodeDecision::Keep,
    }
}

pub fn normalize_kind(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        KIND_MIN_SHRINK_PERCENT | "min_shrink" => KIND_MIN_SHRINK_PERCENT.to_owned(),
        KIND_MAX_PERCENT_OF_SOURCE | "max_percent" | "max_percent_source" => {
            KIND_MAX_PERCENT_OF_SOURCE.to_owned()
        }
        KIND_MAX_OUTPUT_BYTES | "max_bytes" | "max_size" => KIND_MAX_OUTPUT_BYTES.to_owned(),
        "none" | "off" | "disabled" | "" => KIND_NONE.to_owned(),
        other => other.to_owned(),
    }
}

pub fn normalize_violation(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        VIOLATION_FAIL | "failed" => VIOLATION_FAIL.to_owned(),
        VIOLATION_ENCODE_DELETE | "delete" => VIOLATION_ENCODE_DELETE.to_owned(),
        VIOLATION_KEEP | "warn" | "keep_anyway" => VIOLATION_KEEP.to_owned(),
        _ => VIOLATION_SKIP.to_owned(),
    }
}

pub fn parse_limit_value(kind: &str, raw: &str) -> Option<f64> {
    let kind = normalize_kind(kind);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match kind.as_str() {
        KIND_MIN_SHRINK_PERCENT | KIND_MAX_PERCENT_OF_SOURCE => trimmed.parse::<f64>().ok(),
        KIND_MAX_OUTPUT_BYTES => parse_byte_limit(trimmed).map(|b| b as f64),
        _ => None,
    }
}

fn parse_byte_limit(raw: &str) -> Option<u64> {
    if let Some(b) = parse_human_size(raw) {
        return Some(b);
    }
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if let Some(num) = normalized.strip_suffix('g') {
        return Some((num.trim().parse::<f64>().ok()? * 1_000_000_000.0).round() as u64);
    }
    if let Some(num) = normalized.strip_suffix('m') {
        return Some((num.trim().parse::<f64>().ok()? * 1_000_000.0).round() as u64);
    }
    if let Some(num) = normalized.strip_suffix('k') {
        return Some((num.trim().parse::<f64>().ok()? * 1_000.0).round() as u64);
    }
    normalized.parse().ok()
}

pub fn format_limit_value(kind: &str, value: f64) -> String {
    match normalize_kind(kind).as_str() {
        KIND_MAX_OUTPUT_BYTES => human_bytes_ui(value.max(1.0) as u64),
        KIND_MIN_SHRINK_PERCENT | KIND_MAX_PERCENT_OF_SOURCE => format!("{value:.0}"),
        _ => String::new(),
    }
}

/// Upgrades legacy `convert_min_shrink_percent` and normalizes new limit fields.
pub fn normalize_settings_limits(cfg: &mut AppSettings) {
    cfg.convert_size_limit_kind = normalize_kind(&cfg.convert_size_limit_kind);
    cfg.convert_size_limit_violation = normalize_violation(&cfg.convert_size_limit_violation);

    if cfg.convert_size_limit_kind == KIND_NONE && cfg.convert_min_shrink_percent > 0.0 {
        cfg.convert_size_limit_kind = KIND_MIN_SHRINK_PERCENT.to_owned();
        cfg.convert_size_limit_value = format!("{:.0}", cfg.convert_min_shrink_percent);
        if cfg.convert_size_limit_violation.is_empty() {
            cfg.convert_size_limit_violation = VIOLATION_SKIP.to_owned();
        }
    }

    if cfg.convert_size_limit_kind == KIND_NONE {
        cfg.convert_size_limit_value.clear();
    } else if cfg.convert_size_limit_value.trim().is_empty() {
        if cfg.convert_size_limit_kind == KIND_MIN_SHRINK_PERCENT
            && cfg.convert_min_shrink_percent > 0.0
        {
            cfg.convert_size_limit_value = format!("{:.0}", cfg.convert_min_shrink_percent);
        }
    } else if let Some(parsed) =
        parse_limit_value(&cfg.convert_size_limit_kind, &cfg.convert_size_limit_value)
    {
        cfg.convert_size_limit_value = format_limit_value(&cfg.convert_size_limit_kind, parsed);
    }

    if cfg.convert_size_limit_kind == KIND_MIN_SHRINK_PERCENT {
        if let Some(pct) = parse_limit_value(KIND_MIN_SHRINK_PERCENT, &cfg.convert_size_limit_value)
        {
            cfg.convert_min_shrink_percent = pct.clamp(0.0, 95.0) as f32;
        }
    } else {
        cfg.convert_min_shrink_percent = 0.0;
    }
}

pub fn convert_item_will_skip_size_limit(
    item: &ConvertQueueItem,
    settings: &AppSettings,
    estimated_output_bytes: u64,
) -> bool {
    let limit = ConvertSizeLimit::for_item(settings, item);
    if !limit.is_active() || !limit.uses_pre_encode_gate() || item.input_bytes == 0 {
        return false;
    }
    limit.violates(item.input_bytes, estimated_output_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with(kind: &str, value: &str, violation: &str) -> AppSettings {
        let mut s = AppSettings::default();
        s.convert_size_limit_kind = kind.to_owned();
        s.convert_size_limit_value = value.to_owned();
        s.convert_size_limit_violation = violation.to_owned();
        normalize_settings_limits(&mut s);
        s
    }

    #[test]
    fn migrates_legacy_min_shrink_percent() {
        let mut s = AppSettings::default();
        s.convert_min_shrink_percent = 40.0;
        normalize_settings_limits(&mut s);
        assert_eq!(s.convert_size_limit_kind, KIND_MIN_SHRINK_PERCENT);
        assert_eq!(s.convert_size_limit_value, "40");
    }

    #[test]
    fn min_shrink_limit_caps_output() {
        let limit = ConvertSizeLimit {
            kind: KIND_MIN_SHRINK_PERCENT.to_owned(),
            value: 50.0,
            violation: VIOLATION_SKIP.to_owned(),
        };
        assert_eq!(limit.max_allowed_bytes(1_000), Some(500));
        assert!(limit.violates(1_000, 600));
        assert!(!limit.violates(1_000, 400));
    }

    #[test]
    fn max_percent_of_source_limit() {
        let limit = ConvertSizeLimit {
            kind: KIND_MAX_PERCENT_OF_SOURCE.to_owned(),
            value: 60.0,
            violation: VIOLATION_SKIP.to_owned(),
        };
        assert_eq!(limit.max_allowed_bytes(1_000), Some(600));
        assert!(limit.violates(1_000, 700));
    }

    #[test]
    fn max_output_bytes_parses_human_size() {
        assert_eq!(
            parse_limit_value(KIND_MAX_OUTPUT_BYTES, "500M"),
            Some(500_000_000.0)
        );
    }

    #[test]
    fn per_item_override_disables_with_none_kind() {
        let settings = settings_with(KIND_MIN_SHRINK_PERCENT, "50", VIOLATION_SKIP);
        let mut item = ConvertQueueItem::default();
        item.size_limit_kind_override = Some(KIND_NONE.to_owned());
        let limit = ConvertSizeLimit::for_item(&settings, &item);
        assert!(!limit.is_active());
    }

    #[test]
    fn pre_encode_skip_vs_fail() {
        let skip = ConvertSizeLimit {
            kind: KIND_MIN_SHRINK_PERCENT.to_owned(),
            value: 50.0,
            violation: VIOLATION_SKIP.to_owned(),
        };
        let fail = ConvertSizeLimit {
            violation: VIOLATION_FAIL.to_owned(),
            ..skip.clone()
        };
        assert_eq!(
            pre_encode_decision(&skip, 1_000, 800),
            PreEncodeDecision::Skip
        );
        assert_eq!(
            pre_encode_decision(&fail, 1_000, 800),
            PreEncodeDecision::Fail
        );
        assert_eq!(
            pre_encode_decision(&skip, 1_000, 400),
            PreEncodeDecision::Proceed
        );
    }

    #[test]
    fn encode_delete_skips_pre_check() {
        let limit = ConvertSizeLimit {
            kind: KIND_MIN_SHRINK_PERCENT.to_owned(),
            value: 50.0,
            violation: VIOLATION_ENCODE_DELETE.to_owned(),
        };
        assert!(!limit.uses_pre_encode_gate());
        assert_eq!(
            pre_encode_decision(&limit, 1_000, 900),
            PreEncodeDecision::Proceed
        );
        assert_eq!(
            post_encode_decision(&limit, 1_000, 900),
            PostEncodeDecision::Skip
        );
    }
}
