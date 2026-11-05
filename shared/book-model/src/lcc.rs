//! Syntax shared by call-number matching and bibliographic consensus.
#[derive(Debug)]
pub struct LccPrefix<'a> {
    pub letters: String,
    pub number: &'a str,
    pub remainder: &'a str,
}

/// Parse the class and number, retaining cutters and item dates separately.
/// Regional law schedules can use four-letter subclasses such as KJCX.
pub fn parse_lcc_prefix(value: &str) -> Option<LccPrefix<'_>> {
    let value = value.trim();
    let bytes = value.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() && cursor < 4 && bytes[cursor].is_ascii_alphabetic() {
        cursor += 1;
    }
    if cursor == 0 || bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let letters = value[..cursor].to_ascii_uppercase();
    while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'\'') {
        cursor += 1;
    }
    let start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == start {
        return None;
    }
    if bytes.get(cursor) == Some(&b'.') {
        let decimal_start = cursor + 1;
        let mut decimal_end = decimal_start;
        while bytes.get(decimal_end).is_some_and(u8::is_ascii_digit) {
            decimal_end += 1;
        }
        if decimal_end > decimal_start {
            cursor = decimal_end;
        }
    }
    Some(LccPrefix { letters, number: &value[start..cursor], remainder: &value[cursor..] })
}
