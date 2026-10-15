use super::*;
use std::ops::Range;

#[cfg(test)]
pub(super) fn parse_url_phrase_at(context: &ParseContext<'_>, start_idx: usize) -> Option<Detection> {
    let token = BoundedToken::new(context.stream(), start_idx)?;
    parse_url_phrase_from_token(context, start_idx, &token)
}

pub(super) fn parse_url_phrase_from_token(_context: &ParseContext<'_>, _start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    let token = token.prefix_through_byte(url_terminal_end(token.text())?)?;
    token.trimmed_with(trim_url_trailing)?.protected_if(NodeKind::Url, valid_url)
}

pub(super) fn parse_email_phrase_from_token(_context: &ParseContext<'_>, _start_idx: usize, token: &BoundedToken<'_>) -> Option<Detection> {
    let token = token.prefix_through_byte(email_terminal_end(token.text())?)?;
    token.trimmed()?.protected_if(NodeKind::Email, valid_email)
}

pub(crate) fn valid_url(text: &str) -> bool { url_terminal_end(text) == Some(text.len()) }

pub(crate) fn url_prefix_len(text: &str) -> Option<usize> { ["https://", "http://", "www."].into_iter().find_map(|prefix| text.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix)).then_some(prefix.len())) }

fn url_terminal_end(text: &str) -> Option<usize> {
    let prefix_end = url_prefix_len(text)?;
    let domain_end = domain_terminal_end(text, prefix_end)?;
    let after_port = text.get(domain_end..).and_then(|suffix| suffix.strip_prefix(':')).and_then(|suffix| {
        let port_len = suffix.bytes().take_while(u8::is_ascii_digit).count();
        (port_len > 0).then_some(domain_end + 1 + port_len)
    }).unwrap_or(domain_end);
    text.get(after_port..)
        .filter(|suffix| suffix.starts_with(['/', '?', '#']))
        .map_or(Some(after_port), |_| Some(text.len()))
}

fn email_terminal_end(text: &str) -> Option<usize> {
    let at = text.find('@')?;
    valid_email_local(text.get(..at)?)
        .then_some(())?;
    domain_terminal_end(text, at + 1)
}

fn domain_terminal_end(text: &str, domain_start: usize) -> Option<usize> {
    let domain_end = text.get(domain_start..)?.char_indices()
        .take_while(|(_, character)| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
        .last()
        .map_or(domain_start, |(offset, character)| domain_start + offset + character.len_utf8());
    let domain = text.get(domain_start..domain_end)?;
    let mut cursor = domain_start;
    let mut terminal_end = None;
    for label in domain.split('.') {
        if label.is_empty() || !valid_domain_label(label) {
            break;
        }
        cursor += label.len();
        if is_terminal_domain_suffix(label) {
            terminal_end = Some(cursor);
        }
        cursor += 1;
    }
    terminal_end
}

fn valid_domain_label(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn is_terminal_domain_suffix(label: &str) -> bool {
    ROOT_ZONE_TOP_LEVEL_DOMAINS.lines().any(|candidate| candidate.eq_ignore_ascii_case(label))
}

/// IANA root-zone database, vendored on 2026-08-03 from
/// https://data.iana.org/TLD/tlds-alpha-by-domain.txt.
const ROOT_ZONE_TOP_LEVEL_DOMAINS: &str = include_str!("tlds.txt");

pub(crate) fn expanded_url_span(text: &str, span: Range<usize>) -> Range<usize> {
    let mut end = span.end;
    while let Some(expanded) = consume_url_chunk(text, end) {
        end = expanded;
    }
    span.start..end
}

fn consume_url_chunk(text: &str, end: usize) -> Option<usize> {
    let suffix = text.get(end..)?;
    let whitespace_len = suffix.len() - suffix.trim_start_matches(char::is_whitespace).len();
    let chunk_start = end + whitespace_len;
    let chunk = text.get(chunk_start..)?;
    (whitespace_len > 0 && chunk.starts_with(['/', '?', '#', '&'])).then_some(())?;
    let chunk_end = chunk.char_indices()
        .take_while(|(_, character)| !character.is_whitespace() && !matches!(character, '<' | '>' | '"' | '\''))
        .last()
        .map_or(chunk_start, |(offset, character)| chunk_start + offset + character.len_utf8());
    let trimmed_end = chunk_start + crate::stream_util::trim_trailing_punctuation(text.get(chunk_start..chunk_end)?).len();
    (trimmed_end > chunk_start).then_some(trimmed_end)
}

fn trim_url_trailing(text: &str, trailing: &str) -> bool {
    match trailing {
        ")" => text.matches(')').count() > text.matches('(').count(),
        "]" => text.matches(']').count() > text.matches('[').count(),
        "}" => text.matches('}').count() > text.matches('{').count(),
        _ => trailing.as_bytes().last().copied().is_some_and(is_trim_punct),
    }
}

fn valid_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else { return false };
    !domain.as_bytes().contains(&b'@') && valid_email_local(local) && valid_email_domain(domain)
}

pub(crate) fn valid_email_local(local: &str) -> bool { !local.is_empty() && !local.starts_with('.') && !local.ends_with('.') && !local.contains("..") && local.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')) }

pub(crate) fn valid_email_domain(domain: &str) -> bool {
    let mut labels = domain.split('.');
    let mut saw_label = false;

    for label in labels.by_ref() {
        saw_label = true;
        if label.is_empty() {
            return false;
        }
        if !valid_domain_label(label) {
            return false;
        }
    }

    if !saw_label {
        return false;
    }
    domain.rsplit('.').next().is_some_and(is_terminal_domain_suffix)
}
