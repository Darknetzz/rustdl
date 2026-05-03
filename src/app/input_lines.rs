use std::collections::HashSet;

use url::Url;

use crate::ytdlp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputLineKind {
    Valid,
    DuplicateInInput,
    DuplicateExisting,
    Invalid,
}

pub(crate) struct InputLineInfo {
    pub(crate) line: String,
    pub(crate) kind: InputLineKind,
}

pub(crate) fn analyze_input_lines(
    lines: &[String],
    existing_keys: &HashSet<String>,
) -> Vec<InputLineInfo> {
    let mut seen_input = HashSet::new();
    let mut out = Vec::new();
    for line in lines {
        let normalized = ytdlp::normalize_url_for_dedupe(line);
        let parsed_valid = Url::parse(line).is_ok();
        let kind = if !parsed_valid {
            InputLineKind::Invalid
        } else if !normalized.is_empty() && existing_keys.contains(&normalized) {
            InputLineKind::DuplicateExisting
        } else if !normalized.is_empty() && seen_input.contains(&normalized) {
            InputLineKind::DuplicateInInput
        } else {
            InputLineKind::Valid
        };
        if !normalized.is_empty() {
            seen_input.insert(normalized);
        }
        out.push(InputLineInfo {
            line: line.clone(),
            kind,
        });
    }
    out
}

/// True when every non-empty input line is a duplicate (within the paste or vs the queue).
pub(crate) fn is_only_duplicate_lines(info: &[InputLineInfo]) -> bool {
    !info.is_empty()
        && info.iter().all(|line| {
            matches!(
                line.kind,
                InputLineKind::DuplicateInInput | InputLineKind::DuplicateExisting
            )
        })
}

/// If the last line is a valid URL and the edit looks like a paste (paste event or a large
/// append-only insert), append `\n` so the next paste starts on a new line.
pub(crate) fn append_newline_after_pasted_valid_url(
    input: &mut String,
    prev: &str,
    paste_event: bool,
    url_field_has_focus: bool,
) -> bool {
    if input.ends_with('\n') {
        return false;
    }
    let last_line = input.lines().last().unwrap_or("").trim();
    if last_line.is_empty() {
        return false;
    }
    if Url::parse(last_line).is_err() {
        return false;
    }
    let sizable_append = input.len().saturating_sub(prev.len()) >= 10;
    let append_only = input.starts_with(prev) && input.len() > prev.len();
    let trigger = (paste_event && url_field_has_focus) || (sizable_append && append_only);
    if trigger {
        input.push('\n');
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        analyze_input_lines, append_newline_after_pasted_valid_url, is_only_duplicate_lines,
        InputLineKind,
    };

    #[test]
    fn marks_duplicate_in_input() {
        let lines = vec![
            "https://example.com/a".to_owned(),
            "https://example.com/a".to_owned(),
        ];
        let out = analyze_input_lines(&lines, &HashSet::new());
        assert_eq!(out[0].kind, InputLineKind::Valid);
        assert_eq!(out[1].kind, InputLineKind::DuplicateInInput);
    }

    #[test]
    fn marks_duplicate_against_existing_keys() {
        let mut existing = HashSet::new();
        existing.insert("youtube:vid".to_owned());
        let lines = vec!["https://www.youtube.com/watch?v=vid".to_owned()];
        let out = analyze_input_lines(&lines, &existing);
        assert_eq!(out[0].kind, InputLineKind::DuplicateExisting);
        assert!(is_only_duplicate_lines(&out));
    }

    #[test]
    fn only_duplicate_false_when_mixed_or_invalid() {
        let lines = vec![
            "https://example.com/a".to_owned(),
            "not a url".to_owned(),
        ];
        let out = analyze_input_lines(&lines, &HashSet::new());
        assert!(!is_only_duplicate_lines(&out));
    }

    #[test]
    fn newline_after_paste_event_when_focused() {
        let mut s = "https://example.com".to_owned();
        assert!(append_newline_after_pasted_valid_url(
            &mut s, "", true, true
        ));
        assert_eq!(s, "https://example.com\n");
    }

    #[test]
    fn newline_after_large_append_without_paste_flag() {
        let mut s = "https://example.com/abcd".to_owned();
        assert!(append_newline_after_pasted_valid_url(
            &mut s, "", false, true
        ));
        assert_eq!(s, "https://example.com/abcd\n");
    }

    #[test]
    fn no_newline_when_typing_one_char() {
        let mut s = "https://youtu.be/xxxxx".to_owned();
        assert!(!append_newline_after_pasted_valid_url(
            &mut s,
            "https://youtu.be/xxxx",
            false,
            true
        ));
    }
}
