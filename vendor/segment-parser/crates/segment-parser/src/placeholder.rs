use std::ops::Range;
use structure::PlaceholderSemantic;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedPlaceholder {
    span: Range<usize>,
    semantic: PlaceholderSemantic,
}

impl DetectedPlaceholder {
    fn new(span: Range<usize>, semantic: PlaceholderSemantic) -> Self { Self { span, semantic } }
    pub fn span(&self) -> Range<usize> { self.span.clone() }
    pub fn semantic(&self) -> &PlaceholderSemantic { &self.semantic }
    pub fn into_parts(self) -> (Range<usize>, PlaceholderSemantic) { (self.span, self.semantic) }
}

pub fn detect_placeholders(text: &str) -> Vec<DetectedPlaceholder> {
    let mut detected = Vec::new();
    let mut position = 0usize;
    while position < text.len() {
        let placeholder = placeholder_at(text, position);
        if let Some(placeholder) = placeholder {
            position = placeholder.span.end;
            detected.push(placeholder);
            continue;
        }
        position += text[position..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    detected
}

fn placeholder_at(text: &str, start: usize) -> Option<DetectedPlaceholder> {
    match text.get(start..)?.chars().next()? {
        '$' => dollar_brace_placeholder(text, start),
        '{' => double_brace_placeholder(text, start).or_else(|| brace_placeholder(text, start)),
        '%' => named_printf_placeholder(text, start).or_else(|| environment_placeholder(text, start)).or_else(|| printf_placeholder(text, start)),
        _ => None,
    }
}

fn dollar_brace_placeholder(text: &str, start: usize) -> Option<DetectedPlaceholder> {
    let tail = text.get(start..)?;
    let inner_tail = tail.strip_prefix("${")?;
    let close = bounded_closing_offset(inner_tail, '}')?;
    let key = inner_tail.get(..close)?;
    placeholder_with_key(start..start + 2 + close + 1, key, |key| format!("${{{key}}}"))
}

fn double_brace_placeholder(text: &str, start: usize) -> Option<DetectedPlaceholder> {
    let tail = text.get(start..)?;
    let inner_tail = tail.strip_prefix("{{")?;
    let close = inner_tail.find("}}").filter(|close| *close <= 64)?;
    let key = inner_tail.get(..close)?.trim();
    placeholder_with_key(start..start + 2 + close + 2, key, |key| format!("{{{{{key}}}}}"))
}

fn brace_placeholder(text: &str, start: usize) -> Option<DetectedPlaceholder> {
    let tail = text.get(start..)?;
    if tail.starts_with("{{") {
        return None;
    }
    let inner_tail = tail.strip_prefix('{')?;
    let close = bounded_closing_offset(inner_tail, '}')?;
    let inner = inner_tail.get(..close)?;
    let (key, format) = inner.split_once(':').map_or((inner, None), |(key, format)| (key, Some(format)));
    let key = key.trim();
    if format.is_some_and(|format| !valid_placeholder_format(format)) {
        return None;
    }
    let canonical = |key: &str| match format {
        Some(format) => format!("{{{key}:{format}}}"),
        None => format!("{{{key}}}"),
    };
    placeholder_with_key(start..start + 1 + close + 1, key, canonical)
}

fn named_printf_placeholder(text: &str, start: usize) -> Option<DetectedPlaceholder> {
    let tail = text.get(start..)?;
    let inner_tail = tail.strip_prefix("%(")?;
    let close = inner_tail.find(')').filter(|close| *close <= 64)?;
    let key = inner_tail.get(..close)?;
    let conversion_start = 2 + close + 1;
    let conversion = tail.get(conversion_start..)?.chars().next().filter(|character| printf_conversion(*character))?;
    let end = start + conversion_start + conversion.len_utf8();
    if !placeholder_boundary(text, end) {
        return None;
    }
    placeholder_with_key(start..end, key, |key| format!("%({key}){conversion}"))
}

fn environment_placeholder(text: &str, start: usize) -> Option<DetectedPlaceholder> {
    let tail = text.get(start..)?;
    let inner_tail = tail.strip_prefix('%')?;
    let close = inner_tail.find('%').filter(|close| (2..=64).contains(close))?;
    let key = inner_tail.get(..close)?;
    let valid = key.chars().next().is_some_and(|character| character.is_ascii_uppercase() || character == '_')
        && key.chars().all(|character| character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_');
    valid.then(|| DetectedPlaceholder::new(start..start + 1 + close + 1, PlaceholderSemantic::new(key.to_string(), format!("%{key}%"))))
}

fn printf_placeholder(text: &str, start: usize) -> Option<DetectedPlaceholder> {
    let tail = text.get(start..)?;
    let bytes = tail.as_bytes();
    if bytes.first() != Some(&b'%') || bytes.get(1).is_none_or(|byte| *byte == b'%') {
        return None;
    }
    let digit_end = take_ascii(bytes, 1, |byte| byte.is_ascii_digit());
    let positional = || {
        let key = tail.get(1..digit_end)?;
        (digit_end > 1 && bytes.get(digit_end) != Some(&b'$') && placeholder_boundary(text, start + digit_end))
            .then(|| DetectedPlaceholder::new(start..start + digit_end, PlaceholderSemantic::new(key.to_string(), format!("%{key}"))))
    };
    if digit_end > 1 && bytes.get(digit_end).is_none_or(|byte| !printf_after_width_byte(*byte)) {
        return positional();
    }
    let mut cursor = if digit_end > 1 && bytes.get(digit_end) == Some(&b'$') { digit_end + 1 } else { 1 };
    cursor = take_ascii(bytes, cursor, |byte| matches!(byte, b'-' | b'+' | b' ' | b'#' | b'0' | b'\''));
    cursor = if bytes.get(cursor) == Some(&b'*') { cursor + 1 } else { take_ascii(bytes, cursor, |byte| byte.is_ascii_digit()) };
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        cursor = if bytes.get(cursor) == Some(&b'*') { cursor + 1 } else { take_ascii(bytes, cursor, |byte| byte.is_ascii_digit()) };
    }
    cursor = consume_printf_length(bytes, cursor);
    let Some(conversion) = tail.get(cursor..).and_then(|tail| tail.chars().next()).filter(|character| printf_conversion(*character)) else { return positional(); };
    let end = cursor + conversion.len_utf8();
    if !placeholder_boundary(text, start + end) {
        return positional();
    }
    let raw = tail.get(..end)?;
    let key = if digit_end > 1 && bytes.get(digit_end) == Some(&b'$') { tail.get(1..digit_end).unwrap_or(raw) } else { raw };
    Some(DetectedPlaceholder::new(start..start + end, PlaceholderSemantic::new(key.to_string(), raw.to_string())))
}

fn placeholder_with_key(canonical_span: Range<usize>, key: &str, canonical: impl FnOnce(&str) -> String) -> Option<DetectedPlaceholder> {
    valid_placeholder_key(key).then(|| DetectedPlaceholder::new(canonical_span, PlaceholderSemantic::new(key.to_string(), canonical(key))))
}

fn bounded_closing_offset(text: &str, closing: char) -> Option<usize> { text.char_indices().take_while(|(offset, _)| *offset <= 64).find_map(|(offset, character)| (character == closing).then_some(offset)) }

fn valid_placeholder_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 64 {
        return false;
    }
    if key.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    key.chars().next().is_some_and(|character| character.is_alphabetic() || character == '_')
        && key.chars().all(|character| character.is_alphanumeric() || matches!(character, '_' | '.' | '-'))
}

fn valid_placeholder_format(format: &str) -> bool {
    !format.is_empty() && format.len() <= 24 && format.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '#' | '0' | ',' | '+' | '-'))
}

