//! Portable file components and bounded UTF-8 names.


pub(crate) const MAX_FILE_NAME_BYTES: usize = 240;

pub(crate) fn is_reserved(value: &str) -> bool {
    // Windows reserves these device stems even when followed by an extension.
    let stem = value.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT")).is_some_and(|number| matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
}

pub fn validate_component(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || matches!(value, "." | "..") {
        return Err("name must not be empty, '.' or '..'".to_owned());
    }
    if value.chars().any(|character| character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')) {
        return Err("name contains a character that is not portable across supported devices".to_owned());
    }
    if value.ends_with([' ', '.']) || is_reserved(value) {
        return Err("name is reserved or has an unsupported trailing character".to_owned());
    }
    if value.len() > MAX_FILE_NAME_BYTES {
        return Err(format!("name must not exceed {MAX_FILE_NAME_BYTES} UTF-8 bytes"));
    }
    Ok(value.to_owned())
}

pub(crate) fn truncate_utf8(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
