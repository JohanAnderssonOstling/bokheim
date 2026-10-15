use super::*;

pub(super) fn parse_non_translatable_phrase_from_token(context: &ParseContext<'_>, _start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    let token = token.before_first_apostrophe()?.trimmed()?;
    let text = token.text();
    if context.knows_non_translatable(text) {
        return token.protected_if(NodeKind::Opaque, |_| true);
    }
    (text.contains('_') || text.chars().any(char::is_uppercase)).then_some(())?;
    token.protected_if(NodeKind::Opaque, |text| text.chars().count() >= 3 && looks_like_non_translatable_identifier(text))
}

pub(super) fn parse_uuid_phrase_from_token(context: &ParseContext<'_>, start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let detected = token.trimmed()?.detected_if(NodeKind::Opaque, looks_like_uuid)?;
    let units_end = detected.units_end();
    if start_idx > 0 && is_punct_text(stream, start_idx - 1, "{") && is_punct_text(stream, units_end, "}") {
        return None;
    }
    Some(Detection::protected(detected))
}

pub(super) fn parse_ip_address_phrase_from_token(context: &ParseContext<'_>, start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    if !allows_boundary_start(context, start_idx) {
        return None;
    }
    let detected = token.trimmed()?.detected_if(NodeKind::IpAddress, looks_like_ip_address)?;
    BoundaryPolicy::IpAddress.allows_end(context, detected.units_end()).then(|| Detection::protected(detected))
}

pub(super) fn parse_hex_color_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    if !is_punct_text(stream, start_idx, "#") {
        return None;
    }
    let value_idx = start_idx + 1;
    let detected = BoundedToken::new(stream, value_idx)?.trimmed()?.detected_if(NodeKind::Opaque, looks_like_hex_color_value)?.with_units_start(stream, start_idx);
    Some(Detection::protected(detected))
}