fn take_ascii(bytes: &[u8], start: usize, predicate: impl Fn(u8) -> bool) -> usize { (start..bytes.len()).take_while(|index| predicate(bytes[*index])).last().map_or(start, |index| index + 1) }

fn consume_printf_length(bytes: &[u8], cursor: usize) -> usize {
    if bytes.get(cursor..cursor + 2).is_some_and(|length| matches!(length, b"hh" | b"ll")) {
        return cursor + 2;
    }
    if bytes.get(cursor).is_some_and(|byte| matches!(byte, b'h' | b'l' | b'j' | b'z' | b't' | b'L')) { cursor + 1 } else { cursor }
}

fn printf_after_width_byte(byte: u8) -> bool { byte == b'$' || byte == b'.' || printf_conversion(char::from(byte)) || matches!(byte, b'h' | b'l' | b'j' | b'z' | b't' | b'L') }

fn printf_conversion(character: char) -> bool { matches!(character, 'd' | 'i' | 'u' | 'f' | 'F' | 'e' | 'E' | 'g' | 'G' | 'x' | 'X' | 'o' | 'c' | 's' | 'p' | 'a' | 'A' | 'n' | '@') }

fn placeholder_boundary(text: &str, end: usize) -> bool { text.get(end..).and_then(|tail| tail.chars().next()).is_none_or(|character| !character.is_alphanumeric() && character != '_') }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_common_text_placeholder_syntaxes() {
        let text = "{name} {{ user.name }} ${count} {0:N2} %s %02d %1$s %1 %(item)s %PATH%";
        let placeholders = detect_placeholders(text);
        assert_eq!(placeholders.iter().map(|placeholder| placeholder.semantic().canonical()).collect::<Vec<_>>(), vec!["{name}", "{{user.name}}", "${count}", "{0:N2}", "%s", "%02d", "%1$s", "%1", "%(item)s", "%PATH%"]);
    }

    #[test]
    fn rejects_percentages_json_and_prose_braces() {
        for text in ["50%", "100 %", "%%", "{}", "{\"name\": 1}", "{two words}", "${two words}", "%xylophone"] {
            assert!(detect_placeholders(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn accepts_unicode_placeholder_keys() {
        let placeholders = detect_placeholders("{användare.namn} ${用户} %(värde)s");
        assert_eq!(placeholders.iter().map(|placeholder| placeholder.semantic().key()).collect::<Vec<_>>(), vec!["användare.namn", "用户", "värde"]);
    }
}
