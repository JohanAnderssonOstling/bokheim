//! Folder component projection v1. These rules are part of replica behavior.
//! Character classes are explicit so compiler Unicode updates cannot change names.
use crate::filenames::{MAX_FILE_NAME_BYTES, is_reserved, truncate_utf8};

fn whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{0085}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn trim(value: &str) -> &str {
    value.trim_start_matches(whitespace).trim_end_matches(|c| c == '.' || whitespace(c))
}

/// A bounded, portable component derived from intent; callers retain the intent.
pub fn project_folder_name(requested: &str) -> String {
    candidate(requested, "")
}

pub(crate) fn numbered(requested: &str, number: usize) -> String {
    candidate(requested, &format!(" {number}"))
}

fn candidate(requested: &str, suffix: &str) -> String {
    let sanitized = requested.trim_matches(whitespace).chars().map(|c| if matches!(c, '\u{0000}'..='\u{001f}' | '\u{007f}'..='\u{009f}' | '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c }).collect::<String>();
    let capped = truncate_utf8(trim(&sanitized), MAX_FILE_NAME_BYTES - suffix.len());
    // Truncation can expose trailing dots/spaces or a reserved name.
    let base = trim(&capped).trim_start_matches(|c| c == '.' || whitespace(c));
    let base = if base.is_empty() || is_reserved(base) { "Folder" } else { base };
    format!("{base}{suffix}")
}