fn looks_like_hex_color_value(text: &str) -> bool {
    matches!(text.len(), 3 | 4 | 6 | 8) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn looks_like_uuid(text: &str) -> bool {
    let mut parts = text.split('-');
    [8usize, 4, 4, 4, 12]
        .into_iter()
        .all(|len| parts.next().is_some_and(|part| part.len() == len && part.bytes().all(|b| b.is_ascii_hexdigit())))
        && parts.next().is_none()
}

pub(super) fn looks_like_ip_address(text: &str) -> bool {
    let (address, port) = text.rsplit_once(':').map_or((text, None), |(address, port)| (address, Some(port)));
    let valid_octet = |part: &str| !part.is_empty() && part.len() <= 3 && part.bytes().all(|byte| byte.is_ascii_digit()) && part.parse::<u16>().ok().is_some_and(|value| value <= 255);
    let valid_port = |part: &str| !part.is_empty() && part.len() <= 5 && part.bytes().all(|byte| byte.is_ascii_digit()) && part.parse::<u16>().ok().is_some_and(|value| value > 0);
    let mut parts = address.split('.');
    (0..4).all(|_| parts.next().is_some_and(valid_octet)) && parts.next().is_none() && port.is_none_or(valid_port)
}

pub(super) fn parse_path_phrase_from_token(_context: &ParseContext<'_>, _start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> { token.trimmed()?.protected_if(NodeKind::Opaque, looks_like_path) }

pub(super) fn parse_code_identifier_phrase_from_token(_context: &ParseContext<'_>, _start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> { token.trimmed()?.protected_if(NodeKind::Opaque, looks_like_code_identifier) }

pub(super) fn parse_compact_alnum_code_phrase_from_token(context: &ParseContext<'_>, start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    let stream = context.stream();
    let units = &structure::parser_api::units(stream);
    if start_idx > 0
        && units.get(start_idx - 1).is_some_and(|unit| unit.kind() != UnitKind::Whitespace && !unit.is_tag())
    {
        return None;
    }

    token.trimmed()?.protected_if(NodeKind::Opaque, looks_like_compact_alnum_code)
}

fn looks_like_compact_alnum_code(text: &str) -> bool {
    if !(3..=24).contains(&text.chars().count()) {
        return false;
    }
    let mut parts = text.split('.');
    let Some(head) = parts.next() else {
        return false;
    };
    let mut suffixes = parts.peekable();
    let has_suffix = suffixes.peek().is_some();
    if !looks_like_compact_alnum_head(head) || looks_like_lowercase_word_with_trailing_reference(head) || suffixes.any(|part| part.is_empty() || !part.chars().all(char::is_numeric)) {
        return false;
    }
    !has_suffix || !looks_like_prefixed_version_head(head)
}

fn looks_like_compact_alnum_head(text: &str) -> bool { text.chars().all(char::is_alphanumeric) && text.chars().next().is_some_and(char::is_alphabetic) && text.chars().any(char::is_alphabetic) && text.chars().any(char::is_numeric) }

fn looks_like_lowercase_word_with_trailing_reference(text: &str) -> bool {
    let digit_start = text.char_indices().find_map(|(index, character)| character.is_numeric().then_some(index)).unwrap_or(text.len());
    let (word, digits) = text.split_at(digit_start);
    word.chars().count() >= 4 && word.chars().all(char::is_lowercase) && (1..=2).contains(&digits.chars().count()) && digits.chars().all(char::is_numeric)
}

fn looks_like_prefixed_version_head(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(|first| matches!(first, 'v' | 'V')) && characters.clone().next().is_some() && characters.all(|character| character.is_numeric())
}

fn looks_like_code_identifier(text: &str) -> bool {
    if text.chars().count() > 40 || !(text.contains('-') || text.contains('_')) {
        return false;
    }
    if !text.chars().any(char::is_numeric) || !text.chars().any(char::is_uppercase) {
        return false;
    }
    let mut parts = text.split(['-', '_']);
    let Some(first) = parts.next() else {
        return false;
    };
    let valid_prefix = |part: &str| (2..=10).contains(&part.chars().count()) && part.chars().all(char::is_uppercase);
    let valid_part = |part: &str| !part.is_empty() && part.chars().count() <= 20 && part.chars().all(|character| character.is_uppercase() || character.is_numeric());
    valid_prefix(first) && parts.all(valid_part)
}

fn looks_like_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() >= 4 && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && is_path_separator(bytes[2])
    {
        return valid_path_tail(&text[3..], 1);
    }
    if bytes.len() >= 5 && is_path_separator(bytes[0])
        && is_path_separator(bytes[1])
    {
        return valid_path_tail(&text[2..], 2);
    }
    if let Some(tail) = text.strip_prefix('/') {
        return valid_path_tail(tail, 2) && (has_known_unix_root(tail) || has_filename_extension(tail));
    }
    text.strip_prefix("~/")
        .or_else(|| text.strip_prefix("~\\"))
        .or_else(|| text.strip_prefix("./"))
        .or_else(|| text.strip_prefix(".\\"))
        .or_else(|| text.strip_prefix("../"))
        .or_else(|| text.strip_prefix("..\\"))
        .is_some_and(|tail| valid_path_tail(tail, 1))
}

fn valid_path_tail(tail: &str, min_segments: usize) -> bool {
    tail.split(['/', '\\'])
        .try_fold(0usize, |count, segment| (!segment.is_empty() && segment.chars().all(|character| character.is_alphanumeric() || matches!(character, '.' | '_' | '-' | '+' | '~' | '@'))).then_some(count + 1))
        .is_some_and(|segments| segments >= min_segments)
}

fn has_known_unix_root(tail: &str) -> bool {
    tail.split(['/', '\\']).next().is_some_and(|first| {
        matches!(first, "bin" | "sbin" | "usr" | "etc" | "var" | "opt" | "home" | "tmp" | "dev" | "proc" | "sys" | "mnt" | "media" | "run" | "lib")
    })
}

fn has_filename_extension(tail: &str) -> bool {
    tail.split(['/', '\\']).any(|segment| {
        segment.rsplit_once('.').is_some_and(|(_, ext)| {
            (1..=8).contains(&ext.chars().count()) && ext.chars().all(char::is_alphanumeric)
        })
    })
}

fn is_path_separator(byte: u8) -> bool { matches!(byte, b'/' | b'\\') }

fn looks_like_non_translatable_identifier(text: &str) -> bool {
    if text.contains('_') {
        return is_snake_case_like(text);
    }
    if text.contains('-') || text.chars().any(char::is_whitespace) {
        return false;
    }
    let has_lower = text.chars().any(char::is_lowercase);
    let has_upper = text.chars().any(char::is_uppercase);
    if !has_lower || !has_upper {
        return false;
    }
    let mut prev_lower = false;
    text.chars().any(|character| { let transition = prev_lower && character.is_uppercase(); prev_lower = character.is_lowercase(); transition })
}

fn is_snake_case_like(text: &str) -> bool {
    if text.starts_with('_') || text.ends_with('_') {
        return false;
    }
    text.split('_').all(|part| !part.is_empty() && part.chars().all(char::is_alphanumeric))
        && text.chars().any(char::is_alphabetic)
}

#[cfg(test)]
mod tests;
